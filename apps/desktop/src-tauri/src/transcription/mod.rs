pub mod assemblyai;
pub mod commands;
pub mod engine;

use crate::nlp::{lookup_item_id_for_asset, NlpJob, NlpQueue};
use engine::TranscriptionResult;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

const ASSEMBLYAI_API_KEY_SETTING: &str = "assemblyai_api_key";
const ASSEMBLYAI_SPEAKER_LABELS_SETTING: &str = "assemblyai_role_speaker_identification";

#[derive(Clone, Serialize)]
pub struct TranscriptionProgressPayload {
    pub asset_id: String,
    pub pct: u8,
    pub stage: String,
}

#[derive(Clone, Serialize)]
pub struct TranscriptionCompletePayload {
    pub asset_id: String,
    pub text: String,
    pub language: String,
    pub duration_ms: u64,
    pub segments_count: usize,
}

#[derive(Clone, Serialize)]
pub struct TranscriptionErrorPayload {
    pub asset_id: String,
    pub error: String,
}

pub struct TranscriptionJob {
    pub asset_id: String,
    pub asset_path: String,
}

pub struct ManagedTranscriptionResult {
    pub transcription: TranscriptionResult,
    pub model_name: &'static str,
}

pub struct TranscriptionQueue {
    sender: mpsc::Sender<TranscriptionJob>,
}

impl TranscriptionQueue {
    pub fn new() -> (Self, mpsc::Receiver<TranscriptionJob>) {
        let (sender, receiver) = mpsc::channel::<TranscriptionJob>(32);
        (Self { sender }, receiver)
    }

    pub fn submit(&self, job: TranscriptionJob) -> Result<(), String> {
        self.sender
            .try_send(job)
            .map_err(|e| format!("Failed to enqueue transcription job: {e}"))
    }

    pub fn start_worker(
        db_path: PathBuf,
        mut receiver: mpsc::Receiver<TranscriptionJob>,
        app_handle: AppHandle,
    ) {
        std::thread::Builder::new()
            .name("transcription-worker".to_string())
            .spawn(move || {
                let conn = match rusqlite::Connection::open(&db_path) {
                    Ok(c) => {
                        let _ = c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
                        c
                    }
                    Err(e) => {
                        eprintln!("[transcription] Failed to open worker DB connection: {e}");
                        while let Some(job) = receiver.blocking_recv() {
                            emit_error(
                                &app_handle,
                                job.asset_id,
                                format!("Failed to open transcription DB connection: {e}"),
                            );
                        }
                        return;
                    }
                };

                eprintln!("[transcription] EntropIA Lite transcription worker ready; AssemblyAI remote only");
                while let Some(job) = receiver.blocking_recv() {
                    let asset_id = job.asset_id.clone();
                    match process_job(&conn, &job, &db_path, &app_handle) {
                        Ok(result) => emit_complete(&app_handle, asset_id, result),
                        Err(error) => emit_error(&app_handle, asset_id, error),
                    }
                }
            })
            .expect("Failed to spawn transcription worker thread");
    }
}

pub fn ensure_transcription_runtime_ready(_app_handle: &AppHandle) -> Result<(), String> {
    Ok(())
}

pub fn ensure_selected_cloud_key(conn: &rusqlite::Connection) -> Result<(), String> {
    let api_key = crate::settings::get_setting(conn, ASSEMBLYAI_API_KEY_SETTING)
        .unwrap_or_default()
        .trim()
        .to_string();

    if api_key.is_empty() {
        return Err("EntropIA Lite requiere AssemblyAI para transcripción.".to_string());
    }

    Ok(())
}

pub fn transcribe_with_selected_provider(
    app_handle: &AppHandle,
    settings_db_path: Option<&Path>,
    conn: &rusqlite::Connection,
    asset_id: Option<&str>,
    audio_path: &str,
) -> Result<ManagedTranscriptionResult, String> {
    let assemblyai_api_key = crate::settings::get_setting(conn, ASSEMBLYAI_API_KEY_SETTING)
        .unwrap_or_default()
        .trim()
        .to_string();

    if assemblyai_api_key.is_empty() {
        return Err("EntropIA Lite requiere AssemblyAI para transcripción.".to_string());
    }

    let enable_speaker_labels = parse_enabled_by_default(
        crate::settings::get_setting(conn, ASSEMBLYAI_SPEAKER_LABELS_SETTING).as_deref(),
    );

    let mut emit_provider_progress = |pct: u8, stage: &str| {
        if let Some(asset_id) = asset_id {
            emit_progress(app_handle, asset_id, pct, stage);
        }
    };

    let transcription = transcribe_with_assemblyai_provider(
        audio_path,
        &assemblyai_api_key,
        enable_speaker_labels,
        &mut emit_provider_progress,
    )?;

    if let Some(path) = settings_db_path {
        eprintln!(
            "[transcription] AssemblyAI settings loaded from {}",
            path.display()
        );
    }

    Ok(ManagedTranscriptionResult {
        transcription,
        model_name: "assemblyai-universal",
    })
}

fn transcribe_with_assemblyai_provider<F>(
    audio_path: &str,
    api_key: &str,
    enable_speaker_labels: bool,
    on_progress: F,
) -> Result<TranscriptionResult, String>
where
    F: FnMut(u8, &str),
{
    tauri::async_runtime::block_on(async move {
        assemblyai::AssemblyAiClient::new(api_key.to_string())
            .transcribe_file(Path::new(audio_path), enable_speaker_labels, on_progress)
            .await
    })
}

fn parse_enabled_by_default(value: Option<&str>) -> bool {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };

    !matches!(
        value.to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

pub fn cleanup_temp_audio_file(audio_path: &str) -> Result<(), String> {
    match std::fs::remove_file(audio_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Failed to remove temporary audio file {audio_path}: {error}"
        )),
    }
}

fn save_transcription(
    conn: &rusqlite::Connection,
    asset_id: &str,
    result: &TranscriptionResult,
    model_name: &str,
) -> Result<Option<String>, String> {
    let segments_json = serde_json::to_string(&result.segments)
        .map_err(|e| format!("Failed to serialize segments: {e}"))?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO transcriptions(id, asset_id, text_content, language, duration_ms, model, segments, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(asset_id) DO UPDATE SET
           text_content = excluded.text_content,
           language = excluded.language,
           duration_ms = excluded.duration_ms,
           model = excluded.model,
           segments = excluded.segments,
           confidence = excluded.confidence,
           created_at = excluded.created_at",
        rusqlite::params![
            id,
            asset_id,
            result.text,
            result.language,
            result.duration_ms as i64,
            model_name,
            segments_json,
            None::<f64>,
            now,
        ],
    )
    .map_err(|e| format!("Failed to upsert transcription: {e}"))?;

    lookup_item_id_for_asset(conn, asset_id)
}

fn process_job(
    conn: &rusqlite::Connection,
    job: &TranscriptionJob,
    settings_db_path: &Path,
    app_handle: &AppHandle,
) -> Result<TranscriptionResult, String> {
    emit_progress(app_handle, &job.asset_id, 10, "reading");
    eprintln!("[transcription] Transcribing: {}", job.asset_path);
    emit_progress(app_handle, &job.asset_id, 30, "transcribing");

    let ManagedTranscriptionResult {
        transcription: result,
        model_name,
    } = transcribe_with_selected_provider(
        app_handle,
        Some(settings_db_path),
        conn,
        Some(&job.asset_id),
        &job.asset_path,
    )?;

    emit_progress(app_handle, &job.asset_id, 80, "saving");

    if let Some(item_id) = save_transcription(conn, &job.asset_id, &result, model_name)? {
        let nlp_queue = app_handle.state::<NlpQueue>();
        let _ = nlp_queue.submit(NlpJob::ExtractEntitiesForAsset {
            item_id: item_id.clone(),
            asset_id: job.asset_id.clone(),
        });
        let _ = nlp_queue.submit(NlpJob::IndexFts {
            item_id: item_id.clone(),
        });
        let _ = nlp_queue.submit(NlpJob::ComputeAssetEmbedding {
            item_id,
            asset_id: job.asset_id.clone(),
        });
    }

    emit_progress(app_handle, &job.asset_id, 100, "done");
    Ok(result)
}

fn emit_progress(app_handle: &AppHandle, asset_id: &str, pct: u8, stage: &str) {
    if pct == 0 || pct == 10 || pct == 100 {
        crate::app_logs::info(
            app_handle,
            "transcription",
            format!("Transcripción asset_id={asset_id} etapa={stage} progreso={pct}%"),
        );
    }
    let _ = app_handle.emit(
        "transcription:progress",
        TranscriptionProgressPayload {
            asset_id: asset_id.to_string(),
            pct,
            stage: stage.to_string(),
        },
    );
}

fn emit_complete(app_handle: &AppHandle, asset_id: String, result: TranscriptionResult) {
    let text_content = result.text.trim().to_string();
    let text_length = text_content.len();
    crate::app_logs::info(
        app_handle,
        "transcription",
        format!(
            "Transcripción completada: asset_id={} chars={}",
            asset_id, text_length
        ),
    );
    let _ = app_handle.emit(
        "transcription:complete",
        TranscriptionCompletePayload {
            asset_id,
            text: text_content,
            language: result.language,
            duration_ms: result.duration_ms,
            segments_count: result.segments.len(),
        },
    );
}

fn emit_error(app_handle: &AppHandle, asset_id: String, error: String) {
    crate::app_logs::error(
        app_handle,
        "transcription",
        format!("Transcripción falló: asset_id={asset_id} error={error}"),
    );
    let _ = app_handle.emit(
        "transcription:error",
        TranscriptionErrorPayload { asset_id, error },
    );
}

#[cfg(test)]
mod tests {
    use super::parse_enabled_by_default;

    #[test]
    fn speaker_labels_setting_defaults_enabled_when_missing_or_blank() {
        assert!(parse_enabled_by_default(None));
        assert!(parse_enabled_by_default(Some("")));
        assert!(parse_enabled_by_default(Some("   ")));
    }

    #[test]
    fn speaker_labels_setting_disables_only_explicit_false_values() {
        for value in ["false", "False", "0", "no", "off"] {
            assert!(!parse_enabled_by_default(Some(value)));
        }

        for value in ["true", "1", "yes", "on", "anything-else"] {
            assert!(parse_enabled_by_default(Some(value)));
        }
    }
}
