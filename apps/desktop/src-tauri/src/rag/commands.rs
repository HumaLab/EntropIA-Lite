//! Comandos Tauri del chat RAG: `rag_ask` + gestión de conversaciones
//! persistidas (`rag_list_conversations`, `rag_get_conversation`,
//! `rag_delete_conversation`).
//!
//! Pipeline de `rag_ask`: validación → settings + historial → recuperación
//! híbrida (en `spawn_blocking` con la conexión worker) → prompt de
//! fragmentos numerados → LLM remoto → persistencia del intercambio.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use super::params::{rag_params_from_settings, RagParams, TOP_K_MAX, TOP_K_MIN};
use super::{retrieval, store};
use super::{RagAnswer, RagChatTurn, RagConversation, RagConversationSummary, RagSource};
use crate::llm::openrouter::{GenerationParams, OpenRouterClient};

const QUESTION_MAX_CHARS: usize = 4000;

/// Resultado de la fase bloqueante (settings + historial + recuperación).
struct RetrievalPhase {
    api_key: String,
    model: String,
    sources: Vec<RagSource>,
    history: Vec<RagChatTurn>,
    params: RagParams,
}

/// Responde una pregunta con RAG híbrido (vector + FTS5 fusionados con RRF)
/// sobre la base de transcripciones, citando las fuentes con `[n]`. El
/// historial se deriva de la conversación persistida (`conversation_id`) y
/// cada intercambio exitoso se guarda en SQLite; la respuesta devuelve el id
/// real de la conversación (fresco si no existía o fue borrada en vuelo).
/// Si la persistencia falla DESPUÉS de una respuesta exitosa del LLM, la
/// respuesta se devuelve igual con `conversation_id: None` — los errores de
/// validación y del LLM sí se propagan como `Err`.
#[tauri::command]
pub async fn rag_ask(
    question: String,
    conversation_id: Option<String>,
    top_k: Option<u8>,
    db: tauri::State<'_, crate::db::state::AppDbState>,
) -> Result<RagAnswer, String> {
    let question = validate_question(&question)?;
    let requested_top_k = top_k;

    // Fase de recuperación: settings + embedding + SQL corren en el pool
    // bloqueante con la conexión worker (nunca en el hilo del event loop).
    let conn_arc = db.worker_conn.clone();
    let retrieval_question = question.clone();
    let history_conversation_id = conversation_id.clone();
    let phase = tokio::task::spawn_blocking(move || -> Result<RetrievalPhase, String> {
        // Paso 1: lecturas de settings + historial persistido con el lock,
        // soltándolo antes de cualquier I/O de red (el embedding remoto
        // puede tardar hasta 120s).
        let (api_key, model, embedding_config, history, params) = {
            let conn = conn_arc.lock().map_err(|e| e.to_string())?;

            // `get_secret_setting` devuelve None si la referencia
            // `secret_ref:` no se pudo resolver (credencial ausente en el
            // keyring), evitando mandar el placeholder como Bearer token.
            let api_key = validate_chat_api_key(crate::settings::get_secret_setting(
                &conn,
                "openrouter_api_key",
            ))?;
            let model = crate::settings::get_setting(&conn, "openrouter_model")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| crate::settings::DEFAULT_OPENROUTER_MODEL.to_string());
            let embedding_config = crate::nlp::embeddings::config_from_settings(&conn);

            // Parámetros RAG runtime (rag_top_k, rag_min_similarity, etc.);
            // el argumento `top_k` del comando pisa al setting si vino.
            let mut params = rag_params_from_settings(&conn);
            params.top_k = resolve_top_k(requested_top_k, params.top_k);

            // Historial desde la conversación persistida (vacío si el id no
            // existe o no vino); presupuesto de turnos/chars configurable.
            let history = match history_conversation_id.as_deref() {
                Some(id) => store::load_history(&conn, id, params.history_turns)?,
                None => Vec::new(),
            };

            (api_key, model, embedding_config, history, params)
        };

        // Paso 2 (sin lock): pierna vectorial con degradación elegante; si la
        // config o el embedding fallan (clave ausente, error de API), seguimos
        // solo con FTS.
        let query_embedding = match embedding_config.and_then(|config| {
            crate::nlp::embeddings::embed_query_text_with_config(config, &retrieval_question)
        }) {
            Ok(embedding) => Some(embedding),
            Err(error) => {
                eprintln!("[rag] Pierna vectorial deshabilitada (se usa solo FTS): {error}");
                None
            }
        };

        // Paso 3: re-adquirir el lock solo para la recuperación SQL.
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;
        let sources = retrieval::hybrid_retrieve(
            &conn,
            &retrieval_question,
            query_embedding.as_deref(),
            &params,
        )?;

        Ok(RetrievalPhase {
            api_key,
            model,
            sources,
            history,
            params,
        })
    })
    .await
    .map_err(|e| format!("RAG retrieval task panicked: {e}"))??;

    // Sin contenido relevante: no llamamos al LLM; el frontend muestra su
    // propio mensaje de "sin resultados". El intercambio vacío también se
    // persiste para que la conversación quede completa.
    if phase.sources.is_empty() {
        let conversation_id = persist_exchange_or_warn(
            db.worker_conn.clone(),
            conversation_id,
            question,
            String::new(),
            Vec::new(),
            phase.model.clone(),
        )
        .await;
        return Ok(empty_answer(phase.model, conversation_id));
    }

    let prompt = build_rag_prompt(&question, &phase.sources, &phase.history, &phase.params);

    let client = OpenRouterClient::try_new(phase.api_key, phase.model.clone())?;
    let answer = client
        .generate_with_params(
            &prompt,
            &GenerationParams::with_defaults(phase.params.max_tokens, phase.params.temperature),
        )
        .await?;

    // Paso 4: persistencia del intercambio en un cuarto scope de lock corto,
    // SIEMPRE después del await del LLM (nunca cruzamos la red con el lock).
    // Si el LLM falló, el `?` de arriba ya propagó el error sin persistir.
    // Si la PERSISTENCIA falla, la respuesta ya pagada al LLM no se descarta:
    // se devuelve con `conversation_id: None`.
    let conversation_id = persist_exchange_or_warn(
        db.worker_conn.clone(),
        conversation_id,
        question,
        answer.clone(),
        phase.sources.clone(),
        phase.model.clone(),
    )
    .await;

    Ok(RagAnswer {
        answer,
        sources: phase.sources,
        model: phase.model,
        conversation_id,
    })
}

/// Igual que `persist_exchange_blocking`, pero NUNCA propaga el error: una
/// respuesta ya obtenida del LLM no se descarta porque falló la persistencia.
/// Loguea el error y devuelve `None` (el frontend no adopta ningún id).
async fn persist_exchange_or_warn(
    conn_arc: Arc<Mutex<Connection>>,
    conversation_id: Option<String>,
    question: String,
    answer: String,
    sources: Vec<RagSource>,
    model: String,
) -> Option<String> {
    match persist_exchange_blocking(conn_arc, conversation_id, question, answer, sources, model)
        .await
    {
        Ok(id) => Some(id),
        Err(error) => {
            eprintln!(
                "[rag] No se pudo persistir el intercambio (la respuesta se devuelve igual): {error}"
            );
            None
        }
    }
}

/// Persiste el intercambio pregunta/respuesta en el pool bloqueante con un
/// lock corto sobre la conexión worker. Devuelve el id real de la
/// conversación (fresco si no existía).
async fn persist_exchange_blocking(
    conn_arc: Arc<Mutex<Connection>>,
    conversation_id: Option<String>,
    question: String,
    answer: String,
    sources: Vec<RagSource>,
    model: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut conn = conn_arc.lock().map_err(|e| e.to_string())?;
        store::persist_exchange(
            &mut conn,
            conversation_id.as_deref(),
            &question,
            &answer,
            &sources,
            &model,
            store::now_millis(),
        )
    })
    .await
    .map_err(|e| format!("RAG persistence task panicked: {e}"))?
}

/// Lista las conversaciones RAG persistidas, más reciente primero.
#[tauri::command]
pub async fn rag_list_conversations(
    db: tauri::State<'_, crate::db::state::AppDbState>,
) -> Result<Vec<RagConversationSummary>, String> {
    let conn_arc = db.worker_conn.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<RagConversationSummary>, String> {
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;
        store::list_conversations(&conn)
    })
    .await
    .map_err(|e| format!("RAG list task panicked: {e}"))?
}

/// Carga una conversación persistida completa, con mensajes y fuentes.
#[tauri::command]
pub async fn rag_get_conversation(
    conversation_id: String,
    db: tauri::State<'_, crate::db::state::AppDbState>,
) -> Result<RagConversation, String> {
    let conn_arc = db.worker_conn.clone();
    tokio::task::spawn_blocking(move || -> Result<RagConversation, String> {
        let conn = conn_arc.lock().map_err(|e| e.to_string())?;
        store::get_conversation(&conn, &conversation_id)
    })
    .await
    .map_err(|e| format!("RAG get task panicked: {e}"))?
}

/// Elimina una conversación persistida y sus mensajes. Borrar un id
/// inexistente es un no-op exitoso.
#[tauri::command]
pub async fn rag_delete_conversation(
    conversation_id: String,
    db: tauri::State<'_, crate::db::state::AppDbState>,
) -> Result<(), String> {
    let conn_arc = db.worker_conn.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut conn = conn_arc.lock().map_err(|e| e.to_string())?;
        store::delete_conversation(&mut conn, &conversation_id)
    })
    .await
    .map_err(|e| format!("RAG delete task panicked: {e}"))?
}

/// Valida la pregunta del usuario: trim, no vacía y máximo 4000 caracteres
/// (conteo por chars, no bytes).
fn validate_question(question: &str) -> Result<String, String> {
    let question = question.trim().to_string();
    if question.is_empty() {
        return Err(
            "La pregunta no puede estar vacía. Escribí una consulta para buscar en tus documentos."
                .to_string(),
        );
    }
    if question.chars().count() > QUESTION_MAX_CHARS {
        return Err(format!(
            "La pregunta es demasiado larga (máximo {QUESTION_MAX_CHARS} caracteres)."
        ));
    }
    Ok(question)
}

/// Valida la API key cruda devuelta por `settings::get_secret_setting`:
/// `None` cubre tanto la ausencia del setting como una referencia
/// `secret_ref:` sin resolver (credencial faltante en el keyring).
fn validate_chat_api_key(raw: Option<String>) -> Result<String, String> {
    raw.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "OpenRouter API key no configurada. Andá a Configuración para agregarla.".to_string()
        })
}

/// top_k final: el argumento del comando (clamp 1..=20) pisa el setting
/// `rag_top_k`; sin argumento queda el valor del setting (ya validado por
/// `rag_params_from_settings`).
fn resolve_top_k(requested: Option<u8>, settings_top_k: usize) -> usize {
    match requested {
        Some(value) => usize::from(value).clamp(TOP_K_MIN, TOP_K_MAX),
        None => settings_top_k,
    }
}

/// Respuesta vacía cuando la recuperación no encontró fuentes (sin LLM).
/// `conversation_id` es `None` si la persistencia del intercambio falló.
fn empty_answer(model: String, conversation_id: Option<String>) -> RagAnswer {
    RagAnswer {
        answer: String::new(),
        sources: Vec::new(),
        model,
        conversation_id,
    }
}

/// Prompt completo: instrucciones + fragmentos numerados + historial + pregunta.
fn build_rag_prompt(
    question: &str,
    sources: &[RagSource],
    history: &[RagChatTurn],
    params: &RagParams,
) -> String {
    let context = format_fragments(sources);
    let history_block =
        format_history(history, params.history_turns, params.history_turn_max_chars);
    crate::llm::prompt::raw_rag_answer(question, &context, &history_block)
}

/// Fragmentos con el formato `[n] «item_title» (collection_name):\n{snippet}`.
fn format_fragments(sources: &[RagSource]) -> String {
    sources
        .iter()
        .map(|source| {
            format!(
                "[{}] «{}» ({}):\n{}",
                source.index, source.item_title, source.collection_name, source.snippet
            )
        })
        .collect::<Vec<String>>()
        .join("\n\n")
}

/// Últimos `max_turns` turnos, cada uno truncado a `turn_max_chars` (por
/// chars, no bytes), con prefijo Usuario:/Asistente:.
fn format_history(history: &[RagChatTurn], max_turns: usize, turn_max_chars: usize) -> String {
    history
        .iter()
        .skip(history.len().saturating_sub(max_turns))
        .filter(|turn| !turn.content.trim().is_empty())
        .map(|turn| {
            let prefix = if turn.role == "assistant" {
                "Asistente"
            } else {
                "Usuario"
            };
            let content: String = turn.content.trim().chars().take(turn_max_chars).collect();
            format!("{prefix}: {content}")
        })
        .collect::<Vec<String>>()
        .join("\n")
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(role: &str, content: &str) -> RagChatTurn {
        RagChatTurn {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    fn source(index: u32, title: &str, collection: &str, snippet: &str) -> RagSource {
        RagSource {
            index,
            asset_id: format!("asset-{index}"),
            item_id: format!("item-{index}"),
            item_title: title.to_string(),
            collection_id: "col-1".to_string(),
            collection_name: collection.to_string(),
            snippet: snippet.to_string(),
            score: 1.0 / f64::from(index),
            start_seconds: None,
            end_seconds: None,
        }
    }

    #[test]
    fn resolve_top_k_defaults_and_clamps() {
        // Sin argumento: pasa el valor del setting tal cual.
        assert_eq!(resolve_top_k(None, 6), 6);
        assert_eq!(resolve_top_k(None, 13), 13);
        // Con argumento: pisa el setting, clamp 1..=20.
        assert_eq!(resolve_top_k(Some(0), 6), 1);
        assert_eq!(resolve_top_k(Some(3), 6), 3);
        assert_eq!(resolve_top_k(Some(15), 6), 15);
        assert_eq!(resolve_top_k(Some(20), 6), 20);
        assert_eq!(resolve_top_k(Some(200), 6), 20);
    }

    #[test]
    fn validate_question_rejects_empty_and_whitespace() {
        assert!(validate_question("").is_err());
        assert!(validate_question("   \n\t ").is_err());
    }

    #[test]
    fn validate_question_trims_and_accepts_normal_input() {
        assert_eq!(
            validate_question("  ¿Qué pasó en mayo?  ").as_deref(),
            Ok("¿Qué pasó en mayo?")
        );
    }

    #[test]
    fn validate_question_caps_at_4000_chars_not_bytes() {
        // Multibyte char: 4000 chars son 8000 bytes — el límite es por chars.
        let exactly_max = "á".repeat(4000);
        assert!(validate_question(&exactly_max).is_ok());

        let over_max = "á".repeat(4001);
        let error = validate_question(&over_max).expect_err("4001 chars must be rejected");
        assert_eq!(
            error,
            "La pregunta es demasiado larga (máximo 4000 caracteres)."
        );
    }

    #[test]
    fn validate_chat_api_key_rejects_unresolved_secret_ref_as_missing() {
        // `get_secret_setting` devuelve None cuando el valor es un placeholder
        // `secret_ref:` sin credencial en el keyring; la validación debe dar
        // el error amigable en vez de mandar el placeholder como Bearer token.
        let error = validate_chat_api_key(None).expect_err("missing key must be rejected");
        assert_eq!(
            error,
            "OpenRouter API key no configurada. Andá a Configuración para agregarla."
        );
        assert!(validate_chat_api_key(Some("   ".to_string())).is_err());
    }

    #[test]
    fn validate_chat_api_key_trims_and_accepts_resolved_key() {
        assert_eq!(
            validate_chat_api_key(Some("  sk-or-123  ".to_string())).as_deref(),
            Ok("sk-or-123")
        );
    }

    #[test]
    fn format_history_keeps_last_six_turns_and_truncates_content() {
        let mut history = Vec::new();
        for i in 0..8 {
            history.push(turn(
                if i % 2 == 0 { "user" } else { "assistant" },
                &format!("turno {i}"),
            ));
        }
        history.push(turn("user", &"x".repeat(600)));

        let formatted = format_history(&history, 6, 500);
        let lines: Vec<&str> = formatted.lines().collect();
        assert_eq!(lines.len(), 6, "only the last 6 turns survive");
        assert!(!formatted.contains("turno 0"));
        assert!(!formatted.contains("turno 2"));
        assert!(formatted.contains("Usuario: turno 4"));
        assert!(formatted.contains("Asistente: turno 7"));

        let last = lines.last().expect("history should have lines");
        assert!(last.starts_with("Usuario: "));
        assert_eq!(
            last.chars().count(),
            "Usuario: ".chars().count() + 500,
            "content is truncated to 500 chars"
        );
    }

    #[test]
    fn format_history_empty_returns_empty_string() {
        assert!(format_history(&[], 6, 500).is_empty());
    }

    #[test]
    fn format_history_respects_configured_turns_and_chars() {
        let history = vec![
            turn("user", "primer turno"),
            turn("assistant", "segundo turno"),
            turn("user", &"y".repeat(200)),
        ];
        let formatted = format_history(&history, 2, 100);
        let lines: Vec<&str> = formatted.lines().collect();
        assert_eq!(lines.len(), 2, "only the last 2 turns survive");
        assert!(!formatted.contains("primer turno"));
        let last = lines.last().expect("history should have lines");
        assert_eq!(last.chars().count(), "Usuario: ".chars().count() + 100);
    }

    #[test]
    fn build_rag_prompt_contains_numbered_fragments_history_and_question() {
        let sources = vec![
            source(1, "Acta del Cabildo", "Archivo General", "fragmento uno"),
            source(2, "Crónica", "Hemeroteca", "fragmento dos"),
        ];
        let history = vec![turn("user", "hola"), turn("assistant", "buenas")];
        let prompt = build_rag_prompt(
            "¿Qué pasó en mayo?",
            &sources,
            &history,
            &RagParams::default(),
        );

        assert!(prompt.contains("[1] «Acta del Cabildo» (Archivo General):\nfragmento uno"));
        assert!(prompt.contains("[2] «Crónica» (Hemeroteca):\nfragmento dos"));
        assert!(prompt.contains("Usuario: hola"));
        assert!(prompt.contains("Asistente: buenas"));
        assert!(prompt.contains("Pregunta: ¿Qué pasó en mayo?"));
        assert!(prompt.contains("[n]"), "citation instructions present");
    }

    #[test]
    fn build_rag_prompt_without_history_omits_history_block() {
        let sources = vec![source(1, "Acta", "Archivo", "fragmento")];
        let prompt = build_rag_prompt("pregunta", &sources, &[], &RagParams::default());
        assert!(!prompt.contains("Conversación previa"));
        assert!(prompt.contains("Pregunta: pregunta"));
    }

    #[test]
    fn empty_answer_skips_llm_and_returns_empty_payload() {
        let answer = empty_answer("modelo-x".to_string(), Some("conv-1".to_string()));
        assert!(answer.answer.is_empty());
        assert!(answer.sources.is_empty());
        assert_eq!(answer.model, "modelo-x");
        assert_eq!(answer.conversation_id.as_deref(), Some("conv-1"));
    }

    #[test]
    fn empty_answer_carries_none_when_persistence_failed() {
        let answer = empty_answer("modelo-x".to_string(), None);
        assert!(answer.answer.is_empty());
        assert_eq!(answer.conversation_id, None);
    }

    /// Conexión SIN las tablas RAG: fuerza el fallo de persistencia.
    fn conn_without_rag_tables() -> Arc<Mutex<Connection>> {
        Arc::new(Mutex::new(
            Connection::open_in_memory().expect("in-memory DB failed"),
        ))
    }

    #[tokio::test]
    async fn persist_failure_after_llm_answer_returns_none_instead_of_error() {
        // La respuesta del LLM ya está pagada: si la persistencia falla
        // (acá, tablas ausentes), el intercambio se pierde pero la respuesta
        // se devuelve igual con `None` — nunca un `Err`.
        let conversation_id = persist_exchange_or_warn(
            conn_without_rag_tables(),
            None,
            "pregunta".to_string(),
            "respuesta".to_string(),
            Vec::new(),
            "modelo-x".to_string(),
        )
        .await;
        assert_eq!(conversation_id, None);
    }

    #[tokio::test]
    async fn persist_success_returns_the_real_conversation_id() {
        let conn = Connection::open_in_memory().expect("in-memory DB failed");
        conn.execute_batch(
            "CREATE TABLE rag_conversations (
               id TEXT PRIMARY KEY,
               title TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE TABLE rag_messages (
               id TEXT PRIMARY KEY,
               conversation_id TEXT NOT NULL REFERENCES rag_conversations(id) ON DELETE CASCADE,
               sort_index INTEGER NOT NULL,
               role TEXT NOT NULL CHECK(role IN ('user','assistant')),
               content TEXT NOT NULL,
               sources TEXT,
               model TEXT,
               created_at INTEGER NOT NULL
             );",
        )
        .expect("RAG chat schema creation failed");

        let conversation_id = persist_exchange_or_warn(
            Arc::new(Mutex::new(conn)),
            None,
            "pregunta".to_string(),
            "respuesta".to_string(),
            Vec::new(),
            "modelo-x".to_string(),
        )
        .await;
        assert!(
            conversation_id.is_some(),
            "successful persistence keeps returning Some(id)"
        );
    }
}
