/// Tauri IPC commands for transcription operations.
use super::{TranscriptionJob, TranscriptionQueue};
use crate::db::state::AppDbState;
use crate::nlp::NlpQueue;
use std::path::Path;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub async fn test_assemblyai_connection(
    api_key: String,
    app_handle: AppHandle,
    db: State<'_, AppDbState>,
) -> Result<(), String> {
    let api_key = if api_key.trim().is_empty() {
        let conn = db
            .ui_conn
            .lock()
            .map_err(|e| format!("DB lock poisoned: {e}"))?;
        crate::settings::get_secret_setting(&conn, super::ASSEMBLYAI_API_KEY_SETTING)
            .unwrap_or_default()
    } else {
        api_key
    };

    let client = match super::assemblyai::AssemblyAiClient::new(api_key.trim().to_string()) {
        Ok(client) => client,
        Err(error) => {
            crate::app_logs::error(
                &app_handle,
                "settings/assemblyai",
                format!("Falló prueba de conexión AssemblyAI: {error}"),
            );
            return Err(error);
        }
    };
    let result = client.test_connection().await;
    match &result {
        Ok(()) => crate::app_logs::info(
            &app_handle,
            "settings/assemblyai",
            "Conexión AssemblyAI verificada",
        ),
        Err(error) => crate::app_logs::error(
            &app_handle,
            "settings/assemblyai",
            format!("Falló prueba de conexión AssemblyAI: {error}"),
        ),
    }
    result
}

/// Submit a transcription job to the background worker queue.
///
/// Returns immediately with `Ok("queued")`. The worker will process the job
/// asynchronously and emit `transcription:progress`, `transcription:complete`,
/// or `transcription:error` events.
///
/// # Arguments
/// * `asset_id`   — unique ID of the asset in the database
/// * `asset_path` — absolute filesystem path to the audio file
/// * `transcription_queue` — managed state injected by Tauri
#[tauri::command]
pub async fn transcribe_audio(
    asset_id: String,
    asset_path: String,
    app_handle: AppHandle,
    transcription_queue: State<'_, TranscriptionQueue>,
    db: State<'_, AppDbState>,
) -> Result<String, String> {
    super::ensure_transcription_runtime_ready(&app_handle)?;
    {
        let conn = db
            .ui_conn
            .lock()
            .map_err(|e| format!("DB lock poisoned: {e}"))?;
        super::ensure_selected_cloud_key(&conn)?;
    }

    crate::app_logs::info(
        &app_handle,
        "transcription",
        format!("Trabajo de transcripción encolado: asset_id={asset_id}"),
    );

    let job = TranscriptionJob {
        asset_id,
        asset_path,
    };

    transcription_queue.submit(job)?;
    Ok("queued".to_string())
}

/// Update the text_content of the latest transcription for an asset.
///
/// This allows users to manually correct transcription output.
/// Downstream NLP refresh is debounced in the frontend after a period of
/// user inactivity, so this command only persists the edited text.
#[tauri::command]
pub async fn update_transcription_text_cmd(
    asset_id: String,
    text_content: String,
    db: State<'_, AppDbState>,
    _nlp_queue: State<'_, NlpQueue>,
) -> Result<(), String> {
    let conn = db
        .ui_conn
        .lock()
        .map_err(|e| format!("DB lock poisoned: {e}"))?;

    // Find the latest transcription for this asset
    let mut stmt = conn
        .prepare(
            "SELECT id FROM transcriptions WHERE asset_id = ?1 ORDER BY created_at DESC LIMIT 1",
        )
        .map_err(|e| format!("Failed to prepare query: {e}"))?;

    let transcription_id: Result<String, _> = stmt.query_row([&asset_id], |row| row.get(0));

    drop(stmt); // release borrow before execute

    // If no transcription exists, this is a no-op.
    if let Ok(id) = transcription_id {
        conn.execute(
            "UPDATE transcriptions SET text_content = ?1 WHERE id = ?2",
            rusqlite::params![text_content, id],
        )
        .map_err(|e| format!("Failed to update transcription text: {e}"))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn transcribe_dictation(
    audio_path: String,
    app_handle: AppHandle,
    db: State<'_, AppDbState>,
) -> Result<String, String> {
    super::ensure_transcription_runtime_ready(&app_handle)?;
    crate::app_logs::info(
        &app_handle,
        "transcription",
        "Transcripción de dictado iniciada",
    );
    let db_path = db.db_path.clone();
    {
        let conn = db
            .ui_conn
            .lock()
            .map_err(|e| format!("DB lock poisoned: {e}"))?;
        super::ensure_selected_cloud_key(&conn)?;
    }

    let audio_path_for_worker = audio_path.clone();
    let app_handle_for_worker = app_handle.clone();
    let transcription_result = tauri::async_runtime::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| format!("Failed to open settings DB for dictation: {e}"))?;

        super::transcribe_dictation_with_selected_provider(
            &app_handle_for_worker,
            Some(db_path.as_path()),
            &conn,
            &audio_path_for_worker,
        )
        .map(|result| result.transcription)
    })
    .await
    .map_err(|e| format!("Dictation task failed: {e}"))?;

    // Only delete the recording when it lives inside the app data dir or the
    // OS temp dir — never delete arbitrary user files from an IPC-supplied
    // path. An out-of-scope path skips cleanup with a log instead of erroring
    // the whole transcription.
    let app_data_dir = app_handle.path().app_data_dir().ok();
    let cleanup_result = if is_safe_dictation_cleanup_path(
        &audio_path,
        app_data_dir.as_deref(),
        &std::env::temp_dir(),
    ) {
        super::cleanup_temp_audio_file(&audio_path)
    } else {
        crate::app_logs::warn(
            &app_handle,
            "transcription",
            format!(
                "Limpieza temporal omitida: la ruta está fuera de los directorios permitidos: {audio_path}"
            ),
        );
        Ok(())
    };

    match (transcription_result, cleanup_result) {
        (Ok(result), Ok(())) => Ok(result.text.trim().to_string()),
        (Ok(result), Err(cleanup_error)) => {
            eprintln!("[transcription] Dictation cleanup warning: {cleanup_error}");
            crate::app_logs::warn(
                &app_handle,
                "transcription",
                format!("Dictado transcripto, pero falló limpieza temporal: {cleanup_error}"),
            );
            Ok(result.text.trim().to_string())
        }
        (Err(error), Ok(())) => {
            crate::app_logs::error(
                &app_handle,
                "transcription",
                format!("Dictado falló: {error}"),
            );
            Err(error)
        }
        (Err(error), Err(cleanup_error)) => {
            crate::app_logs::error(
                &app_handle,
                "transcription",
                format!(
                    "Dictado falló y también falló limpieza temporal: {error}; {cleanup_error}"
                ),
            );
            Err(format!(
                "{error}\nTemporary file cleanup failed: {cleanup_error}"
            ))
        }
    }
}

/// True when the dictation audio path resolves inside the app data dir or the
/// OS temp dir — the only locations the frontend writes dictation recordings
/// to, and therefore the only locations this command is allowed to delete from.
fn is_safe_dictation_cleanup_path(
    audio_path: &str,
    app_data_dir: Option<&Path>,
    temp_dir: &Path,
) -> bool {
    let within = |root: &Path| crate::path_utils::ensure_within_dir(audio_path, root).is_ok();
    app_data_dir.is_some_and(within) || within(temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictation_cleanup_allows_app_data_and_temp_dir_paths() {
        let app_data = tempfile::tempdir().expect("tempdir app data");
        let temp = tempfile::tempdir().expect("tempdir temp");
        let app_data_file = app_data.path().join("dictation.wav");
        let temp_file = temp.path().join("dictation.wav");
        std::fs::write(&app_data_file, b"audio").expect("write app data file");
        std::fs::write(&temp_file, b"audio").expect("write temp file");

        assert!(is_safe_dictation_cleanup_path(
            &app_data_file.to_string_lossy(),
            Some(app_data.path()),
            temp.path(),
        ));
        assert!(is_safe_dictation_cleanup_path(
            &temp_file.to_string_lossy(),
            Some(app_data.path()),
            temp.path(),
        ));
        // Missing app data dir still allows temp-dir paths.
        assert!(is_safe_dictation_cleanup_path(
            &temp_file.to_string_lossy(),
            None,
            temp.path(),
        ));
    }

    #[test]
    fn dictation_cleanup_rejects_paths_outside_allowed_dirs() {
        let app_data = tempfile::tempdir().expect("tempdir app data");
        let temp = tempfile::tempdir().expect("tempdir temp");
        let outside = tempfile::tempdir().expect("tempdir outside");
        let outside_file = outside.path().join("document.wav");
        std::fs::write(&outside_file, b"audio").expect("write outside file");

        assert!(!is_safe_dictation_cleanup_path(
            &outside_file.to_string_lossy(),
            Some(app_data.path()),
            temp.path(),
        ));
        assert!(!is_safe_dictation_cleanup_path(
            "",
            Some(app_data.path()),
            temp.path(),
        ));
    }
}
