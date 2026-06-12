use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::db::state::AppDbState;
const SECRET_REF_PREFIX: &str = "secret_ref:";
const OPENROUTER_MODEL_KEY: &str = "openrouter_model";
const LEGACY_DEFAULT_OPENROUTER_MODEL: &str = "google/gemma-3-4b-it";
pub(crate) const DEFAULT_OPENROUTER_MODEL: &str = "google/gemma-4-26b-a4b-it";
const SECRET_SERVICE: &str = "EntropIA Lite";
const SECRET_KEYS: &[&str] = &[
    "openrouter_api_key",
    "assemblyai_api_key",
    "glm_ocr_api_key",
];

async fn invalidate_dependency_probe_cache_if_needed(
    key: &str,
    deps: Option<&State<'_, crate::deps::DepsState>>,
) {
    if crate::deps::should_invalidate_cache_for_setting(key) {
        if let Some(deps_state) = deps {
            crate::deps::invalidate_probe_cache(deps_state.inner()).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn settings_get(
    key: String,
    db: State<'_, AppDbState>,
) -> Result<Option<String>, String> {
    let conn = db
        .ui_conn
        .lock()
        .map_err(|e| format!("DB lock error: {e}"))?;
    let result = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok();
    Ok(result)
}

#[tauri::command]
pub async fn settings_set(
    key: String,
    value: String,
    app_handle: AppHandle,
    db: State<'_, AppDbState>,
    deps: State<'_, crate::deps::DepsState>,
) -> Result<(), String> {
    let should_invalidate = crate::deps::should_invalidate_cache_for_setting(&key);
    // Keyring calls (Credential Manager on Windows, Secret Service on Linux)
    // are blocking; keep them off the async runtime workers.
    let prepared = {
        let key = key.clone();
        let value = value.clone();
        tokio::task::spawn_blocking(move || prepare_setting_value_for_storage(&key, &value))
            .await
            .map_err(|e| format!("Secret storage task failed: {e}"))
    };
    let stored_value = match prepared {
        Ok(Ok(stored_value)) => stored_value,
        Ok(Err(error)) | Err(error) => {
            crate::app_logs::error(
                &app_handle,
                setting_log_source(&key),
                format!("No se pudo guardar configuración: key={key}; {error}"),
            );
            return Err(error);
        }
    };
    {
        let conn = db.ui_conn.lock().map_err(|e| {
            let error = format!("DB lock error: {e}");
            crate::app_logs::error(
                &app_handle,
                setting_log_source(&key),
                format!("No se pudo guardar configuración: key={key}; {error}"),
            );
            error
        })?;
        conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)",
            params![key.as_str(), stored_value.as_str()],
        )
        .map_err(|e| {
            let error = format!("Failed to save setting: {e}");
            crate::app_logs::error(
                &app_handle,
                setting_log_source(&key),
                format!("No se pudo guardar configuración: key={key}; {error}"),
            );
            error
        })?;
    }
    if should_invalidate {
        invalidate_dependency_probe_cache_if_needed(&key, Some(&deps)).await;
    }
    crate::app_logs::info(
        &app_handle,
        setting_log_source(&key),
        format!("Configuración guardada: key={key}"),
    );
    Ok(())
}

#[tauri::command]
pub async fn settings_get_all(db: State<'_, AppDbState>) -> Result<Vec<SettingEntry>, String> {
    let conn = db
        .ui_conn
        .lock()
        .map_err(|e| format!("DB lock error: {e}"))?;
    let mut stmt = conn
        .prepare("SELECT key, value FROM app_settings ORDER BY key")
        .map_err(|e| format!("Failed to prepare settings query: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SettingEntry {
                key: row.get(0)?,
                value: row.get(1)?,
            })
        })
        .map_err(|e| format!("Failed to query settings: {e}"))?;
    let mut entries = Vec::new();
    for entry in rows.flatten() {
        entries.push(entry);
    }
    Ok(entries)
}

#[tauri::command]
pub async fn settings_delete(
    key: String,
    app_handle: AppHandle,
    db: State<'_, AppDbState>,
    deps: State<'_, crate::deps::DepsState>,
) -> Result<(), String> {
    let should_invalidate = crate::deps::should_invalidate_cache_for_setting(&key);
    if is_secret_key(&key) {
        // Blocking Win32 credential call — run off the async workers.
        let secret_key = key.clone();
        let _ = tokio::task::spawn_blocking(move || delete_secret(&secret_key)).await;
    }
    {
        let conn = db.ui_conn.lock().map_err(|e| {
            let error = format!("DB lock error: {e}");
            crate::app_logs::error(
                &app_handle,
                setting_log_source(&key),
                format!("No se pudo borrar configuración: key={key}; {error}"),
            );
            error
        })?;
        conn.execute(
            "DELETE FROM app_settings WHERE key = ?1",
            params![key.as_str()],
        )
        .map_err(|e| {
            let error = format!("Failed to delete setting: {e}");
            crate::app_logs::error(
                &app_handle,
                setting_log_source(&key),
                format!("No se pudo borrar configuración: key={key}; {error}"),
            );
            error
        })?;
    }
    if should_invalidate {
        invalidate_dependency_probe_cache_if_needed(&key, Some(&deps)).await;
    }
    crate::app_logs::info(
        &app_handle,
        setting_log_source(&key),
        format!("Configuración borrada: key={key}"),
    );
    Ok(())
}

fn is_secret_key(key: &str) -> bool {
    SECRET_KEYS.contains(&key)
}

fn setting_log_source(key: &str) -> &'static str {
    match key {
        "openrouter_api_key"
        | "openrouter_model"
        | "openrouter_embedding_model"
        | "llm_mode"
        | "embedding_provider" => "settings/openrouter",
        "assemblyai_api_key" | "assemblyai_role_speaker_identification" | "stt_mode" => {
            "settings/assemblyai"
        }
        "glm_ocr_api_key" | "ocrh_mode" => "settings/glm_ocr",
        key if key.starts_with("prompt_") => "settings/prompts",
        key if key.starts_with("llm_") => "settings/llm",
        _ => "settings",
    }
}

fn secret_ref_for_key(key: &str) -> String {
    format!("{SECRET_REF_PREFIX}{key}")
}

fn keyring_entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SECRET_SERVICE, key)
        .map_err(|error| format!("No se pudo abrir keyring para '{key}': {error}"))
}

fn store_secret(key: &str, value: &str) -> Result<(), String> {
    keyring_entry(key)?
        .set_password(value)
        .map_err(|error| format!("No se pudo guardar secreto '{key}' en keyring: {error}"))
}

fn read_secret(key: &str) -> Result<String, String> {
    keyring_entry(key)?
        .get_password()
        .map_err(|error| format!("No se pudo leer secreto '{key}' desde keyring: {error}"))
}

fn delete_secret(key: &str) -> Result<(), String> {
    keyring_entry(key)?
        .delete_credential()
        .map_err(|error| format!("No se pudo borrar secreto '{key}' del keyring: {error}"))
}

fn prepare_setting_value_for_storage(key: &str, value: &str) -> Result<String, String> {
    if !is_secret_key(key) {
        return Ok(value.to_string());
    }

    let trimmed = value.trim();
    if trimmed.starts_with(SECRET_REF_PREFIX) {
        return Ok(trimmed.to_string());
    }
    if trimmed.is_empty() {
        let _ = delete_secret(key);
        return Ok(String::new());
    }

    store_secret(key, trimmed)?;
    Ok(secret_ref_for_key(key))
}

fn resolve_secret_ref(key: &str, value: &str) -> Option<String> {
    if is_secret_key(key) && value.starts_with(SECRET_REF_PREFIX) {
        return read_secret(key).ok();
    }
    None
}

// ---------------------------------------------------------------------------
// Internal helpers (for Rust-side reading, used by LLM worker)
// ---------------------------------------------------------------------------

/// Read a setting value directly from a rusqlite connection.
/// Used by the LLM worker to read API keys without going through Tauri state.
pub fn get_setting(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok()?;
    resolve_secret_ref(key, &value).or(Some(value))
}

pub fn migrate_legacy_default_openrouter_model(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute(
        "UPDATE app_settings SET value = ?1 WHERE key = ?2 AND value = ?3",
        rusqlite::params![
            DEFAULT_OPENROUTER_MODEL,
            OPENROUTER_MODEL_KEY,
            LEGACY_DEFAULT_OPENROUTER_MODEL
        ],
    )
    .map(|_| ())
    .map_err(|e| format!("Failed to migrate default OpenRouter model: {e}"))
}

pub fn get_secret_setting(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    let value = get_setting(conn, key)?;
    if value.trim().starts_with(SECRET_REF_PREFIX) {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )
        .expect("settings table");
        conn
    }

    #[test]
    fn migrates_only_legacy_default_openrouter_model() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)",
            params![OPENROUTER_MODEL_KEY, LEGACY_DEFAULT_OPENROUTER_MODEL],
        )
        .expect("insert legacy default");

        migrate_legacy_default_openrouter_model(&conn).expect("migration");

        assert_eq!(
            get_setting(&conn, OPENROUTER_MODEL_KEY).as_deref(),
            Some(DEFAULT_OPENROUTER_MODEL)
        );
    }

    #[test]
    fn preserves_custom_openrouter_model() {
        let conn = setup_conn();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)",
            params![OPENROUTER_MODEL_KEY, "anthropic/claude-3.7-sonnet"],
        )
        .expect("insert custom model");

        migrate_legacy_default_openrouter_model(&conn).expect("migration");

        assert_eq!(
            get_setting(&conn, OPENROUTER_MODEL_KEY).as_deref(),
            Some("anthropic/claude-3.7-sonnet")
        );
    }
}
