use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::state::AppDbState;
const SECRET_REF_PREFIX: &str = "secret_ref:";
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
    db: State<'_, AppDbState>,
    deps: State<'_, crate::deps::DepsState>,
) -> Result<(), String> {
    let should_invalidate = crate::deps::should_invalidate_cache_for_setting(&key);
    let stored_value = prepare_setting_value_for_storage(&key, &value)?;
    {
        let conn = db
            .ui_conn
            .lock()
            .map_err(|e| format!("DB lock error: {e}"))?;
        conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)",
            params![key.as_str(), stored_value.as_str()],
        )
        .map_err(|e| format!("Failed to save setting: {e}"))?;
    }
    if should_invalidate {
        invalidate_dependency_probe_cache_if_needed(&key, Some(&deps)).await;
    }
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
    for row in rows {
        if let Ok(entry) = row {
            entries.push(entry);
        }
    }
    Ok(entries)
}

#[tauri::command]
pub async fn settings_delete(
    key: String,
    db: State<'_, AppDbState>,
    deps: State<'_, crate::deps::DepsState>,
) -> Result<(), String> {
    let should_invalidate = crate::deps::should_invalidate_cache_for_setting(&key);
    if is_secret_key(&key) {
        let _ = delete_secret(&key);
    }
    {
        let conn = db
            .ui_conn
            .lock()
            .map_err(|e| format!("DB lock error: {e}"))?;
        conn.execute(
            "DELETE FROM app_settings WHERE key = ?1",
            params![key.as_str()],
        )
        .map_err(|e| format!("Failed to delete setting: {e}"))?;
    }
    if should_invalidate {
        invalidate_dependency_probe_cache_if_needed(&key, Some(&deps)).await;
    }
    Ok(())
}

fn is_secret_key(key: &str) -> bool {
    SECRET_KEYS.contains(&key)
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

pub fn get_secret_setting(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    let value = get_setting(conn, key)?;
    if value.trim().starts_with(SECRET_REF_PREFIX) {
        None
    } else {
        Some(value)
    }
}
