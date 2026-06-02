pub mod commands;
pub mod glm_ocr;
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
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

const OCRH_SETTING_GLM_OCR_API_KEY: &str = "glm_ocr_api_key";

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
        std::thread::Builder::new()
            .name("ocr-worker".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                pdf::init_pdfium_path(&app_handle);
                let conn = match rusqlite::Connection::open(&db_path) {
                    Ok(c) => {
                        let _ = c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
                        c
                    }
                    Err(e) => {
                        eprintln!("[OCR] Failed to open worker DB connection: {e}");
                        while let Some(job) = receiver.blocking_recv() {
                            emit_error(
                                &app_handle,
                                job.asset_id,
                                format!("Failed to open OCR DB connection: {e}"),
                            );
                        }
                        return;
                    }
                };

                eprintln!("[OCR] EntropIA Lite OCR worker ready; GLM-OCR remote only");
                while let Some(job) = receiver.blocking_recv() {
                    let asset_id = job.asset_id.clone();
                    let result =
                        tauri::async_runtime::block_on(process_job(&conn, &job, &app_handle));

                    match result {
                        Ok(output) => complete_job(&conn, &app_handle, &asset_id, output),
                        Err(error) => emit_error(&app_handle, asset_id, error),
                    }
                }
            })
            .expect("Failed to spawn OCR worker thread");
    }
}

fn complete_job(
    conn: &rusqlite::Connection,
    app_handle: &AppHandle,
    asset_id: &str,
    output: ProcessedOcrOutput,
) {
    let method = output.ocr.method.clone();
    let text_content = output.ocr.text.clone();
    let save_result = save_extraction(conn, asset_id, &text_content, &method)
        .and_then(|_| match output.layout.as_ref() {
            Some(layout) => save_layout(conn, asset_id, layout),
            None => delete_layout(conn, asset_id),
        })
        .and_then(|_| lookup_item_id_for_asset(conn, asset_id));

    if let Err(e) = &save_result {
        crate::app_logs::error(
            app_handle,
            "ocr",
            format!("No se pudo guardar extracción de {asset_id}: {e}"),
        );
    } else if let Ok(Some(item_id)) = &save_result {
        let nlp_queue = app_handle.state::<NlpQueue>();
        let _ = nlp_queue.submit(NlpJob::ExtractEntitiesForAsset {
            item_id: item_id.clone(),
            asset_id: asset_id.to_string(),
        });
        let _ = nlp_queue.submit(NlpJob::IndexFts {
            item_id: item_id.clone(),
        });
        let _ = nlp_queue.submit(NlpJob::ComputeAssetEmbedding {
            item_id: item_id.clone(),
            asset_id: asset_id.to_string(),
        });
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
    conn: &rusqlite::Connection,
    job: &OcrJob,
    app_handle: &AppHandle,
) -> Result<ProcessedOcrOutput, String> {
    let _ = &job.mode;
    emit_progress(app_handle, &job.asset_id, 25, "reading");
    let bytes = tokio::fs::read(&job.asset_path)
        .await
        .map_err(|e| format!("Failed to read {}: {e}", job.asset_path))?;
    let api_key = get_glm_ocr_api_key(conn);
    if api_key.is_empty() {
        return Err("EntropIA Lite requiere GLM-OCR remoto para OCR.".to_string());
    }

    let method = if job.asset_type == "pdf" {
        "pdf_glm_ocr"
    } else {
        "glm_ocr"
    };
    process_with_glm_ocr_provider(&bytes, &job.asset_id, app_handle, &api_key, method).await
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

            if let (Some(formatted_text), Some(ref bbox)) = (
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
    let id = uuid::Uuid::new_v4().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO extractions(id, asset_id, text_content, method, confidence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(asset_id) DO UPDATE SET
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
    let id = uuid::Uuid::new_v4().to_string();
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
