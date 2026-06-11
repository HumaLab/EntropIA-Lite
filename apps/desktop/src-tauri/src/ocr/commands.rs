/// Tauri IPC commands for OCR operations.
use super::{update_extraction_text, OcrQueue};
use crate::db::state::AppDbState;
use crate::nlp::NlpQueue;
use crate::path_utils::normalize_windows_path_string;
use serde::Serialize;
use tauri::{AppHandle, State};

/// A single rendered PDF page returned by `render_pdf_pages_cmd`.
#[derive(Clone, Serialize)]
pub struct RenderedPage {
    pub page_number: u32,
    pub png_path: String,
}

/// Submit an OCR extraction job to the background worker queue.
///
/// Returns immediately with `Ok("queued")`. The worker will process the job
/// asynchronously and emit `ocr:progress`, `ocr:complete`, or `ocr:error` events.
///
/// # Arguments
/// * `asset_id`   — unique ID of the asset in the database
/// * `asset_path` — absolute filesystem path to the asset file
/// * `asset_type` — `"pdf"` or `"image"`
/// * `mode`       — accepted for UI compatibility; Lite always uses remote GLM-OCR
/// * `ocr_queue`  — managed state injected by Tauri
#[tauri::command]
pub async fn extract_text(
    asset_id: String,
    asset_path: String,
    asset_type: String,
    mode: Option<String>,
    app_handle: AppHandle,
    ocr_queue: State<'_, OcrQueue>,
    db: State<'_, AppDbState>,
) -> Result<String, String> {
    let _ = mode;
    let ocr_mode = super::OcrMode::High;
    let conn = db
        .ui_conn
        .lock()
        .map_err(|e| format!("DB lock poisoned: {e}"))?;
    super::ensure_selected_cloud_key(&conn)?;
    drop(conn);

    crate::app_logs::info(
        &app_handle,
        "ocr",
        format!("Trabajo OCR encolado: asset_id={asset_id}, tipo={asset_type}, modo={ocr_mode:?}"),
    );

    let job = super::OcrJob {
        asset_id,
        asset_path,
        asset_type,
        mode: ocr_mode,
    };

    ocr_queue.submit(job)?;
    Ok("queued".to_string())
}

#[tauri::command]
pub async fn test_glm_ocr_connection(
    api_key: String,
    app_handle: AppHandle,
    db: State<'_, AppDbState>,
) -> Result<(), String> {
    let api_key = if api_key.trim().is_empty() {
        let conn = db
            .ui_conn
            .lock()
            .map_err(|e| format!("DB lock poisoned: {e}"))?;
        crate::settings::get_secret_setting(&conn, super::OCRH_SETTING_GLM_OCR_API_KEY)
            .unwrap_or_default()
    } else {
        api_key
    };

    let result = super::glm_ocr::GlmOcrClient::new(api_key.trim().to_string())
        .test_connection()
        .await;
    match &result {
        Ok(()) => crate::app_logs::info(
            &app_handle,
            "settings/glm_ocr",
            "Conexión GLM OCR verificada",
        ),
        Err(error) => crate::app_logs::error(
            &app_handle,
            "settings/glm_ocr",
            format!("Falló prueba de conexión GLM OCR: {error}"),
        ),
    }
    result
}

/// Update the text_content of the latest extraction for an asset.
///
/// This allows users to manually correct OCR output and persist the correction.
/// The original extraction metadata (id, created_at, method, confidence) is preserved.
#[tauri::command]
pub async fn update_extraction_text_cmd(
    asset_id: String,
    text_content: String,
    db: State<'_, AppDbState>,
    _nlp_queue: State<'_, NlpQueue>,
) -> Result<(), String> {
    let conn = db
        .ui_conn
        .lock()
        .map_err(|e| format!("DB lock poisoned: {e}"))?;
    update_extraction_text(&conn, &asset_id, &text_content)?;

    Ok(())
}

/// Generate a thumbnail PNG for the first page of a PDF.
///
/// Returns the filesystem path to the cached thumbnail. The frontend should
/// use `convertFileSrc()` to turn this path into a webview-accessible URL.
///
/// Thumbnails are cached at `{app_data_dir}/thumbnails/{asset_id}.png`.
/// If a cached thumbnail already exists, the cached path is returned immediately
/// without re-rendering.
#[tauri::command]
pub async fn generate_pdf_thumbnail(
    asset_path: String,
    asset_id: String,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    use std::io::Write;
    use tauri::Manager;

    // Ensure Pdfium DLL path is initialized before any PDF operations.
    // This is a no-op if already called by the OCR worker; safe to call multiple times.
    super::pdf::init_pdfium_path(&app_handle);

    // Resolve thumbnails directory
    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;

    let thumb_dir = app_dir.join("thumbnails");
    std::fs::create_dir_all(&thumb_dir)
        .map_err(|e| format!("Failed to create thumbnails directory: {e}"))?;

    let thumb_path = thumb_dir.join(format!("{asset_id}.png"));

    // Return cached thumbnail immediately if it exists
    if thumb_path.exists() {
        return Ok(normalize_windows_path_string(&thumb_path));
    }

    // Read PDF and render thumbnail in a blocking task
    // (pdfium is CPU-intensive and must not block the async runtime)
    let result_path = tokio::task::spawn_blocking(move || {
        let bytes =
            std::fs::read(&asset_path).map_err(|e| format!("Failed to read PDF file: {e}"))?;

        let png_data = super::pdf::render_pdf_thumbnail(&bytes)?;

        // Write thumbnail to disk
        let mut file = std::fs::File::create(&thumb_path)
            .map_err(|e| format!("Failed to create thumbnail file: {e}"))?;
        file.write_all(&png_data)
            .map_err(|e| format!("Failed to write thumbnail data: {e}"))?;

        Ok::<String, String>(normalize_windows_path_string(&thumb_path))
    })
    .await
    .map_err(|e| format!("Thumbnail generation task panicked: {e}"))??;

    Ok(result_path)
}

/// Generate or retrieve a cached bounded thumbnail for an image asset.
///
/// The path hash is part of the filename so edited assets that receive a new
/// file path do not reuse stale thumbnails for the same asset ID.
#[tauri::command]
pub async fn generate_image_thumbnail(
    asset_path: String,
    asset_id: String,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    use image::ImageFormat;
    use sha2::{Digest, Sha256};
    use std::io::Cursor;
    use tauri::Manager;

    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;

    let thumb_dir = app_dir.join("thumbnails");
    std::fs::create_dir_all(&thumb_dir)
        .map_err(|e| format!("Failed to create thumbnails directory: {e}"))?;

    let path_hash = Sha256::digest(asset_path.as_bytes());
    let thumb_name = format!("image-{asset_id}-{path_hash:x}.png");
    let thumb_path = thumb_dir.join(thumb_name);

    if thumb_path.exists() {
        return Ok(normalize_windows_path_string(&thumb_path));
    }

    let result_path = tokio::task::spawn_blocking(move || {
        let image = image::ImageReader::open(&asset_path)
            .map_err(|e| format!("Failed to open image file: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("Failed to detect image format: {e}"))?
            .decode()
            .map_err(|e| format!("Failed to decode image file: {e}"))?;

        let thumbnail = image.thumbnail(400, 400);
        let mut png_data = Vec::new();
        thumbnail
            .write_to(&mut Cursor::new(&mut png_data), ImageFormat::Png)
            .map_err(|e| format!("Failed to encode image thumbnail: {e}"))?;

        std::fs::write(&thumb_path, png_data)
            .map_err(|e| format!("Failed to write image thumbnail: {e}"))?;

        Ok::<String, String>(normalize_windows_path_string(&thumb_path))
    })
    .await
    .map_err(|e| format!("Image thumbnail generation task panicked: {e}"))??;

    Ok(result_path)
}

/// Delete cached image thumbnails for an asset. Best-effort cleanup when assets
/// are removed; stale path-version thumbnails are harmless if removal fails.
#[tauri::command]
pub async fn delete_image_thumbnail(
    asset_id: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Manager;

    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;

    let thumb_dir = app_dir.join("thumbnails");
    if !thumb_dir.exists() {
        return Ok(());
    }

    let prefix = format!("image-{asset_id}-");
    for entry in std::fs::read_dir(&thumb_dir)
        .map_err(|e| format!("Failed to read thumbnails directory: {e}"))?
    {
        let entry = entry.map_err(|e| format!("Failed to read thumbnail entry: {e}"))?;
        let filename = entry.file_name();
        let filename = filename.to_string_lossy();
        if filename.starts_with(&prefix) && filename.ends_with(".png") {
            std::fs::remove_file(entry.path())
                .map_err(|e| format!("Failed to delete image thumbnail: {e}"))?;
        }
    }

    Ok(())
}

/// Delete a cached PDF thumbnail for an asset.
///
/// Called when a PDF asset is deleted to clean up the thumbnail cache.
/// Returns `Ok(())` even if the file doesn't exist (ENOENT is OK).
#[tauri::command]
pub async fn delete_pdf_thumbnail(
    asset_id: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Manager;

    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;

    let thumb_path = app_dir.join("thumbnails").join(format!("{asset_id}.png"));

    if thumb_path.exists() {
        std::fs::remove_file(&thumb_path)
            .map_err(|e| format!("Failed to delete thumbnail: {e}"))?;
    }

    Ok(())
}

/// Check whether a PDF is scanned (image-only) by testing if its native text
/// layer passes quality checks.
///
/// Returns `true` if the PDF has insufficient native text (likely scanned/image-only)
/// and should be split into per-page image assets during import.
#[tauri::command]
pub async fn is_scanned_pdf(
    asset_path: String,
    app_handle: tauri::AppHandle,
) -> Result<bool, String> {
    // Ensure Pdfium is initialized
    super::pdf::init_pdfium_path(&app_handle);

    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&asset_path))
        .await
        .map_err(|e| format!("Failed to read PDF file: {e}"))?
        .map_err(|e| format!("Failed to read PDF file: {e}"))?;

    let is_scanned = tokio::task::spawn_blocking(move || {
        super::pdf_probe::profile_pdf_bytes(&bytes).map(|profile| profile.should_render_as_images)
    })
    .await
    .map_err(|e| format!("PDF check task panicked: {e}"))??;

    Ok(is_scanned)
}

/// Build a conservative per-page profile for a PDF.
///
/// Only confidently native documents should stay as PDF; mixed, uncertain,
/// image-only, and image-with-OCR documents should be rendered as image pages.
#[tauri::command]
pub async fn probe_pdf(
    asset_path: String,
    app_handle: tauri::AppHandle,
) -> Result<super::pdf_probe::DocumentProfile, String> {
    super::pdf::init_pdfium_path(&app_handle);

    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&asset_path))
        .await
        .map_err(|e| format!("Failed to read PDF file: {e}"))?
        .map_err(|e| format!("Failed to read PDF file: {e}"))?;

    tokio::task::spawn_blocking(move || super::pdf_probe::profile_pdf_bytes(&bytes))
        .await
        .map_err(|e| format!("PDF profile task panicked: {e}"))?
}

/// Render all pages of a PDF as PNG images and save them to disk.
///
/// Used by the frontend import flow to convert scanned PDFs into per-page
/// image assets. Each page is rendered at 300 DPI (target width 2550px),
/// saved as a PNG file in the specified output directory.
///
/// # Arguments
/// * `pdf_path` — Absolute filesystem path to the source PDF file
/// * `output_dir` — Directory where PNG files will be saved (created if missing)
/// * `filename_prefix` — Prefix for output filenames (e.g., "document" → "document_page_1.png").
///   Sanitized before use: path separators, `:`, and `..` cannot escape `output_dir`.
///
/// # Returns
/// A list of `RenderedPage` objects with page numbers and absolute file paths.
#[tauri::command]
pub async fn render_pdf_pages(
    pdf_path: String,
    output_dir: String,
    filename_prefix: String,
    app_handle: tauri::AppHandle,
) -> Result<Vec<RenderedPage>, String> {
    use tauri::Manager;

    // Ensure Pdfium is initialized
    super::pdf::init_pdfium_path(&app_handle);

    let filename_prefix = sanitize_filename_prefix(&filename_prefix);

    // The output directory must stay inside the app data dir — rendered pages
    // become assets there, and an IPC-supplied path must not write elsewhere.
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {e}"))?;
    let out_dir = validate_render_output_dir(&output_dir, &app_data_dir)?;

    // Create output directory if it doesn't exist
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Failed to create output directory: {e}"))?;

    // Read PDF and render pages in a blocking task. The batch helper parses
    // the document once for all pages on the dedicated Pdfium render thread
    // (the engine itself binds once per app lifetime).
    let pages = tokio::task::spawn_blocking(move || {
        let bytes =
            std::fs::read(&pdf_path).map_err(|e| format!("Failed to read PDF file: {e}"))?;

        let file_paths =
            super::pdf::render_pdf_pages_to_png_files(&bytes, &out_dir, &filename_prefix)?;

        Ok::<Vec<RenderedPage>, String>(
            file_paths
                .into_iter()
                .enumerate()
                .map(|(page_index, file_path)| RenderedPage {
                    page_number: (page_index + 1) as u32,
                    png_path: normalize_windows_path_string(&file_path),
                })
                .collect(),
        )
    })
    .await
    .map_err(|e| format!("PDF render task panicked: {e}"))??;

    Ok(pages)
}

/// Validate that the caller-supplied output directory resolves inside the app
/// data dir. The directory may not exist yet (it is created right after);
/// `..` traversal and out-of-scope paths are rejected. Returns the
/// canonicalized directory to use for writing.
fn validate_render_output_dir(
    output_dir: &str,
    app_data_dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    crate::path_utils::ensure_within_dir(output_dir, app_data_dir)
        .map_err(|e| format!("Invalid output directory: {e}"))
}

/// Sanitize a caller-supplied filename prefix so the output files cannot
/// escape the output directory: path separators, drive colons, and control
/// characters become `_`, and `..` sequences are collapsed. Normal basename
/// prefixes (e.g., "document") pass through unchanged; an empty result falls
/// back to "document".
fn sanitize_filename_prefix(prefix: &str) -> String {
    let mut sanitized: String = prefix
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    while sanitized.contains("..") {
        sanitized = sanitized.replace("..", "_");
    }

    if sanitized.trim().is_empty() {
        "document".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_prefix_keeps_normal_basenames() {
        assert_eq!(sanitize_filename_prefix("document"), "document");
        assert_eq!(
            sanitize_filename_prefix("Acta 1923 - foja 4"),
            "Acta 1923 - foja 4"
        );
    }

    #[test]
    fn sanitize_filename_prefix_strips_separators_and_traversal() {
        let windows = sanitize_filename_prefix(r"..\..\AppData\evil");
        assert!(!windows.contains('\\'));
        assert!(!windows.contains(".."));

        let unix = sanitize_filename_prefix("../../etc/evil");
        assert!(!unix.contains('/'));
        assert!(!unix.contains(".."));

        assert_eq!(sanitize_filename_prefix("C:evil"), "C_evil");
    }

    #[test]
    fn sanitize_filename_prefix_falls_back_when_empty() {
        assert_eq!(sanitize_filename_prefix(""), "document");
        assert_eq!(sanitize_filename_prefix("   "), "document");
        assert_eq!(sanitize_filename_prefix("///"), "___");
    }

    #[test]
    fn validate_render_output_dir_accepts_dirs_inside_app_data() {
        let app_data = tempfile::tempdir().expect("tempdir");
        let existing = app_data.path().join("assets").join("col-1");
        std::fs::create_dir_all(&existing).expect("create existing dir");

        assert!(validate_render_output_dir(&existing.to_string_lossy(), app_data.path()).is_ok());
        // Not-yet-created output dirs inside the app data dir are valid too.
        let missing = app_data.path().join("assets").join("col-2").join("item-9");
        assert!(validate_render_output_dir(&missing.to_string_lossy(), app_data.path()).is_ok());
    }

    #[test]
    fn validate_render_output_dir_rejects_outside_and_traversal_paths() {
        let app_data = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir outside");

        assert!(
            validate_render_output_dir(&outside.path().to_string_lossy(), app_data.path()).is_err()
        );
        let escape = app_data.path().join("assets").join("..").join("..");
        assert!(validate_render_output_dir(&escape.to_string_lossy(), app_data.path()).is_err());
        assert!(validate_render_output_dir("", app_data.path()).is_err());
    }
}
