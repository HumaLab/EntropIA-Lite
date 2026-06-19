/// Remote-only embedding support for EntropIA Lite.
///
/// Lite keeps the same Tauri commands and DB contracts. Embeddings are generated through
/// OpenRouter's embeddings API when configured; otherwise jobs degrade with a
/// clear error.
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use super::chunking::{chunk_text, MAX_CHARS as MAX_EMBEDDING_CHARS};
use super::text_provider;

pub const EMBEDDING_PROVIDER_SETTING_KEY: &str = "embedding_provider";
pub const OPENROUTER_EMBEDDING_MODEL_SETTING_KEY: &str = "openrouter_embedding_model";
pub const LOCAL_EMBEDDING_MODEL_DIR_SETTING_KEY: &str = "local_embedding_model_dir";
pub const DEFAULT_OPENROUTER_EMBEDDING_MODEL: &str = "baai/bge-m3";
pub const OPENROUTER_EMBEDDING_DIMENSIONS: usize = 1024;
const OPENROUTER_EMBEDDINGS_URL: &str = "https://openrouter.ai/api/v1/embeddings";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEmbeddingCandidate {
    pub asset_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetEmbeddingCoverageSummary {
    pub total_assets: i64,
    pub assets_with_text: i64,
    pub assets_with_embedding: i64,
    pub assets_missing_embedding: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalEmbeddingModelFileInfo {
    pub filename: String,
    pub source_path: String,
    pub destination: String,
    pub size_bytes: Option<u64>,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LocalEmbeddingModelInfo {
    pub exists: bool,
    pub available: bool,
    pub can_auto_download: bool,
    pub directory: String,
    pub path: String,
    pub size_bytes: Option<u64>,
    pub required_files: Vec<LocalEmbeddingModelFileInfo>,
    pub missing_files: Vec<LocalEmbeddingModelFileInfo>,
    pub source_repo: String,
}

#[derive(Clone, serde::Serialize)]
pub struct EmbeddingDownloadProgressPayload {
    pub pct: u8,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub file: String,
}

#[derive(Clone, serde::Serialize)]
pub struct EmbeddingDownloadCompletePayload {
    pub path: String,
}

#[derive(Clone, serde::Serialize)]
pub struct EmbeddingDownloadErrorPayload {
    pub error: String,
}

#[derive(Clone)]
pub struct EmbeddingConfig {
    pub api_key: String,
    pub model_name: String,
}

pub struct EmbeddingEngine {
    client: OpenRouterEmbeddingClient,
    cache: Mutex<HashMap<u64, Vec<f32>>>,
}

struct OpenRouterEmbeddingClient {
    api_key: String,
    model_name: String,
    endpoint_url: String,
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl EmbeddingEngine {
    pub fn init(config: EmbeddingConfig) -> Result<Self, String> {
        Self::init_openrouter_with_endpoint(config, OPENROUTER_EMBEDDINGS_URL.to_string())
    }

    fn init_openrouter_with_endpoint(
        config: EmbeddingConfig,
        endpoint_url: String,
    ) -> Result<Self, String> {
        if config.api_key.trim().is_empty() {
            return Err("OpenRouter API key no configurada para embeddings".to_string());
        }
        if config.model_name.trim().is_empty() {
            return Err("OpenRouter embedding model no configurado".to_string());
        }

        eprintln!(
            "[nlp/embeddings] Remote OpenRouter embedding engine configured: model={}, dimensions={}",
            config.model_name, OPENROUTER_EMBEDDING_DIMENSIONS,
        );

        Ok(Self {
            client: OpenRouterEmbeddingClient {
                api_key: config.api_key,
                model_name: config.model_name,
                endpoint_url,
            },
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>, String> {
        let key = rolling_hash64(text.as_bytes());
        if let Ok(cache) = self.cache.lock() {
            if let Some(hit) = cache.get(&key) {
                return Ok(hit.clone());
            }
        }

        let vector = self.client.embed_text(text)?;
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= 128 {
                if let Some(first_key) = cache.keys().next().copied() {
                    cache.remove(&first_key);
                }
            }
            cache.insert(key, vector.clone());
        }

        Ok(vector)
    }

    pub fn provider_name(&self) -> &'static str {
        "api"
    }
}

pub(crate) fn config_cache_key(config: &EmbeddingConfig) -> String {
    format!(
        "api|{}|{}",
        config.model_name,
        rolling_hash64(config.api_key.as_bytes())
    )
}

impl OpenRouterEmbeddingClient {
    fn embed_text(&self, text: &str) -> Result<Vec<f32>, String> {
        if text.trim().is_empty() {
            return Err("OpenRouter embedding input is empty".to_string());
        }

        let chunks = chunk_text(text);
        if chunks.len() > 1 {
            eprintln!(
                "[nlp/embeddings] text exceeded {MAX_EMBEDDING_CHARS} chars, splitting into {} chunks",
                chunks.len()
            );
        }

        let mut accumulator: Option<Vec<f32>> = None;
        for chunk in &chunks {
            let vector = self.embed_single_chunk(&chunk.text)?;
            accumulate_chunk_vector(&mut accumulator, vector, &self.model_name)?;
        }

        let mut averaged =
            accumulator.ok_or_else(|| "OpenRouter embedding produced no vectors".to_string())?;
        let n = chunks.len() as f32;
        for value in averaged.iter_mut() {
            *value /= n;
        }

        if averaged.len() != OPENROUTER_EMBEDDING_DIMENSIONS {
            return Err(format!(
                "OpenRouter embedding model '{}' returned {} dimensions; expected {} for {}",
                self.model_name,
                averaged.len(),
                OPENROUTER_EMBEDDING_DIMENSIONS,
                DEFAULT_OPENROUTER_EMBEDDING_MODEL,
            ));
        }

        Ok(averaged)
    }

    fn embed_single_chunk(&self, chunk: &str) -> Result<Vec<f32>, String> {
        let request = EmbeddingRequest {
            model: self.model_name.as_str(),
            input: chunk,
        };

        let client = reqwest::blocking::Client::builder()
            .user_agent("EntropIA-Desktop/0.1 (historical-research-app)")
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| format!("Failed to build OpenRouter embedding client: {e}"))?;

        let response = client
            .post(&self.endpoint_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://hlab.com.ar/")
            .header("X-Title", "EntropIA")
            .json(&request)
            .send()
            .map_err(|e| format!("OpenRouter embedding request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(format!("OpenRouter embedding API error ({status}): {body}"));
        }

        let parsed: EmbeddingResponse = response
            .json()
            .map_err(|e| format!("Failed to parse OpenRouter embedding response: {e}"))?;

        parsed
            .data
            .into_iter()
            .next()
            .map(|entry| entry.embedding)
            .ok_or_else(|| "OpenRouter embedding response returned no vectors".to_string())
    }
}

/// Suma `vector` sobre el acumulador de chunks. Cada respuesta de la API debe
/// compartir la dimensión del acumulador; un mismatch devuelve `Err` en lugar
/// de indexar fuera de rango.
fn accumulate_chunk_vector(
    accumulator: &mut Option<Vec<f32>>,
    vector: Vec<f32>,
    model_name: &str,
) -> Result<(), String> {
    match accumulator.as_mut() {
        Some(acc) => {
            if vector.len() != acc.len() {
                return Err(format!(
                    "El modelo de embeddings '{model_name}' devolvió vectores con dimensiones inconsistentes entre fragmentos ({} y {}). Reintentá la operación; si persiste, verificá el modelo de embeddings en Configuración.",
                    acc.len(),
                    vector.len(),
                ));
            }
            for (slot, value) in acc.iter_mut().zip(vector) {
                *slot += value;
            }
        }
        None => *accumulator = Some(vector),
    }
    Ok(())
}

pub fn config_from_settings(conn: &Connection) -> Result<EmbeddingConfig, String> {
    let provider_setting = crate::settings::get_setting(conn, EMBEDDING_PROVIDER_SETTING_KEY)
        .unwrap_or_else(|| "api".to_string())
        .trim()
        .to_ascii_lowercase();

    if matches!(provider_setting.as_str(), "local" | "offline" | "onnx") {
        return Err("Proveedor de embeddings no disponible en EntropIA Lite. Configurá OpenRouter en Configuración para usar embeddings remotos.".to_string());
    }
    if !matches!(provider_setting.as_str(), "" | "api" | "openrouter") {
        return Err(format!(
            "Proveedor de embeddings no soportado en Lite: {provider_setting}. Usá 'api' u 'openrouter'."
        ));
    }

    let api_key = crate::settings::get_setting(conn, "openrouter_api_key")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    if api_key.is_empty() {
        return Err("OpenRouter API key no configurada. Configurá OpenRouter para generar embeddings remotos.".to_string());
    }

    let model_name = crate::settings::get_setting(conn, OPENROUTER_EMBEDDING_MODEL_SETTING_KEY)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_OPENROUTER_EMBEDDING_MODEL.to_string());

    Ok(EmbeddingConfig {
        api_key,
        model_name,
    })
}

/// Embed a single ad-hoc query (por ejemplo, una pregunta del chat RAG)
/// usando una configuración de OpenRouter ya resuelta (vía
/// `config_from_settings`), sin tocar la base de datos.
///
/// Construye un engine transitorio sobre el cliente remoto existente — no
/// duplica el código HTTP. Pensado para llamarse dentro de `spawn_blocking`
/// SIN sostener el lock de la conexión (el cliente usa reqwest bloqueante
/// con timeout de 120s).
pub(crate) fn embed_query_text_with_config(
    config: EmbeddingConfig,
    text: &str,
) -> Result<Vec<f32>, String> {
    let engine = EmbeddingEngine::init(config)?;
    engine.embed_text(text)
}

pub fn resolve_local_embedding_model_dir(
    configured: Option<&str>,
    app_data_dir: Option<&Path>,
) -> PathBuf {
    configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            app_data_dir
                .map(|root| root.join("models").join("embeddings").join("bge-m3"))
                .unwrap_or_else(|| PathBuf::from("models/embeddings/bge-m3"))
        })
}

pub fn get_local_embedding_model_info(model_dir: Option<PathBuf>) -> LocalEmbeddingModelInfo {
    let directory = model_dir.unwrap_or_else(|| PathBuf::from("models/embeddings/bge-m3"));
    LocalEmbeddingModelInfo {
        exists: false,
        available: false,
        can_auto_download: false,
        directory: directory.to_string_lossy().to_string(),
        path: String::new(),
        size_bytes: None,
        required_files: Vec::new(),
        missing_files: Vec::new(),
        source_repo: "remote-openrouter".to_string(),
    }
}

pub fn download_local_embedding_model_files(
    _model_dir: &Path,
    app_handle: &AppHandle,
) -> Result<(), String> {
    let error = "No requerido en EntropIA Lite. Configurá OpenRouter en Configuración para usar embeddings remotos."
        .to_string();
    let _ = app_handle.emit(
        "embedding:download_error",
        EmbeddingDownloadErrorPayload {
            error: error.clone(),
        },
    );
    Err(error)
}

pub fn compute_and_store_for_asset(
    engine: Option<&EmbeddingEngine>,
    conn: &Connection,
    item_id: &str,
    asset_id: &str,
) -> Result<(), String> {
    compute_and_store_for_asset_with_unavailable_reason(engine, conn, item_id, asset_id, None)
}

pub fn compute_and_store_for_asset_with_unavailable_reason(
    engine: Option<&EmbeddingEngine>,
    conn: &Connection,
    item_id: &str,
    asset_id: &str,
    unavailable_reason: Option<&str>,
) -> Result<(), String> {
    let text = text_provider::get_asset_text(conn, asset_id)?;
    if text.trim().is_empty() {
        return Err(format!(
            "No source text available for asset '{asset_id}' (run OCR/transcription first)"
        ));
    }

    let engine = engine.ok_or_else(|| {
        embedding_degradation_log(
            item_id,
            &embedding_engine_unavailable_reason(unavailable_reason),
        )
    })?;

    let provider = engine.provider_name();
    eprintln!(
        "[nlp/embeddings] EMBED start provider={provider} item_id={item_id} asset_id={asset_id} chars={}",
        text.chars().count()
    );

    let vector = engine
        .embed_text(&text)
        .map_err(|e| embedding_degradation_log(item_id, &e))?;
    let blob = floats_to_blob(&vector);
    upsert_vec_asset(conn, item_id, asset_id, &blob)?;
    Ok(())
}

pub fn embedding_engine_unavailable_reason(last_init_error: Option<&str>) -> String {
    match last_init_error
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(error) => {
            format!("No remote embedding engine configured. Last initialization error: {error}")
        }
        None => {
            "No remote embedding engine configured. Set OpenRouter API credentials for embeddings."
                .to_string()
        }
    }
}

pub fn summarize_asset_embedding_coverage(
    conn: &Connection,
) -> Result<AssetEmbeddingCoverageSummary, String> {
    conn.query_row(
        r#"
        WITH asset_text AS (
            SELECT
                a.id AS asset_id,
                EXISTS(SELECT 1 FROM extractions e WHERE e.asset_id = a.id AND LENGTH(TRIM(COALESCE(e.text_content, ''))) > 0)
                OR EXISTS(SELECT 1 FROM transcriptions t WHERE t.asset_id = a.id AND LENGTH(TRIM(COALESCE(t.text_content, ''))) > 0) AS has_text,
                EXISTS(SELECT 1 FROM vec_assets v WHERE v.asset_id = a.id) AS has_embedding
            FROM assets a
        )
        SELECT
            COUNT(*),
            SUM(CASE WHEN has_text THEN 1 ELSE 0 END),
            SUM(CASE WHEN has_embedding THEN 1 ELSE 0 END),
            SUM(CASE WHEN has_text AND NOT has_embedding THEN 1 ELSE 0 END)
        FROM asset_text
        "#,
        [],
        |row| {
            Ok(AssetEmbeddingCoverageSummary {
                total_assets: row.get(0)?,
                assets_with_text: row.get(1)?,
                assets_with_embedding: row.get(2)?,
                assets_missing_embedding: row.get(3)?,
            })
        },
    )
    .map_err(|e| format!("Failed to summarize asset embedding coverage: {e}"))
}

pub fn list_asset_embedding_candidates(
    conn: &Connection,
    force: bool,
    limit: Option<usize>,
) -> Result<Vec<AssetEmbeddingCandidate>, String> {
    let mut sql = String::from(
        r#"
        SELECT a.id, a.item_id
        FROM assets a
        WHERE (
            EXISTS(SELECT 1 FROM extractions e WHERE e.asset_id = a.id AND LENGTH(TRIM(COALESCE(e.text_content, ''))) > 0)
            OR EXISTS(SELECT 1 FROM transcriptions t WHERE t.asset_id = a.id AND LENGTH(TRIM(COALESCE(t.text_content, ''))) > 0)
        )
        AND (?1 = 1 OR NOT EXISTS(SELECT 1 FROM vec_assets v WHERE v.asset_id = a.id))
        ORDER BY a.created_at ASC, a.id ASC
        "#,
    );

    if let Some(limit) = limit {
        sql.push_str(&format!(" LIMIT {limit}"));
    }

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare asset embedding backfill query: {e}"))?;

    let rows = stmt
        .query_map(params![if force { 1_i64 } else { 0_i64 }], |row| {
            Ok(AssetEmbeddingCandidate {
                asset_id: row.get(0)?,
                item_id: row.get(1)?,
            })
        })
        .map_err(|e| format!("Failed to query asset embedding backfill candidates: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read asset embedding backfill candidates: {e}"))
}

fn floats_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn embedding_degradation_log(item_id: &str, reason: &str) -> String {
    format!("[nlp/embeddings] Skipping embedding for {item_id}: {reason}")
}

fn upsert_vec_asset(
    conn: &Connection,
    item_id: &str,
    asset_id: &str,
    blob: &[u8],
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO vec_assets(asset_id, item_id, embedding) VALUES (?1, ?2, ?3) ON CONFLICT(asset_id) DO UPDATE SET item_id=excluded.item_id, embedding=excluded.embedding",
        params![asset_id, item_id, blob],
    )
    .map(|_| ())
    .map_err(|e| format!("[nlp/embeddings] Failed to persist asset embedding for {asset_id}: {e}"))
}

fn rolling_hash64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_first_chunk_initializes_accumulator() {
        let mut acc = None;
        accumulate_chunk_vector(&mut acc, vec![1.0, 2.0], "test-model").unwrap();
        assert_eq!(acc, Some(vec![1.0, 2.0]));
    }

    #[test]
    fn accumulate_same_dimensions_sums_componentwise() {
        let mut acc = Some(vec![1.0, 2.0]);
        accumulate_chunk_vector(&mut acc, vec![3.0, 4.0], "test-model").unwrap();
        assert_eq!(acc, Some(vec![4.0, 6.0]));
    }

    #[test]
    fn accumulate_longer_vector_errors_instead_of_panicking() {
        let mut acc = Some(vec![0.0; 768]);
        let error = accumulate_chunk_vector(&mut acc, vec![0.0; 1024], "test-model").unwrap_err();
        assert!(error.contains("dimensiones inconsistentes"));
        assert!(error.contains("768"));
        assert!(error.contains("1024"));
        assert!(error.contains("test-model"));
    }

    #[test]
    fn accumulate_shorter_vector_errors_instead_of_corrupting_average() {
        let mut acc = Some(vec![0.0; 1024]);
        let error = accumulate_chunk_vector(&mut acc, vec![0.0; 768], "test-model").unwrap_err();
        assert!(error.contains("dimensiones inconsistentes"));
    }

    #[test]
    fn upsert_vec_asset_twice_emits_update_not_delete_insert() {
        // Regression for #21: re-embedding the same asset must emit ONE UPDATE in
        // sync_oplog, NOT a DELETE+INSERT pair. INSERT OR REPLACE = DELETE+INSERT
        // fires the `_d` tombstone trigger, which can delete the row on the remote.
        use crate::sync::capture::ensure_capture;
        use crate::sync::test_support::{new_synced_test_db, set_session_with_capture};

        let conn = new_synced_test_db();
        ensure_capture(&conn).expect("ensure capture");
        set_session_with_capture(&conn);

        upsert_vec_asset(&conn, "item-1", "asset-1", &[1u8, 2, 3]).expect("first upsert");
        upsert_vec_asset(&conn, "item-1", "asset-1", &[4u8, 5, 6]).expect("second upsert");

        let deletes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync_oplog WHERE table_name = 'vec_assets' AND op = 'D'",
                [],
                |row| row.get(0),
            )
            .expect("count deletes");
        let updates: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync_oplog WHERE table_name = 'vec_assets' AND op = 'U'",
                [],
                |row| row.get(0),
            )
            .expect("count updates");

        assert_eq!(deletes, 0, "re-embedding must not emit a tombstone (op 'D')");
        assert!(updates >= 1, "re-embedding must emit an UPDATE (op 'U')");
    }
}
