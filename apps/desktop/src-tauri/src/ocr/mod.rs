pub mod commands;
pub mod glm_ocr;
pub mod pdf_probe;
pub mod postprocess;
pub mod provider;

mod pdf;
pub mod reading_order;

#[cfg(debug_assertions)]
mod debug_viz;

use crate::nlp::{lookup_item_id_for_asset, NlpJob, NlpQueue};
use base64::Engine;
use glm_ocr::{GlmOcrLayoutDetail, GlmOcrResponse};
use provider::LayoutCategory;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{mpsc, Notify, Semaphore};

const OCRH_SETTING_GLM_OCR_API_KEY: &str = "glm_ocr_api_key";

/// Maximum number of OCR jobs processed concurrently.
///
/// GLM-OCR jobs are network-bound (a multi-MB base64 upload followed by remote
/// parsing), so overlapping a few requests hides most of the request latency.
/// Three permits keep peak memory bounded — each in-flight job holds its file
/// bytes plus a ~1.33x base64 copy — and stay polite to the upstream API.
/// Pdfium work needs no permit here: it serializes through the dedicated
/// render thread in `pdf.rs`.
const MAX_CONCURRENT_OCR_JOBS: usize = 3;

#[derive(Clone, Serialize)]
pub struct OcrProgressPayload {
    pub asset_id: String,
    pub pct: u8,
    pub stage: String,
}

#[derive(Clone, Serialize)]
pub struct OcrCompletePayload {
    pub asset_id: String,
    pub method: String,
    pub text_length: usize,
    pub text_content: String,
}

#[derive(Clone, Serialize)]
pub struct OcrErrorPayload {
    pub asset_id: String,
    pub error: String,
}

#[derive(Debug, Clone)]
struct ProcessedOcrOutput {
    ocr: provider::OcrOutput,
    layout: Option<LayoutPersistencePayload>,
}

#[derive(Debug, Clone, Serialize)]
struct LayoutPersistencePayload {
    model: String,
    image_width: u32,
    image_height: u32,
    regions: Vec<PersistedLayoutRegion>,
    blocks: Vec<PersistedLayoutBlock>,
}

#[derive(Debug, Clone, Serialize)]
struct PersistedLayoutRegion {
    page: u32,
    image_width: u32,
    image_height: u32,
    category: String,
    bbox: LayoutBbox,
    confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
struct PersistedLayoutBlock {
    page: u32,
    image_width: u32,
    image_height: u32,
    label: String,
    content: String,
    bbox: LayoutBbox,
    order: i32,
    group_id: i32,
}

#[derive(Debug, Clone, Serialize)]
struct LayoutBbox {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

pub struct OcrJob {
    pub asset_id: String,
    pub asset_path: String,
    pub asset_type: String,
    pub mode: OcrMode,
}

/// Lite always uses remote GLM-OCR. The enum is kept to preserve the command/job contract.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum OcrMode {
    #[default]
    High,
}

pub struct OcrQueue {
    sender: mpsc::Sender<OcrJob>,
}

impl OcrQueue {
    pub fn new() -> (Self, mpsc::Receiver<OcrJob>) {
        let (sender, receiver) = mpsc::channel::<OcrJob>(64);
        (Self { sender }, receiver)
    }

    pub fn submit(&self, job: OcrJob) -> Result<(), String> {
        self.sender
            .try_send(job)
            .map_err(|e| format!("Failed to enqueue OCR job: {e}"))
    }

    pub fn start_worker(
        db_path: std::path::PathBuf,
        mut receiver: mpsc::Receiver<OcrJob>,
        app_handle: AppHandle,
    ) {
        tauri::async_runtime::spawn(async move {
            pdf::init_pdfium_path(&app_handle);
            eprintln!(
                "[OCR] EntropIA Lite OCR worker ready; GLM-OCR remote only \
                 (up to {MAX_CONCURRENT_OCR_JOBS} concurrent jobs)"
            );

            let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_OCR_JOBS));
            let in_flight = Arc::new(InFlightAssets::default());

            // Dispatcher: pulls jobs in FIFO order and spawns one task per
            // job, never blocking on gating itself — a duplicate enqueue of
            // an asset whose job is still running must not stall the queue
            // for unrelated assets (head-of-line blocking). When the queue
            // sender drops on app shutdown, `recv()` returns `None` and the
            // dispatcher exits — same cancel-on-shutdown behavior as before.
            while let Some(job) = receiver.recv().await {
                let semaphore = Arc::clone(&semaphore);
                let in_flight = Arc::clone(&in_flight);
                let db_path = db_path.clone();
                let app_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    // Acquire the per-asset gate FIRST, then a concurrency
                    // permit: a duplicate-asset task parked on the gate holds
                    // no permit, so it can never starve other assets. The
                    // gate still guarantees the same asset is never processed
                    // by two jobs concurrently; the token's and permit's
                    // Drops release both even if the job task panics.
                    let _token = in_flight.begin(&job.asset_id).await;
                    let _permit = semaphore
                        .acquire_owned()
                        .await
                        .expect("OCR semaphore is never closed");
                    run_job(&db_path, &job, &app_handle).await;
                });
            }
        });
    }
}

/// Process one OCR job end to end with its own SQLite connection.
///
/// Each concurrent job opens a dedicated connection (same WAL/foreign-keys
/// pragmas and busy timeout as the old single worker connection): rusqlite
/// connections must never be shared across concurrent tasks. All blocking
/// rusqlite work (open, settings read, persistence) runs on the blocking
/// pool — with `busy_timeout` = 5s a contended database would otherwise pin
/// a shared async worker thread for seconds.
async fn run_job(db_path: &std::path::Path, job: &OcrJob, app_handle: &AppHandle) {
    let asset_id = job.asset_id.clone();

    let setup_db_path = db_path.to_path_buf();
    let setup = tokio::task::spawn_blocking(move || {
        open_worker_connection(&setup_db_path).map(|conn| {
            let api_key = get_glm_ocr_api_key(&conn);
            (conn, api_key)
        })
    })
    .await
    .map_err(|e| format!("OCR DB setup task panicked: {e}"))
    .and_then(|result| result);

    let (conn, api_key) = match setup {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("[OCR] Failed to open worker DB connection: {e}");
            emit_error(
                app_handle,
                asset_id,
                format!("Failed to open OCR DB connection: {e}"),
            );
            return;
        }
    };

    match process_job(&api_key, job, app_handle).await {
        Ok(output) => complete_job(conn, app_handle, &asset_id, output).await,
        Err(error) => emit_error(app_handle, asset_id, error),
    }
}

fn open_worker_connection(db_path: &std::path::Path) -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
    // Other connections (UI, NLP, LLM workers and sibling OCR jobs) write to
    // the same DB; wait for locks instead of failing with SQLITE_BUSY.
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    Ok(conn)
}

/// Tracks assets with an OCR job currently in flight so the same asset is
/// never processed by two concurrent jobs: extractions/layouts are keyed by
/// `asset_id`, and racing jobs could interleave progress/complete events.
#[derive(Default)]
struct InFlightAssets {
    assets: Mutex<HashSet<String>>,
    released: Notify,
}

impl InFlightAssets {
    /// Try to mark `asset_id` as in flight. Returns a token that releases the
    /// asset on drop, or `None` if a job for the asset is already running.
    fn try_begin(self: &Arc<Self>, asset_id: &str) -> Option<InFlightToken> {
        let mut assets = self.assets.lock().expect("in-flight asset set poisoned");
        assets.insert(asset_id.to_string()).then(|| InFlightToken {
            registry: Arc::clone(self),
            asset_id: asset_id.to_string(),
        })
    }

    /// Mark `asset_id` as in flight, waiting until any previous job for the
    /// same asset releases its token.
    async fn begin(self: &Arc<Self>, asset_id: &str) -> InFlightToken {
        loop {
            let released = self.released.notified();
            tokio::pin!(released);
            // Register for the release notification BEFORE checking the set,
            // so a token dropped between the check and the `.await` below
            // still wakes this waiter.
            released.as_mut().enable();
            if let Some(token) = self.try_begin(asset_id) {
                return token;
            }
            released.await;
        }
    }

    fn finish(&self, asset_id: &str) {
        let mut assets = self.assets.lock().expect("in-flight asset set poisoned");
        assets.remove(asset_id);
        drop(assets);
        self.released.notify_waiters();
    }
}

/// RAII guard for an in-flight asset: releasing on drop keeps the set correct
/// even if a job task panics or is cancelled.
struct InFlightToken {
    registry: Arc<InFlightAssets>,
    asset_id: String,
}

impl Drop for InFlightToken {
    fn drop(&mut self) {
        self.registry.finish(&self.asset_id);
    }
}

async fn complete_job(
    conn: rusqlite::Connection,
    app_handle: &AppHandle,
    asset_id: &str,
    output: ProcessedOcrOutput,
) {
    let method = output.ocr.method.clone();
    let text_content = output.ocr.text.clone();

    // Persistence and the item lookup are blocking rusqlite calls; run them
    // on the blocking pool. `ocr:complete` is emitted only after the
    // persistence transaction commits — that ordering contract is tested.
    let persist_asset_id = asset_id.to_string();
    let persist_text = text_content.clone();
    let persist_method = method.clone();
    let layout = output.layout;
    let persisted = tokio::task::spawn_blocking(move || {
        let mut conn = conn;
        persist_output(
            &mut conn,
            &persist_asset_id,
            &persist_text,
            &persist_method,
            layout.as_ref(),
        )?;
        Ok::<_, String>(lookup_item_id_for_asset(&conn, &persist_asset_id))
    })
    .await
    .map_err(|e| format!("OCR persistence task panicked: {e}"))
    .and_then(|result| result);

    let item_lookup = match persisted {
        Ok(lookup) => lookup,
        Err(e) => {
            crate::app_logs::error(
                app_handle,
                "ocr",
                format!("No se pudo guardar extracción de {asset_id}: {e}"),
            );
            emit_error(
                app_handle,
                asset_id.to_string(),
                format!("No se pudo guardar la extracción en la base de datos: {e}"),
            );
            return;
        }
    };

    match item_lookup {
        Ok(Some(item_id)) => {
            let nlp_queue = app_handle.state::<NlpQueue>();
            let _ = nlp_queue.submit(NlpJob::IndexFts {
                item_id: item_id.clone(),
            });
            let _ = nlp_queue.submit(NlpJob::ComputeAssetEmbedding {
                item_id,
                asset_id: asset_id.to_string(),
            });
        }
        Ok(None) => {}
        // The extraction is already committed; a failed lookup only skips NLP
        // indexing, so log it without reporting the OCR job as failed.
        Err(e) => crate::app_logs::error(
            app_handle,
            "ocr",
            format!("No se pudo resolver el item del asset {asset_id}: {e}"),
        ),
    }

    let _ = app_handle.emit(
        "ocr:complete",
        OcrCompletePayload {
            asset_id: asset_id.to_string(),
            method,
            text_length: text_content.len(),
            text_content,
        },
    );
}

/// Persist the OCR extraction and its layout atomically: both writes commit in
/// a single transaction so a failure cannot leave new extraction text alongside
/// a stale layout (or vice versa).
fn persist_output(
    conn: &mut rusqlite::Connection,
    asset_id: &str,
    text_content: &str,
    method: &str,
    layout: Option<&LayoutPersistencePayload>,
) -> Result<(), String> {
    let tx = conn
        .transaction()
        .map_err(|e| format!("Failed to start OCR persistence transaction: {e}"))?;

    save_extraction(&tx, asset_id, text_content, method)?;
    match layout {
        Some(layout) => save_layout(&tx, asset_id, layout)?,
        None => delete_layout(&tx, asset_id)?,
    }

    tx.commit()
        .map_err(|e| format!("Failed to commit OCR persistence transaction: {e}"))
}

fn emit_error(app_handle: &AppHandle, asset_id: String, error: String) {
    let _ = app_handle.emit("ocr:error", OcrErrorPayload { asset_id, error });
}

fn get_glm_ocr_api_key(conn: &rusqlite::Connection) -> String {
    crate::settings::get_setting(conn, OCRH_SETTING_GLM_OCR_API_KEY)
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub(super) fn ensure_selected_cloud_key(conn: &rusqlite::Connection) -> Result<(), String> {
    if get_glm_ocr_api_key(conn).is_empty() {
        return Err(
            "EntropIA Lite requiere GLM-OCR remoto. Andá a Configuración > OCRH y cargá una API key antes de usar OCR."
                .to_string(),
        );
    }

    Ok(())
}

async fn process_job(
    api_key: &str,
    job: &OcrJob,
    app_handle: &AppHandle,
) -> Result<ProcessedOcrOutput, String> {
    let _ = &job.mode;
    emit_progress(app_handle, &job.asset_id, 25, "reading");
    let bytes = tokio::fs::read(&job.asset_path)
        .await
        .map_err(|e| format!("Failed to read {}: {e}", job.asset_path))?;
    if api_key.is_empty() {
        return Err("EntropIA Lite requiere GLM-OCR remoto para OCR.".to_string());
    }

    let method = if job.asset_type == "pdf" {
        "pdf_glm_ocr"
    } else {
        "glm_ocr"
    };
    process_with_glm_ocr_provider(&bytes, &job.asset_id, app_handle, api_key, method).await
}

fn encode_bytes_for_glm_ocr(bytes: &[u8]) -> Result<String, String> {
    let mime = if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else {
        match image::guess_format(bytes)
            .map_err(|e| format!("No pude detectar el formato de la imagen para GLM-OCR: {e}"))?
        {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            other => {
                return Err(format!(
                    "GLM-OCR sólo acepta PDF, PNG o JPG/JPEG. Formato detectado no soportado: {other:?}"
                ))
            }
        }
    };

    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

async fn process_with_glm_ocr_provider(
    bytes: &[u8],
    asset_id: &str,
    app_handle: &AppHandle,
    api_key: &str,
    method: &str,
) -> Result<ProcessedOcrOutput, String> {
    emit_progress(app_handle, asset_id, 55, "submitting_glm_ocr");
    let payload = encode_bytes_for_glm_ocr(bytes)?;
    let client = glm_ocr::GlmOcrClient::new(api_key.to_string());
    emit_progress(app_handle, asset_id, 75, "waiting_glm_ocr");
    let response = client.parse_file(&payload).await?;

    #[cfg(debug_assertions)]
    {
        let _ = debug_viz::save_glm_ocr_response_debug(&response, method, asset_id);
    }

    if !glm_response_has_useful_content(&response) {
        return Err("GLM-OCR devolvió una respuesta vacía para este asset.".to_string());
    }

    emit_progress(app_handle, asset_id, 92, "parsing_glm_ocr");
    let output = glm_response_to_processed_output(&response, method)?;
    emit_progress(app_handle, asset_id, 100, "done");
    Ok(output)
}

fn glm_label_to_layout_category(label: &str) -> Option<LayoutCategory> {
    match label {
        "title" => Some(LayoutCategory::Title),
        "text" => Some(LayoutCategory::PlainText),
        "table" => Some(LayoutCategory::Table),
        "image" => Some(LayoutCategory::Figure),
        "formula" => Some(LayoutCategory::PlainText),
        _ => None,
    }
}

fn strip_html_tags(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut inside_tag = false;

    for ch in value.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => {
                inside_tag = false;
                result.push(' ');
            }
            _ if !inside_tag => result.push(ch),
            _ => {}
        }
    }

    result
}

fn normalize_glm_text_fragment(value: &str) -> String {
    strip_html_tags(value)
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .replace("<br />", " ")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

fn collect_glm_markdown_titles(markdown: &str) -> std::collections::HashSet<String> {
    let mut titles = std::collections::HashSet::new();
    let mut inside_centered_block = false;
    let mut centered_block_has_heading = false;
    let mut centered_lines: Vec<String> = Vec::new();

    let flush_centered_block = |titles: &mut std::collections::HashSet<String>,
                                centered_block_has_heading: bool,
                                centered_lines: &mut Vec<String>| {
        if centered_block_has_heading {
            for line in centered_lines.iter() {
                let normalized = normalize_glm_text_fragment(line);
                if !normalized.is_empty() {
                    titles.insert(normalized);
                }
            }
        }
        centered_lines.clear();
    };

    for raw_line in markdown.lines() {
        let line = raw_line.trim();
        let line_without_html = strip_html_tags(line);
        let normalized_line = line_without_html.trim();

        if line.starts_with("<div") && line.contains("align=\"center\"") {
            inside_centered_block = true;
            centered_block_has_heading = false;
            centered_lines.clear();
            continue;
        }

        if inside_centered_block {
            if line.starts_with("</div") {
                flush_centered_block(&mut titles, centered_block_has_heading, &mut centered_lines);
                inside_centered_block = false;
                centered_block_has_heading = false;
                continue;
            }

            if normalized_line.starts_with('#') {
                centered_block_has_heading = true;
            }

            let cleaned = normalized_line.trim_start_matches('#').trim();
            if !cleaned.is_empty() {
                centered_lines.push(cleaned.to_string());
            }
            continue;
        }

        if normalized_line.starts_with('#') {
            let title = normalized_line.trim_start_matches('#').trim();
            if !title.is_empty() {
                titles.insert(normalize_glm_text_fragment(title));
            }
        }
    }

    if inside_centered_block {
        flush_centered_block(&mut titles, centered_block_has_heading, &mut centered_lines);
    }

    titles
}

fn resolve_glm_effective_label(
    raw_label: &str,
    content: &str,
    markdown_titles: &std::collections::HashSet<String>,
) -> String {
    if raw_label == "text" {
        let normalized_content =
            normalize_glm_text_fragment(content.trim_start_matches('#').trim());
        if !normalized_content.is_empty() && markdown_titles.contains(&normalized_content) {
            return "title".to_string();
        }
        if content.trim_start().starts_with('#') {
            return "title".to_string();
        }
    }

    raw_label.to_string()
}

fn page_dimensions_from_glm_response(response: &GlmOcrResponse, page_index: usize) -> (u32, u32) {
    response
        .data_info
        .as_ref()
        .and_then(|info| info.pages.get(page_index))
        .map(|page| (page.width, page.height))
        .unwrap_or((0, 0))
}

fn normalized_bbox_to_pixels(
    detail: &GlmOcrLayoutDetail,
    fallback_width: u32,
    fallback_height: u32,
) -> Option<LayoutBbox> {
    if detail.bbox_2d.len() != 4 {
        return None;
    }

    let width = detail.width.unwrap_or(fallback_width);
    let height = detail.height.unwrap_or(fallback_height);
    if width == 0 || height == 0 {
        return None;
    }

    let raw_x1 = detail.bbox_2d[0];
    let raw_y1 = detail.bbox_2d[1];
    let raw_3 = detail.bbox_2d[2];
    let raw_4 = detail.bbox_2d[3];
    let looks_normalized = [raw_x1, raw_y1, raw_3, raw_4]
        .iter()
        .all(|value| *value >= 0.0 && *value <= 1.0);

    let (x1, y1, x2, y2) = if looks_normalized {
        let norm_x1 = raw_x1.clamp(0.0, 1.0);
        let norm_y1 = raw_y1.clamp(0.0, 1.0);
        let norm_3 = raw_3.clamp(0.0, 1.0);
        let norm_4 = raw_4.clamp(0.0, 1.0);
        let x1 = (norm_x1 * width as f32).round() as i32;
        let y1 = (norm_y1 * height as f32).round() as i32;

        if norm_3 > norm_x1 && norm_4 > norm_y1 {
            (
                x1,
                y1,
                (norm_3 * width as f32).round() as i32,
                (norm_4 * height as f32).round() as i32,
            )
        } else {
            (
                x1,
                y1,
                ((norm_x1 + norm_3).clamp(0.0, 1.0) * width as f32).round() as i32,
                ((norm_y1 + norm_4).clamp(0.0, 1.0) * height as f32).round() as i32,
            )
        }
    } else {
        let x1 = raw_x1.round() as i32;
        let y1 = raw_y1.round() as i32;
        if raw_3 > raw_x1 && raw_4 > raw_y1 {
            (x1, y1, raw_3.round() as i32, raw_4.round() as i32)
        } else {
            (
                x1,
                y1,
                (raw_x1 + raw_3).round() as i32,
                (raw_y1 + raw_4).round() as i32,
            )
        }
    };

    Some(LayoutBbox {
        x: x1,
        y: y1,
        width: (x2 - x1).max(0),
        height: (y2 - y1).max(0),
    })
}

fn glm_response_has_useful_content(response: &GlmOcrResponse) -> bool {
    if !response.md_results.trim().is_empty() {
        return true;
    }

    response.layout_details.iter().flatten().any(|detail| {
        let label = detail.label.as_deref().unwrap_or_default();
        let content = detail.content.as_deref().unwrap_or_default().trim();
        !content.is_empty() && matches!(label, "text" | "table" | "formula")
    })
}

fn glm_response_to_processed_output(
    response: &GlmOcrResponse,
    method: &str,
) -> Result<ProcessedOcrOutput, String> {
    let mut blocks = Vec::new();
    let mut regions = Vec::new();
    let mut ocr_regions = Vec::new();
    let mut max_width = 0_u32;
    let mut max_height = 0_u32;
    let markdown_titles = collect_glm_markdown_titles(&response.md_results);

    for (page_idx, page_details) in response.layout_details.iter().enumerate() {
        let page =
            u32::try_from(page_idx + 1).map_err(|_| "GLM-OCR page index overflow".to_string())?;
        let (fallback_width, fallback_height) =
            page_dimensions_from_glm_response(response, page_idx);

        for detail in page_details {
            let raw_label = detail.label.as_deref().unwrap_or("text");
            let content = detail.content.clone().unwrap_or_default();
            let trimmed = content.trim();
            let width = detail.width.unwrap_or(fallback_width);
            let height = detail.height.unwrap_or(fallback_height);
            let bbox = normalized_bbox_to_pixels(detail, fallback_width, fallback_height);
            max_width = max_width.max(width);
            max_height = max_height.max(height);
            let effective_label = resolve_glm_effective_label(raw_label, trimmed, &markdown_titles);

            let Some(mapped_category) = glm_label_to_layout_category(&effective_label) else {
                continue;
            };

            if let (Some(formatted_text), Some(bbox)) = (
                format_region_text(&mapped_category, &content),
                bbox.as_ref(),
            ) {
                ocr_regions.push(provider::OcrRegion {
                    text: formatted_text,
                    confidence: 1.0,
                    bbox: Some(provider::BoundingBox {
                        x: bbox.x,
                        y: bbox.y,
                        width: bbox.width as u32,
                        height: bbox.height as u32,
                    }),
                    column: None,
                });
            }

            if let Some(bbox) = bbox {
                let order = detail.index.unwrap_or((blocks.len() + 1) as i32);
                regions.push(PersistedLayoutRegion {
                    page,
                    image_width: width,
                    image_height: height,
                    category: effective_label.clone(),
                    bbox: bbox.clone(),
                    confidence: 1.0,
                });
                blocks.push(PersistedLayoutBlock {
                    page,
                    image_width: width,
                    image_height: height,
                    label: effective_label,
                    content: trimmed.to_string(),
                    bbox,
                    order,
                    group_id: page as i32,
                });
            }
        }
    }

    blocks.sort_by_key(|block| (block.page, block.order));
    let text = if !response.md_results.trim().is_empty() {
        response.md_results.trim().to_string()
    } else {
        blocks
            .iter()
            .filter_map(|block| {
                glm_label_to_layout_category(block.label.as_str())
                    .and_then(|category| format_region_text(&category, &block.content))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    Ok(ProcessedOcrOutput {
        ocr: provider::OcrOutput {
            text,
            regions: ocr_regions,
            method: method.to_string(),
        },
        layout: (!blocks.is_empty()).then_some(LayoutPersistencePayload {
            model: method.to_string(),
            image_width: max_width,
            image_height: max_height,
            regions,
            blocks,
        }),
    })
}

fn save_extraction(
    conn: &rusqlite::Connection,
    asset_id: &str,
    text_content: &str,
    method: &str,
) -> Result<(), String> {
    // Deterministic id (DESIGN §4.6): one extraction per asset, so derive the id
    // from asset_id to converge duplicates across devices.
    let id = format!("ext-{asset_id}");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO extractions(id, asset_id, text_content, method, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(asset_id) DO UPDATE SET
           id = excluded.id,
           text_content = excluded.text_content,
           method = excluded.method,
           confidence = excluded.confidence,
           created_at = excluded.created_at",
        rusqlite::params![id, asset_id, text_content, method, None::<f64>, now],
    )
    .map_err(|e| format!("Failed to upsert extraction: {e}"))?;

    Ok(())
}

fn save_layout(
    conn: &rusqlite::Connection,
    asset_id: &str,
    layout: &LayoutPersistencePayload,
) -> Result<(), String> {
    // Deterministic id (DESIGN §4.6): one layout per asset.
    let id = format!("lay-{asset_id}");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let regions_json = serde_json::to_string(&layout.regions)
        .map_err(|e| format!("Failed to serialize layout regions: {e}"))?;
    let blocks_json = serde_json::to_string(&layout.blocks)
        .map_err(|e| format!("Failed to serialize layout blocks: {e}"))?;

    conn.execute(
        "INSERT INTO layouts(id, asset_id, regions, blocks, model, image_width, image_height, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(asset_id) DO UPDATE SET
           id = excluded.id,
           regions = excluded.regions,
           blocks = excluded.blocks,
           model = excluded.model,
           image_width = excluded.image_width,
           image_height = excluded.image_height,
           created_at = excluded.created_at",
        rusqlite::params![
            id,
            asset_id,
            regions_json,
            blocks_json,
            layout.model,
            layout.image_width,
            layout.image_height,
            now,
        ],
    )
    .map_err(|e| format!("Failed to upsert layout: {e}"))?;

    Ok(())
}

fn delete_layout(conn: &rusqlite::Connection, asset_id: &str) -> Result<(), String> {
    conn.execute(
        "DELETE FROM layouts WHERE asset_id = ?1",
        rusqlite::params![asset_id],
    )
    .map_err(|e| format!("Failed to delete stale layout: {e}"))?;
    Ok(())
}

fn update_extraction_text(
    conn: &rusqlite::Connection,
    asset_id: &str,
    text_content: &str,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT id FROM extractions WHERE asset_id = ?1 ORDER BY created_at DESC LIMIT 1")
        .map_err(|e| format!("Failed to prepare query: {e}"))?;
    let extraction_id: Result<String, _> = stmt.query_row([asset_id], |row| row.get(0));
    drop(stmt);

    match extraction_id {
        Ok(id) => {
            conn.execute(
                "UPDATE extractions SET text_content = ?1 WHERE id = ?2",
                rusqlite::params![text_content, id],
            )
            .map_err(|e| format!("Failed to update extraction text: {e}"))?;
            Ok(())
        }
        Err(_) => Ok(()),
    }
}

#[allow(dead_code)]
fn format_region_text(category: &LayoutCategory, text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    match category {
        LayoutCategory::Title => Some(format!("## {trimmed}")),
        LayoutCategory::PlainText => Some(trimmed.to_string()),
        LayoutCategory::Table => Some(format!("---\n{trimmed}\n---")),
        LayoutCategory::Figure => None,
        LayoutCategory::Caption => Some(trimmed.to_string()),
        LayoutCategory::Footnote => Some(format!("Note: {trimmed}")),
        LayoutCategory::Header => None,
        LayoutCategory::Footer => None,
        LayoutCategory::Code => Some(format!("```\n{trimmed}\n```")),
        LayoutCategory::Reference => Some(trimmed.to_string()),
        LayoutCategory::Abandoned => None,
    }
}

fn emit_progress(app_handle: &AppHandle, asset_id: &str, pct: u8, stage: &str) {
    let _ = app_handle.emit(
        "ocr:progress",
        OcrProgressPayload {
            asset_id: asset_id.to_string(),
            pct,
            stage: stage.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glm_bbox_conversion_supports_xywh_shape() {
        let bbox = normalized_bbox_to_pixels(
            &GlmOcrLayoutDetail {
                index: Some(1),
                label: Some("text".to_string()),
                bbox_2d: vec![0.1, 0.2, 0.05, 0.04],
                content: Some("Texto".to_string()),
                height: Some(1000),
                width: Some(800),
            },
            800,
            1000,
        )
        .expect("bbox");

        assert_eq!(bbox.x, 80);
        assert_eq!(bbox.y, 200);
        assert_eq!(bbox.width, 40);
        assert_eq!(bbox.height, 40);
    }

    #[test]
    fn test_glm_response_to_processed_output_uses_markdown_and_maps_layout() {
        let response = GlmOcrResponse {
            id: Some("task-1".to_string()),
            created: Some(1),
            model: Some("GLM-OCR".to_string()),
            md_results: "# Título\n\nTexto".to_string(),
            layout_details: vec![vec![GlmOcrLayoutDetail {
                index: Some(1),
                label: Some("text".to_string()),
                bbox_2d: vec![0.1, 0.2, 0.5, 0.4],
                content: Some("Título".to_string()),
                height: Some(1000),
                width: Some(800),
            }]],
            data_info: None,
            request_id: Some("req-1".to_string()),
        };

        let output = glm_response_to_processed_output(&response, "glm_ocr").expect("glm output");
        let layout = output.layout.as_ref().expect("layout");

        assert_eq!(output.ocr.text, "# Título\n\nTexto");
        assert_eq!(output.ocr.method, "glm_ocr");
        assert_eq!(layout.blocks[0].label, "title");
        assert_eq!(layout.image_width, 800);
        assert_eq!(layout.image_height, 1000);
    }

    fn open_extractions_only_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory DB");
        conn.execute_batch(
            "CREATE TABLE extractions (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL UNIQUE,
                text_content TEXT NOT NULL,
                method TEXT NOT NULL,
                confidence REAL,
                created_at INTEGER NOT NULL
            );",
        )
        .expect("create extractions table");
        conn
    }

    fn sample_layout() -> LayoutPersistencePayload {
        LayoutPersistencePayload {
            model: "glm_ocr".to_string(),
            image_width: 800,
            image_height: 1000,
            regions: Vec::new(),
            blocks: Vec::new(),
        }
    }

    #[test]
    fn test_persist_output_saves_extraction_and_layout() {
        let mut conn = open_extractions_only_db();
        conn.execute_batch(
            "CREATE TABLE layouts (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL UNIQUE,
                regions TEXT NOT NULL,
                blocks TEXT NOT NULL,
                model TEXT NOT NULL,
                image_width INTEGER NOT NULL,
                image_height INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );",
        )
        .expect("create layouts table");

        let layout = sample_layout();
        persist_output(&mut conn, "asset-1", "texto", "glm_ocr", Some(&layout))
            .expect("persist output");

        let extractions: i64 = conn
            .query_row("SELECT COUNT(*) FROM extractions", [], |row| row.get(0))
            .expect("count extractions");
        let layouts: i64 = conn
            .query_row("SELECT COUNT(*) FROM layouts", [], |row| row.get(0))
            .expect("count layouts");
        assert_eq!(extractions, 1);
        assert_eq!(layouts, 1);

        // Deterministic ids (DESIGN §4.6).
        let extraction_id: String = conn
            .query_row(
                "SELECT id FROM extractions WHERE asset_id = 'asset-1'",
                [],
                |row| row.get(0),
            )
            .expect("read extraction id");
        let layout_id: String = conn
            .query_row(
                "SELECT id FROM layouts WHERE asset_id = 'asset-1'",
                [],
                |row| row.get(0),
            )
            .expect("read layout id");
        assert_eq!(extraction_id, "ext-asset-1");
        assert_eq!(layout_id, "lay-asset-1");
    }

    #[test]
    fn save_extraction_id_converges_on_re_ocr() {
        // A re-OCR (UPSERT by asset_id) must keep the deterministic id even if a
        // legacy UUID row was present first (DESIGN §4.6: id = excluded.id).
        let conn = open_extractions_only_db();
        conn.execute(
            "INSERT INTO extractions(id, asset_id, text_content, method, confidence, created_at)
             VALUES ('legacy-uuid', 'asset-9', 'old', 'native', NULL, 1)",
            [],
        )
        .expect("seed legacy row");

        save_extraction(&conn, "asset-9", "nuevo", "glm_ocr").expect("re-ocr upsert");

        let id: String = conn
            .query_row(
                "SELECT id FROM extractions WHERE asset_id = 'asset-9'",
                [],
                |row| row.get(0),
            )
            .expect("read converged id");
        assert_eq!(id, "ext-asset-9");
    }

    #[test]
    fn test_persist_output_rolls_back_extraction_when_layout_save_fails() {
        // No layouts table → save_layout fails after save_extraction succeeded.
        let mut conn = open_extractions_only_db();

        let layout = sample_layout();
        let result = persist_output(&mut conn, "asset-1", "texto", "glm_ocr", Some(&layout));
        assert!(result.is_err(), "expected layout save failure");

        let extractions: i64 = conn
            .query_row("SELECT COUNT(*) FROM extractions", [], |row| row.get(0))
            .expect("count extractions");
        assert_eq!(
            extractions, 0,
            "extraction must roll back when layout save fails"
        );
    }

    #[test]
    fn in_flight_try_begin_blocks_duplicates_until_release() {
        let registry = Arc::new(InFlightAssets::default());

        let token = registry.try_begin("asset-1").expect("first begin succeeds");
        assert!(
            registry.try_begin("asset-1").is_none(),
            "same asset must not run concurrently"
        );
        assert!(
            registry.try_begin("asset-2").is_some(),
            "other assets are unaffected"
        );

        drop(token);
        assert!(
            registry.try_begin("asset-1").is_some(),
            "asset is available again after the token is released"
        );
    }

    #[tokio::test]
    async fn in_flight_begin_waits_for_previous_job_to_release() {
        let registry = Arc::new(InFlightAssets::default());
        let first = registry.begin("asset-1").await;

        let waiter_registry = Arc::clone(&registry);
        let mut waiter = tokio::spawn(async move {
            let _token = waiter_registry.begin("asset-1").await;
        });

        let still_waiting =
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut waiter).await;
        assert!(
            still_waiting.is_err(),
            "a duplicate-asset job must wait while the first is in flight"
        );

        drop(first);
        tokio::time::timeout(std::time::Duration::from_secs(5), waiter)
            .await
            .expect("duplicate job should start once the first releases")
            .expect("waiter task should not panic");
    }

    #[tokio::test]
    async fn pending_duplicate_does_not_starve_other_assets() {
        // Mirrors the job-task acquisition order (per-asset gate FIRST, then
        // a concurrency permit): a duplicate parked on the gate must hold no
        // permit, so jobs for unrelated assets keep flowing.
        let semaphore = Arc::new(Semaphore::new(2));
        let registry = Arc::new(InFlightAssets::default());

        // Job 1: asset-1 in flight, holding its gate and one permit.
        let first_token = registry.begin("asset-1").await;
        let first_permit = Arc::clone(&semaphore)
            .acquire_owned()
            .await
            .expect("semaphore open");

        // Job 2: duplicate of asset-1 — parks on the gate, holds NO permit.
        let dup_registry = Arc::clone(&registry);
        let dup_semaphore = Arc::clone(&semaphore);
        let mut duplicate = tokio::spawn(async move {
            let _token = dup_registry.begin("asset-1").await;
            let _permit = dup_semaphore.acquire_owned().await.expect("semaphore open");
        });
        let parked =
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut duplicate).await;
        assert!(
            parked.is_err(),
            "duplicate must wait while asset-1 is in flight"
        );

        // Job 3: a DIFFERENT asset must acquire its gate and a permit even
        // though a duplicate is pending (the old dispatcher blocked here).
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let _token = registry.begin("asset-2").await;
            let _permit = Arc::clone(&semaphore)
                .acquire_owned()
                .await
                .expect("semaphore open");
        })
        .await
        .expect("a pending duplicate must not starve other assets");

        // Once job 1 releases, the duplicate proceeds.
        drop(first_permit);
        drop(first_token);
        tokio::time::timeout(std::time::Duration::from_secs(5), duplicate)
            .await
            .expect("duplicate should run once the first job releases")
            .expect("duplicate task should not panic");
    }

    #[test]
    fn test_glm_response_has_useful_content_detects_empty_responses() {
        let empty = GlmOcrResponse {
            id: None,
            created: None,
            model: None,
            md_results: "   ".to_string(),
            layout_details: vec![vec![GlmOcrLayoutDetail {
                index: Some(1),
                label: Some("image".to_string()),
                bbox_2d: vec![0.0, 0.0, 1.0, 1.0],
                content: Some("https://example.com/image.png".to_string()),
                height: Some(10),
                width: Some(10),
            }]],
            data_info: None,
            request_id: None,
        };

        assert!(!glm_response_has_useful_content(&empty));
    }
}
