use serde::{Deserialize, Serialize};
use std::time::Duration;

const GLM_OCR_API_URL: &str = "https://api.z.ai/api/paas/v4/layout_parsing";
const GLM_OCR_TEST_IMAGE_URL: &str = "https://cdn.bigmodel.cn/static/logo/introduction.png";

/// Lenient error envelope: Z.AI reports errors both as nested
/// `{"error":{"code":...,"message":...}}` and as flat `{"code":...,"msg":...}`,
/// sometimes WITH HTTP 200. `code` is a raw JSON value because the provider
/// mixes string and numeric codes.
#[derive(Deserialize)]
struct GlmOcrApiErrorEnvelope {
    #[serde(default)]
    error: Option<GlmOcrApiError>,
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct GlmOcrApiError {
    #[serde(default)]
    code: Option<serde_json::Value>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Serialize)]
struct LayoutParsingRequest<'a> {
    model: &'static str,
    file: &'a str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlmOcrResponse {
    #[allow(dead_code)]
    pub id: Option<String>,
    #[allow(dead_code)]
    pub created: Option<i64>,
    #[allow(dead_code)]
    pub model: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub md_results: String,
    #[serde(default, deserialize_with = "null_to_default")]
    pub layout_details: Vec<Vec<GlmOcrLayoutDetail>>,
    pub data_info: Option<GlmOcrDataInfo>,
    #[allow(dead_code)]
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlmOcrLayoutDetail {
    pub index: Option<i32>,
    pub label: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub bbox_2d: Vec<f32>,
    pub content: Option<String>,
    pub height: Option<u32>,
    pub width: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlmOcrDataInfo {
    #[allow(dead_code)]
    pub num_pages: Option<u32>,
    #[serde(default, deserialize_with = "null_to_default")]
    pub pages: Vec<GlmOcrPageInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GlmOcrPageInfo {
    pub width: u32,
    pub height: u32,
}

/// Accept `null` for fields the provider sometimes emits as JSON null instead
/// of omitting them, mapping it to the type's default.
fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// Outcome of a single layout-parsing attempt, classified so the retry loop
/// can distinguish transient failures (provider 5xx/429/408, timeouts) from
/// deterministic ones (auth, malformed bodies).
#[derive(Debug)]
struct GlmOcrCallError {
    message: String,
    retryable: bool,
}

pub struct GlmOcrClient {
    client: reqwest::Client,
    api_key: String,
}

impl GlmOcrClient {
    pub fn new(api_key: String) -> Self {
        // Requests upload multi-MB base64 bodies and the OCR worker runs a
        // small bounded number of jobs concurrently, so a stalled connection
        // without timeouts would pin a concurrency slot indefinitely. The
        // overall timeout is generous to allow slow uploads of large scans.
        let client = reqwest::Client::builder()
            .user_agent("EntropIA-Desktop/0.1 (historical-research-app)")
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to build reqwest client");

        Self { client, api_key }
    }

    pub async fn test_connection(&self) -> Result<(), String> {
        let response = self
            .client
            .post(GLM_OCR_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&LayoutParsingRequest {
                model: "glm-ocr",
                file: GLM_OCR_TEST_IMAGE_URL,
            })
            .send()
            .await
            .map_err(|e| format!("GLM-OCR connection test failed: {}", error_chain(&e)))?;

        Self::ensure_success(response).await.map(|_| ())
    }

    /// Parse a file, retrying transient failures (provider 5xx/429/408,
    /// timeouts, connection resets) up to 3 total attempts with a short
    /// backoff. Non-retryable errors (auth, malformed bodies) return
    /// immediately.
    pub async fn parse_file(&self, file: &str) -> Result<GlmOcrResponse, String> {
        const MAX_ATTEMPTS: u32 = 3;
        const BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.parse_file_once(file).await {
                Ok(response) => return Ok(response),
                Err(error) if error.retryable && attempt < MAX_ATTEMPTS => {
                    let delay = BACKOFF[(attempt - 1) as usize];
                    eprintln!(
                        "[ocr/glm_ocr] attempt {attempt}/{MAX_ATTEMPTS} failed (retryable): {}; retrying in {}s",
                        error.message,
                        delay.as_secs()
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error.message),
            }
        }
    }

    async fn parse_file_once(&self, file: &str) -> Result<GlmOcrResponse, GlmOcrCallError> {
        let response = self
            .client
            .post(GLM_OCR_API_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&LayoutParsingRequest {
                model: "glm-ocr",
                file,
            })
            .send()
            .await
            .map_err(|e| GlmOcrCallError {
                message: format!("GLM-OCR request failed: {}", error_chain(&e)),
                retryable: true,
            })?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(|e| GlmOcrCallError {
            message: format!("GLM-OCR response body read failed: {}", error_chain(&e)),
            retryable: true,
        })?;

        interpret_parse_file_body(status, &body)
    }

    async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, String> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let body = response.text().await.unwrap_or_default();
        Err(format!(
            "GLM-OCR API error ({status}): {}",
            non_success_error_detail(&body)
        ))
    }
}

// ---------------------------------------------------------------------------
// Response interpretation (pure, unit-testable)
// ---------------------------------------------------------------------------

/// Walk the `source()` chain so reqwest errors keep their detail. Without
/// this, a timeout surfaces as the opaque "error decoding response body"
/// instead of "error decoding response body: operation timed out".
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut source = e.source();
    while let Some(inner) = source {
        parts.push(inner.to_string());
        source = inner.source();
    }
    parts.join(": ")
}

/// First ~300 chars of the body, trimmed, for inclusion in error messages.
fn body_snippet(body: &str) -> String {
    const MAX_CHARS: usize = 300;
    let trimmed = body.trim();
    match trimmed.char_indices().nth(MAX_CHARS) {
        Some((index, _)) => format!("{}…", &trimmed[..index]),
        None => trimmed.to_string(),
    }
}

fn status_is_retryable(status: u16) -> bool {
    matches!(status, 408 | 429 | 500..=599)
}

/// An envelope error is retryable when its code maps to a transient HTTP
/// class (408/429/5xx) or the message hints at a transient provider
/// condition (timeout, overloaded).
fn code_is_retryable(code: &Option<serde_json::Value>) -> bool {
    match code {
        Some(serde_json::Value::Number(number)) => number
            .as_u64()
            .is_some_and(|code| u16::try_from(code).is_ok_and(status_is_retryable)),
        Some(serde_json::Value::String(text)) => {
            text.trim().parse::<u16>().is_ok_and(status_is_retryable)
        }
        _ => false,
    }
}

fn message_hints_transient(message: Option<&str>) -> bool {
    message.is_some_and(|message| {
        let lowered = message.to_lowercase();
        lowered.contains("timeout")
            || lowered.contains("timed out")
            || lowered.contains("overloaded")
    })
}

/// Provider error detail (and retryability) when the body carries an
/// unambiguous error signal: a nested `error` object, or the flat Z.AI shape
/// with BOTH `code` and `msg`/`message`. Requiring both flat fields keeps a
/// success body with stray fields from being misread as an error.
fn envelope_error(body: &str) -> Option<(String, bool)> {
    let envelope = serde_json::from_str::<GlmOcrApiErrorEnvelope>(body).ok()?;

    if let Some(error) = &envelope.error {
        let detail = match (&error.code, &error.message) {
            (Some(code), Some(message)) => format!("{message} (code {code})"),
            (None, Some(message)) => message.clone(),
            (Some(code), None) => format!("code {code}"),
            (None, None) => "unspecified error".to_string(),
        };
        let retryable =
            code_is_retryable(&error.code) || message_hints_transient(error.message.as_deref());
        return Some((detail, retryable));
    }

    let message = envelope.msg.as_ref().or(envelope.message.as_ref())?;
    let code = envelope.code.as_ref()?;
    let retryable = code_is_retryable(&envelope.code) || message_hints_transient(Some(message));
    Some((format!("{message} (code {code})"), retryable))
}

/// Liberal detail extraction for non-2xx bodies (the status already says it
/// failed): nested error → flat msg/message → snippet.
fn non_success_error_detail(body: &str) -> String {
    if let Some((detail, _)) = envelope_error(body) {
        return detail;
    }
    if let Ok(envelope) = serde_json::from_str::<GlmOcrApiErrorEnvelope>(body) {
        if let Some(message) = envelope.msg.or(envelope.message) {
            return message;
        }
    }
    body_snippet(body)
}

/// Decision ladder for a layout-parsing HTTP exchange. Pure so the
/// classification (message + retryability) is unit-testable without HTTP
/// mocking. A 200 body that parses but carries no content is returned as-is:
/// the caller decides what "empty OCR output" means.
fn interpret_parse_file_body(status: u16, body: &str) -> Result<GlmOcrResponse, GlmOcrCallError> {
    // 1. Non-2xx: the status alone decides retryability.
    if !(200..300).contains(&status) {
        return Err(GlmOcrCallError {
            message: format!(
                "GLM-OCR API error (HTTP {status}): {}",
                non_success_error_detail(body)
            ),
            retryable: status_is_retryable(status),
        });
    }

    // 2. Error envelope with HTTP 200: Z.AI reports business errors this way.
    if let Some((detail, retryable)) = envelope_error(body) {
        return Err(GlmOcrCallError {
            message: format!("GLM-OCR API error: {detail}. Body: {}", body_snippet(body)),
            retryable,
        });
    }

    // 3. Unparseable body: include the serde detail and a snippet so the
    // failure is never opaque. The request itself succeeded — not retryable.
    serde_json::from_str::<GlmOcrResponse>(body).map_err(|error| GlmOcrCallError {
        message: format!(
            "GLM-OCR returned an unexpected body ({error}): {}",
            body_snippet(body)
        ),
        retryable: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interpret(status: u16, body: &str) -> Result<GlmOcrResponse, GlmOcrCallError> {
        interpret_parse_file_body(status, body)
    }

    #[test]
    fn happy_path_parses_full_response() {
        let body = r##"{
            "id": "task-1",
            "created": 1718000000,
            "model": "glm-ocr",
            "md_results": "# Acta\n\nTexto del documento",
            "layout_details": [[{
                "index": 0,
                "label": "text",
                "bbox_2d": [0.1, 0.2, 0.8, 0.9],
                "content": "Texto del documento",
                "height": 1200,
                "width": 800
            }]],
            "data_info": {"num_pages": 1, "pages": [{"width": 800, "height": 1200}]},
            "request_id": "req-1"
        }"##;
        let parsed = interpret(200, body).unwrap();
        assert_eq!(parsed.md_results, "# Acta\n\nTexto del documento");
        assert_eq!(parsed.layout_details.len(), 1);
        assert_eq!(
            parsed.layout_details[0][0].content.as_deref(),
            Some("Texto del documento")
        );
    }

    #[test]
    fn missing_optional_fields_parse_leniently() {
        let parsed = interpret(200, r#"{"request_id":"req-1"}"#).unwrap();
        assert!(parsed.md_results.is_empty());
        assert!(parsed.layout_details.is_empty());
    }

    #[test]
    fn null_content_fields_parse_leniently() {
        let parsed = interpret(200, r#"{"md_results":null,"layout_details":null}"#).unwrap();
        assert!(parsed.md_results.is_empty());
        assert!(parsed.layout_details.is_empty());
    }

    #[test]
    fn nested_error_envelope_with_http_200_is_non_retryable() {
        let body = r#"{"error":{"code":"1113","message":"Invalid API key"}}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("Invalid API key"));
        assert!(error.message.contains("1113"));
    }

    #[test]
    fn flat_error_envelope_with_http_200_is_reported() {
        let body = r#"{"code":1302,"msg":"Concurrency limit reached"}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("Concurrency limit reached"));
        assert!(error.message.contains("1302"));
    }

    #[test]
    fn error_envelope_with_timeout_message_is_retryable() {
        let body = r#"{"error":{"message":"Upstream provider timed out"}}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(error.retryable);
    }

    #[test]
    fn error_envelope_with_5xx_code_is_retryable() {
        let body = r#"{"error":{"code":"503","message":"Service unavailable"}}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(error.retryable);
    }

    #[test]
    fn success_body_with_stray_message_field_is_not_an_error() {
        let body = r#"{"md_results":"hola","message":"ok"}"#;
        let parsed = interpret(200, body).unwrap();
        assert_eq!(parsed.md_results, "hola");
    }

    #[test]
    fn unparseable_html_body_includes_snippet_and_serde_detail() {
        let body = "<html><body><h1>502 Bad Gateway</h1></body></html>";
        let error = interpret(200, body).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("unexpected body"));
        assert!(error.message.contains("502 Bad Gateway"));
        // The serde detail must survive (kills the opaque case forever).
        assert!(error.message.contains("expected value"));
    }

    #[test]
    fn non_2xx_429_is_retryable_and_includes_envelope_detail() {
        let error = interpret(429, r#"{"error":{"message":"rate limited"}}"#).unwrap_err();
        assert!(error.retryable);
        assert!(error.message.contains("HTTP 429"));
        assert!(error.message.contains("rate limited"));
    }

    #[test]
    fn non_2xx_401_with_flat_msg_is_non_retryable() {
        let error = interpret(401, r#"{"msg":"auth failed"}"#).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("HTTP 401"));
        assert!(error.message.contains("auth failed"));
    }

    #[test]
    fn long_error_body_is_truncated_to_a_snippet() {
        let body = "x".repeat(2000);
        let error = interpret(500, &body).unwrap_err();
        assert!(error.retryable);
        assert!(error.message.len() < 500);
        assert!(error.message.contains('…'));
    }
}
