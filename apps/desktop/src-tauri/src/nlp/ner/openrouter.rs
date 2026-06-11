use serde::Deserialize;

use super::types::{sanitize_entity_value, Entity, EntitySource, EntityType};
use super::{find_entity_span, is_suppressed_by_protected, normalize_entity_value};
use crate::nlp::chunking::{chunk_text, MAX_CHARS as MAX_NLP_CHARS};

pub const DEFAULT_OPENROUTER_NER_MODEL: &str = "google/gemma-4-26b-a4b-it";
pub const OPENROUTER_NER_MODEL_SETTING_KEY: &str = "openrouter_ner_model";
/// Default `max_tokens` for NER calls. Entity arrays are verbose, and the
/// previous 1024 default truncated long entity lists mid-array.
pub const DEFAULT_NER_MAX_TOKENS: i32 = 4096;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum NerPayload {
    Array(Vec<RawNerEntity>),
    Object { entities: Vec<RawNerEntity> },
}

#[derive(Debug, Deserialize)]
struct RawNerEntity {
    #[serde(
        default,
        alias = "entity",
        alias = "text",
        alias = "entidad",
        alias = "valor",
        alias = "nombre"
    )]
    value: String,
    #[serde(
        default,
        alias = "entity_type",
        alias = "category",
        alias = "label",
        alias = "tipo",
        alias = "categoria",
        alias = "categoría",
        alias = "clase"
    )]
    #[serde(rename = "type")]
    entity_type: String,
    #[serde(default, alias = "inicio")]
    start_offset: Option<usize>,
    #[serde(default, alias = "fin")]
    end_offset: Option<usize>,
    #[serde(default, alias = "confianza", alias = "score")]
    confidence: Option<f32>,
}

pub fn build_ner_prompt(text: &str) -> String {
    format!(
        "Extraé entidades nombradas del texto histórico. Devolvé SOLO JSON válido, sin markdown. \
Usá exclusivamente estas categorías: PER, LOC, ORG, DATE, MISC. \
Formato: [{{\"value\":\"...\",\"type\":\"PER|LOC|ORG|DATE|MISC\",\"start_offset\":0,\"end_offset\":0,\"confidence\":0.95}}]. \
Si no hay entidades, devolvé []. No inventes entidades ni uses categorías fuera del contrato.\n\nTexto:\n{text}"
    )
}

fn render_ner_prompt(template: Option<&str>, text: &str) -> String {
    match template.map(str::trim).filter(|value| !value.is_empty()) {
        Some(template) if template.contains("{text}") => template.replace("{text}", text),
        Some(template) => format!("{template}\n\nTexto:\n{text}"),
        None => build_ner_prompt(text),
    }
}

pub async fn extract_entities_with_openrouter(
    api_key: String,
    model_name: String,
    text: &str,
    protected_entities: &[Entity],
    prompt_template: Option<String>,
    params: Option<crate::llm::openrouter::GenerationParams>,
) -> Result<Vec<Entity>, String> {
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return Err(openrouter_ner_unavailable(
            "OpenRouter API key no configurada para NER",
        ));
    }

    let model_name = normalize_model_name(&model_name);
    let client = crate::llm::openrouter::OpenRouterClient::try_new(api_key, model_name.clone())
        .map_err(|error| openrouter_ner_unavailable(&error))?;
    let prompt = render_ner_prompt(prompt_template.as_deref(), text);
    let params = params.unwrap_or_else(|| {
        crate::llm::openrouter::GenerationParams::with_defaults(DEFAULT_NER_MAX_TOKENS, 0.3)
    });
    eprintln!(
        "[nlp/ner] Runtime OpenRouter NER prompt source={}, params: temperature={}, max_tokens={}",
        if prompt_template
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            "user override"
        } else {
            "default"
        },
        params.temperature,
        params.max_tokens,
    );
    let raw = client
        .generate_with_params(&prompt, &params)
        .await
        .map_err(|error| openrouter_ner_unavailable(&error))?;

    parse_openrouter_entities(text, protected_entities, &raw, &model_name)
}

/// Extract entities from long `text` by chunking and aggregating per-chunk
/// results. Each chunk is sent to OpenRouter separately and the returned
/// entity offsets are rebased by the chunk's start position in the source
/// document, so consumers can treat the aggregate as if it came from a single
/// extraction pass.
#[allow(dead_code)]
pub async fn extract_entities_with_openrouter_chunked(
    api_key: String,
    model_name: String,
    text: &str,
    protected_entities: &[Entity],
    prompt_template: Option<String>,
    params: Option<crate::llm::openrouter::GenerationParams>,
) -> Result<Vec<Entity>, String> {
    let chunks = chunk_text(text);
    if chunks.len() > 1 {
        eprintln!(
            "[nlp/ner] text exceeded {MAX_NLP_CHARS} chars, splitting into {} chunks",
            chunks.len()
        );
    }

    let mut all_entities = Vec::new();
    for chunk in &chunks {
        let mut entities = extract_entities_with_openrouter(
            api_key.clone(),
            model_name.clone(),
            &chunk.text,
            protected_entities,
            prompt_template.clone(),
            params.clone(),
        )
        .await?;

        if chunk.start > 0 {
            for entity in entities.iter_mut() {
                entity.start_offset = entity.start_offset.saturating_add(chunk.start);
                entity.end_offset = entity.end_offset.saturating_add(chunk.start);
            }
        }

        all_entities.append(&mut entities);
    }

    Ok(all_entities)
}

pub fn parse_openrouter_entities(
    text: &str,
    protected_entities: &[Entity],
    raw_response: &str,
    model_name: &str,
) -> Result<Vec<Entity>, String> {
    let content = strip_markdown_fences(raw_response);
    let json = extract_json_payload(&content)?;
    let payload: NerPayload = match serde_json::from_str(json) {
        Ok(payload) => payload,
        Err(error) => {
            let Some(salvaged) = salvage_truncated_array(json, &error) else {
                return Err(format!("OpenRouter NER failed to parse JSON: {error}."));
            };
            eprintln!(
                "[nlp/ner] response looks truncated by max_tokens ({error}); salvaged {} of {} bytes — results may be partial",
                salvaged.len(),
                json.len()
            );
            serde_json::from_str(&salvaged)
                .map_err(|error| format!("OpenRouter NER failed to parse JSON: {error}."))?
        }
    };

    let raw_entities = match payload {
        NerPayload::Array(items) => items,
        NerPayload::Object { entities } => entities,
    };

    let mut deduped_keys = std::collections::HashSet::new();
    let mut entities = Vec::new();
    for raw in raw_entities {
        let value = sanitize_entity_value(&raw.value);
        if value.is_empty() {
            continue;
        }
        let Some(entity_type) = parse_openrouter_entity_type(&raw.entity_type) else {
            continue;
        };
        let (start_offset, end_offset) = match (raw.start_offset, raw.end_offset) {
            (Some(start), Some(end)) if end >= start => (start, end),
            _ => find_entity_span(text, &value).unwrap_or((0, 0)),
        };

        let entity = Entity {
            entity_type,
            value,
            start_offset,
            end_offset,
            confidence: raw.confidence.unwrap_or(0.95).clamp(0.0, 1.0),
            source: EntitySource::Llm,
            model_name: Some(model_name.to_string()),
        };

        if is_suppressed_by_protected(&entity, protected_entities) {
            continue;
        }

        let key = (
            normalize_entity_value(&entity.value),
            entity.entity_type.as_str().to_string(),
        );
        if deduped_keys.insert(key) {
            entities.push(entity);
        }
    }

    entities.sort_by_key(|entity| entity.start_offset);
    Ok(entities)
}

pub fn normalize_model_name(model_name: &str) -> String {
    let trimmed = model_name.trim();
    if trimmed.is_empty() {
        DEFAULT_OPENROUTER_NER_MODEL.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Wrap an OpenRouter failure for NER consumers. The "Configure OpenRouter
/// API key/model." hint is only appended for auth/config-class errors —
/// transient provider failures (5xx, timeouts, rate limits) keep a neutral
/// prefix so the hint doesn't mislead users whose configuration is fine.
pub fn openrouter_ner_unavailable(reason: &str) -> String {
    if is_openrouter_config_error(reason) {
        format!("OpenRouter NER unavailable: {reason}. Configure OpenRouter API key/model.")
    } else {
        format!("NER falló llamando a OpenRouter: {reason}")
    }
}

/// Auth/config-class errors: missing API key, 401/403 (HTTP status or error
/// envelope code), OpenRouter's "User not found." invalid-key message, or an
/// invalid model. Matches the message shapes produced by
/// `crate::llm::openrouter` and the local missing-key guards.
fn is_openrouter_config_error(reason: &str) -> bool {
    let lowered = reason.to_lowercase();
    lowered.contains("api key")
        || lowered.contains("http 401")
        || lowered.contains("http 403")
        || lowered.contains("code 401")
        || lowered.contains("code 403")
        || lowered.contains("user not found")
        || lowered.contains("no auth credentials")
        || lowered.contains("invalid model")
        || lowered.contains("not a valid model")
}

fn parse_openrouter_entity_type(value: &str) -> Option<EntityType> {
    // Unicode-aware uppercase plus accent stripping: `to_ascii_uppercase` left
    // lowercase accented labels like "institución" untouched, so accented match
    // arms were unreachable. Labels never contain Ñ, so only vowels are mapped.
    let normalized: String = value
        .trim()
        .to_uppercase()
        .chars()
        .map(|ch| match ch {
            'Á' => 'A',
            'É' => 'E',
            'Í' => 'I',
            'Ó' => 'O',
            'Ú' | 'Ü' => 'U',
            other => other,
        })
        .collect();

    match normalized.as_str() {
        "PER" | "PERSON" | "PERSONA" | "PERSONAS" => Some(EntityType::Person),
        "LOC" | "LOCATION" | "PLACE" | "LUGAR" | "LUGARES" => Some(EntityType::Place),
        "ORG" | "ORGANIZATION" | "ORGANISATION" | "ORGANIZACION" | "ORGANIZACIONES"
        | "INSTITUTION" | "INSTITUCION" => Some(EntityType::Organization),
        "DATE" | "FECHA" | "FECHAS" => Some(EntityType::Date),
        "MISC" | "OTHER" | "OTRO" => Some(EntityType::Misc),
        _ => None,
    }
}

/// Best-effort recovery for array responses truncated by `max_tokens`.
///
/// `extract_json_payload` can only trim the payload back to the last `}` it
/// finds, which leaves an unterminated array (`EOF while parsing a list`).
/// When the payload looks like an array and serde failed with an EOF-class
/// error, cut at the end of the last complete top-level object — tracking
/// brace depth while respecting strings and escapes — and close the array.
fn salvage_truncated_array(json: &str, error: &serde_json::Error) -> Option<String> {
    if !json.trim_start().starts_with('[') || !error.is_eof() {
        return None;
    }

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut last_complete_end = None;

    for (index, ch) in json.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    last_complete_end = Some(index + ch.len_utf8());
                }
            }
            _ => {}
        }
    }

    let end = last_complete_end?;
    let mut salvaged = json[..end].to_string();
    salvaged.push(']');
    Some(salvaged)
}

fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();
    if !trimmed.starts_with("```") {
        return trimmed.to_string();
    }

    let without_opening = trimmed
        .strip_prefix("```")
        .unwrap_or(trimmed)
        .trim_start_matches("json")
        .trim_start_matches("JSON")
        .trim();

    without_opening
        .strip_suffix("```")
        .unwrap_or(without_opening)
        .trim()
        .to_string()
}

fn extract_json_payload(content: &str) -> Result<&str, String> {
    let start = content
        .find('[')
        .or_else(|| content.find('{'))
        .ok_or_else(|| "OpenRouter NER did not return JSON content.".to_string())?;
    let end = content
        .rfind(']')
        .or_else(|| content.rfind('}'))
        .ok_or_else(|| "OpenRouter NER did not return a closed JSON payload.".to_string())?;

    if end < start {
        return Err("OpenRouter NER returned malformed JSON boundaries.".to_string());
    }

    Ok(&content[start..=end])
}

#[cfg(test)]
mod tests {
    use super::openrouter_ner_unavailable;

    #[test]
    fn missing_api_key_gets_configure_hint() {
        let message = openrouter_ner_unavailable("OpenRouter API key no configurada para NER");
        assert!(message.starts_with("OpenRouter NER unavailable:"));
        assert!(message.ends_with("Configure OpenRouter API key/model."));
    }

    #[test]
    fn http_401_gets_configure_hint() {
        let message =
            openrouter_ner_unavailable("OpenRouter API error (HTTP 401): User not found.");
        assert!(message.contains("Configure OpenRouter API key/model."));
    }

    #[test]
    fn http_403_gets_configure_hint() {
        let message = openrouter_ner_unavailable("OpenRouter API error (HTTP 403): forbidden");
        assert!(message.contains("Configure OpenRouter API key/model."));
    }

    #[test]
    fn error_envelope_code_401_gets_configure_hint() {
        let message =
            openrouter_ner_unavailable("OpenRouter API error: User not found. (code 401). Body: …");
        assert!(message.contains("Configure OpenRouter API key/model."));
    }

    #[test]
    fn invalid_model_gets_configure_hint() {
        let message = openrouter_ner_unavailable(
            "OpenRouter API error (HTTP 400): foo/bar is not a valid model ID",
        );
        assert!(message.contains("Configure OpenRouter API key/model."));
    }

    #[test]
    fn transient_provider_error_keeps_neutral_prefix() {
        let message = openrouter_ner_unavailable(
            "OpenRouter API error: Provider returned error (code 502). Body: {\"error\":{}}",
        );
        assert!(message.starts_with("NER falló llamando a OpenRouter:"));
        assert!(!message.contains("Configure OpenRouter API key/model."));
    }

    #[test]
    fn timeout_keeps_neutral_prefix_and_detail() {
        let message = openrouter_ner_unavailable(
            "OpenRouter request failed: error decoding response body: operation timed out",
        );
        assert!(message.starts_with("NER falló llamando a OpenRouter:"));
        assert!(message.contains("operation timed out"));
        assert!(!message.contains("Configure OpenRouter API key/model."));
    }

    #[test]
    fn unexpected_body_keeps_neutral_prefix() {
        let message = openrouter_ner_unavailable(
            "OpenRouter returned an unexpected body (expected value at line 1 column 1): <html>",
        );
        assert!(message.starts_with("NER falló llamando a OpenRouter:"));
        assert!(!message.contains("Configure OpenRouter API key/model."));
    }
}
