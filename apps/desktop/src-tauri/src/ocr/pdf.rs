//! PDF page rendering for OCR fallback.
//!
//! `render_pdf_pages_to_png_files()` rasterizes every page of a PDF to PNG
//! files via `pdfium-render`, parsing the document once for the whole batch.
//! It powers the scanned-PDF import flow.
//!
//! Thumbnails:
//! - `render_pdf_thumbnail()` renders the first page at 400px width, suitable for
//!   card previews in the collection view.
//!
//! # Pdfium render thread (serialization point)
//!
//! PDFium (the C library) is NOT thread-safe, and `pdfium-render` 0.8 without
//! its `sync` feature marks the `Pdfium` struct `!Send + !Sync`. Additionally,
//! the crate's default `thread_safe` feature holds a process-global lock from
//! `FPDF_InitLibrary` until `FPDF_DestroyLibrary` — i.e. for as long as a
//! `Pdfium` instance is alive — so exactly ONE long-lived instance may exist.
//!
//! All Pdfium work therefore goes through a single dedicated actor thread
//! (`pdfium-render`, spawned lazily via [`PDF_ACTOR`]): it binds/initializes
//! the engine once per app lifetime (on the first request) and processes
//! render/probe requests strictly in arrival order. That receive loop is the
//! serialization point for every PDF operation in the app. The public entry
//! points below keep their original blocking signatures and simply forward to
//! the actor over a channel, so callers are unchanged.
//!
//! # Pdfium native library resolution
//!
//! The `pdfium-render` crate requires a native Pdfium shared library (`pdfium.dll`
//! on Windows, `libpdfium.so` on Linux, `libpdfium.dylib` on macOS).
//!
//! Resolution order (matching the bundled native-library patterns):
//! 1. **Bundled resource** — `resources/lib/` via Tauri's `BaseDirectory::Resource`
//! 2. **Dev fallback** — `CARGO_MANIFEST_DIR/resources/lib/` (for development)
//! 3. **System library** — OS default search paths (`PATH`, `/usr/lib`, etc.)
//!
//! Call `init_pdfium_path()` once during app startup (from OCR worker or command
//! handler) to cache the resolved path. If never called, falls back to the
//! current directory + system library (original pdfium-render behavior).

use super::pdf_probe::DocumentProfile;
use pdfium_render::prelude::*;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::sync::{Mutex, OnceLock};
use tauri::path::BaseDirectory;
use tauri::Manager;

/// Cached resolved path to the Pdfium native library.
///
/// - `Some(Some(path))` = initialized with a resolved DLL path
/// - `Some(None)` = initialized, but DLL not found in bundled paths (use system library)
/// - `None` = not yet initialized (fall back to CWD + system library)
static PDFIUM_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

/// Resolve the Pdfium native library path using 3-tier resolution.
///
/// This function MUST be called once during app startup (from the OCR worker or
/// command handler) to cache the DLL path. It is safe to call multiple times —
/// only the first call sets the cached value.
///
/// # Resolution order
/// 1. Tauri resource path: `BaseDirectory::Resource` + `resources/lib/`
/// 2. CARGO_MANIFEST_DIR fallback: `<manifest>/resources/lib/`
/// 3. No bundled path found → falls back to system library at runtime
pub fn init_pdfium_path(app_handle: &tauri::AppHandle) {
    let resolved = resolve_pdfium_dll_path(
        bundled_pdfium_candidate_paths(app_handle),
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
    );

    let cache = PDFIUM_PATH.get_or_init(|| Mutex::new(None));
    let mut cached = cache.lock().expect("pdfium path cache poisoned");
    let should_update = match (&*cached, &resolved) {
        (None, Some(_)) => true,
        (Some(existing), Some(new_path)) => existing != new_path,
        _ => false,
    };

    if should_update {
        *cached = resolved.clone();
    }

    match cached.as_ref() {
        Some(path) => eprintln!(
            "[pdf] ✅ Pdfium native library resolved: {}",
            path.display()
        ),
        None => {
            eprintln!(
                "[pdf] Pdfium no se resolvió desde recursos bundle/dev; se intentará la librería del sistema si ya está disponible ({})",
                dll_name_display()
            )
        }
    }
}

fn resolve_pdfium_dll_path(
    bundled_candidates: impl IntoIterator<Item = PathBuf>,
    manifest_dir: &std::path::Path,
) -> Option<PathBuf> {
    let dll_name = Pdfium::pdfium_platform_library_name();

    for bundled_path in bundled_candidates {
        if bundled_path.exists() {
            return Some(strip_windows_prefix(bundled_path));
        }
    }

    for dev_path in dev_pdfium_candidate_paths(manifest_dir, dll_name.to_string_lossy().as_ref()) {
        if dev_path.exists() {
            return Some(strip_windows_prefix(dev_path));
        }
    }

    None
}

fn bundled_pdfium_candidate_paths(app_handle: &tauri::AppHandle) -> Vec<PathBuf> {
    let dll_name = Pdfium::pdfium_platform_library_name();
    pdfium_resource_relative_paths(dll_name.to_string_lossy().as_ref())
        .into_iter()
        .filter_map(|relative_path| {
            app_handle
                .path()
                .resolve(relative_path, BaseDirectory::Resource)
                .ok()
        })
        .collect()
}

fn pdfium_resource_relative_paths(dll_name: &str) -> Vec<PathBuf> {
    let base_candidate = PathBuf::from("resources").join("lib").join(dll_name);

    #[cfg(target_os = "linux")]
    {
        let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        vec![
            base_candidate,
            PathBuf::from("resources")
                .join("lib")
                .join(platform)
                .join(dll_name),
        ]
    }

    #[cfg(not(target_os = "linux"))]
    {
        vec![base_candidate]
    }
}

fn dev_pdfium_candidate_paths(manifest_dir: &Path, dll_name: &str) -> Vec<PathBuf> {
    let base_candidate = manifest_dir.join("resources").join("lib").join(dll_name);

    #[cfg(target_os = "linux")]
    {
        let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
        let mut candidates = vec![base_candidate];
        candidates.push(
            manifest_dir
                .join("resources")
                .join("lib")
                .join(&platform)
                .join(dll_name),
        );
        candidates
    }

    #[cfg(not(target_os = "linux"))]
    {
        vec![base_candidate]
    }
}

/// Strip the Windows `\\?\` UNC prefix from a path if present.
///
/// Tauri's `resolve()` on Windows may return paths with the `\\?\` prefix
/// (extended-length path prefix). Some native libraries and APIs don't handle
/// this prefix correctly, so we strip it for compatibility.
fn strip_windows_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy().into_owned();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

/// Initialize a Pdfium instance without panicking.
///
/// Uses the cached DLL path if `init_pdfium_path()` was called, otherwise
/// falls back to current directory + system library (original behavior).
///
/// Only the render actor thread may call this (plus the no-panic unit test):
/// while the returned instance is alive, `pdfium-render`'s `thread_safe`
/// marshall holds a process-global lock, so creating a second instance
/// anywhere else would block forever.
///
/// # Errors
/// Returns `Err` with a human-readable message if the Pdfium native
/// library cannot be loaded (missing DLL/so/dylib, wrong architecture, etc.).
fn get_pdfium() -> Result<Pdfium, String> {
    let cached_path = PDFIUM_PATH
        .get()
        .and_then(|cache| cache.lock().ok().and_then(|path| path.clone()));
    let attempted_resolved_path = cached_path.clone();

    let bindings = match cached_path.as_ref() {
        // Initialized with a resolved DLL path — try that first, then system library
        Some(path) => Pdfium::bind_to_library(path).or_else(|path_err| {
            eprintln!(
                "[pdf] Failed to load pdfium from bundled/dev path ({}): {path_err}; trying system library if already available",
                path.display()
            );
            Pdfium::bind_to_system_library()
        }),
        // Initialized but no bundled DLL found — system library only
        None if PDFIUM_PATH.get().is_some() => Pdfium::bind_to_system_library(),
        // Not initialized — fall back to CWD + system library (original pdfium-render behavior)
        None => Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path("./"))
            .or_else(|_| Pdfium::bind_to_system_library()),
    }
    .map_err(|e| {
        let resolved_path_note = attempted_resolved_path
            .as_ref()
            .map(|path| format!("- Resolved bundled/dev path attempted: {}\n", path.display()))
            .unwrap_or_default();

        format!(
            "Could not load Pdfium native library.\n\
             Error: {e}\n\n\
             Resolution tried:\n\
             {}\
             - Bundled resource: resources/lib/{}\n\
             - Development: CARGO_MANIFEST_DIR/resources/lib/{}\n\
             - Linux dev fallback: CARGO_MANIFEST_DIR/resources/lib/linux-x86_64/{}\n\
             - Existing system library paths\n\n\
               Pdfium is an application-bundled resource for PDF rendering. Reinstall EntropIA Lite or contact support if this file is missing.",
            resolved_path_note,
            dll_name_display(),
            dll_name_display(),
            dll_name_display(),
        )
    })?;

    Ok(Pdfium::new(bindings))
}

/// Returns the platform-specific Pdfium library filename for error messages.
fn dll_name_display() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "pdfium.dll"
    }
    #[cfg(target_os = "linux")]
    {
        "libpdfium.so"
    }
    #[cfg(target_os = "macos")]
    {
        "libpdfium.dylib"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        "pdfium"
    }
}

/// A request for the Pdfium render actor. Each variant carries its inputs by
/// value plus a oneshot-style response channel the actor replies on.
enum PdfRequest {
    RenderPages {
        bytes: Vec<u8>,
        out_dir: PathBuf,
        filename_prefix: String,
        respond: std_mpsc::Sender<Result<Vec<PathBuf>, String>>,
    },
    RenderThumbnail {
        bytes: Vec<u8>,
        respond: std_mpsc::Sender<Result<Vec<u8>, String>>,
    },
    ProfileDocument {
        bytes: Vec<u8>,
        respond: std_mpsc::Sender<Result<DocumentProfile, String>>,
    },
}

const PDF_ACTOR_UNAVAILABLE: &str =
    "PDF render thread is not available (it exited unexpectedly); restart EntropIA Lite";

/// Sender half of the render actor's request channel. The actor thread is
/// spawned lazily on first use and lives for the rest of the app lifetime.
static PDF_ACTOR: OnceLock<std_mpsc::Sender<PdfRequest>> = OnceLock::new();

fn pdf_actor() -> &'static std_mpsc::Sender<PdfRequest> {
    PDF_ACTOR.get_or_init(|| {
        let (sender, receiver) = std_mpsc::channel::<PdfRequest>();
        std::thread::Builder::new()
            .name("pdfium-render".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || run_pdf_actor(receiver))
            .expect("Failed to spawn Pdfium render thread");
        sender
    })
}

/// Actor loop: owns the only `Pdfium` instance in the process and serves
/// requests strictly in arrival order — this is the serialization point for
/// all PDF work.
fn run_pdf_actor(receiver: std_mpsc::Receiver<PdfRequest>) {
    // The engine slot starts empty; the first request binds Pdfium. On bind
    // failure the slot stays empty so the next request retries (e.g. when the
    // DLL path could not be resolved yet). After the first success the same
    // engine serves every request until the app exits.
    let mut engine: Option<PdfiumEngine> = None;
    while let Ok(request) = receiver.recv() {
        handle_pdf_request(&mut engine, bind_pdfium_engine, request);
    }
}

/// Dispatch one request to the matching engine operation and reply on its
/// response channel. Generic over the engine so routing is unit-testable
/// without the Pdfium native library.
fn handle_pdf_request<E: PdfEngineOps>(
    engine: &mut Option<E>,
    bind: impl FnOnce() -> Result<E, String>,
    request: PdfRequest,
) {
    match request {
        PdfRequest::RenderPages {
            bytes,
            out_dir,
            filename_prefix,
            respond,
        } => {
            let result = run_engine_operation(engine, bind, |engine| {
                engine.render_pages(&bytes, &out_dir, &filename_prefix)
            });
            let _ = respond.send(result);
        }
        PdfRequest::RenderThumbnail { bytes, respond } => {
            let result =
                run_engine_operation(engine, bind, |engine| engine.render_thumbnail(&bytes));
            let _ = respond.send(result);
        }
        PdfRequest::ProfileDocument { bytes, respond } => {
            let result =
                run_engine_operation(engine, bind, |engine| engine.profile_document(&bytes));
            let _ = respond.send(result);
        }
    }
}

/// Bind the engine on first use, then run `operation` against it. A panic
/// inside the operation must not kill the render thread: it is converted into
/// an error for this request and the actor keeps serving subsequent ones.
fn run_engine_operation<E, T>(
    engine: &mut Option<E>,
    bind: impl FnOnce() -> Result<E, String>,
    operation: impl FnOnce(&E) -> Result<T, String>,
) -> Result<T, String> {
    if engine.is_none() {
        *engine = Some(bind()?);
    }
    let bound = engine.as_ref().expect("engine bound above");
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(bound)))
        .unwrap_or_else(|_| Err("PDF operation panicked; the document may be corrupt".to_string()))
}

/// The operations the render actor can perform. Implemented by the real
/// Pdfium-backed engine and by fakes in unit tests.
trait PdfEngineOps {
    fn render_pages(
        &self,
        bytes: &[u8],
        out_dir: &Path,
        filename_prefix: &str,
    ) -> Result<Vec<PathBuf>, String>;
    fn render_thumbnail(&self, bytes: &[u8]) -> Result<Vec<u8>, String>;
    fn profile_document(&self, bytes: &[u8]) -> Result<DocumentProfile, String>;
}

/// The real engine: wraps the app's single long-lived `Pdfium` instance.
struct PdfiumEngine {
    pdfium: Pdfium,
}

fn bind_pdfium_engine() -> Result<PdfiumEngine, String> {
    get_pdfium().map(|pdfium| PdfiumEngine { pdfium })
}

impl PdfEngineOps for PdfiumEngine {
    fn render_pages(
        &self,
        bytes: &[u8],
        out_dir: &Path,
        filename_prefix: &str,
    ) -> Result<Vec<PathBuf>, String> {
        render_pdf_pages_with_engine(&self.pdfium, bytes, out_dir, filename_prefix)
    }

    fn render_thumbnail(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        render_pdf_thumbnail_with_engine(&self.pdfium, bytes)
    }

    fn profile_document(&self, bytes: &[u8]) -> Result<DocumentProfile, String> {
        super::pdf_probe::profile_pdf_with_engine(&self.pdfium, bytes)
    }
}

/// Returns `true` if the text contains at least `MIN_ALPHANUM_CHARS` valid
/// UTF-8 alphanumeric characters. Used to decide whether native PDF text is
/// rich enough or we should fall back to OCR.
#[cfg(test)]
fn is_quality_text(text: &str) -> bool {
    const MIN_ALPHANUM_CHARS: usize = 50;
    text.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_ALPHANUM_CHARS
}

/// Render every page of a PDF to PNG files on disk.
///
/// Parses the document ONCE on the render actor thread (the engine itself is
/// bound once per app lifetime), then iterates the pages. Each page is
/// rendered at 300 DPI equivalent (target width 2550px) and written to
/// `{out_dir}/{filename_prefix}_page_{n}.png` (1-based page numbers).
///
/// Blocks until the render actor replies; call from a blocking-safe context
/// (worker thread or `spawn_blocking`).
///
/// Returns the written file paths in page order.
///
/// # Errors
/// Returns `Err` if:
/// - Pdfium fails to initialize
/// - PDF cannot be loaded
/// - Rendering, encoding, or writing any page fails
pub fn render_pdf_pages_to_png_files(
    bytes: &[u8],
    out_dir: &Path,
    filename_prefix: &str,
) -> Result<Vec<PathBuf>, String> {
    let (respond, response) = std_mpsc::channel();
    pdf_actor()
        .send(PdfRequest::RenderPages {
            bytes: bytes.to_vec(),
            out_dir: out_dir.to_path_buf(),
            filename_prefix: filename_prefix.to_string(),
            respond,
        })
        .map_err(|_| PDF_ACTOR_UNAVAILABLE.to_string())?;
    response
        .recv()
        .map_err(|_| PDF_ACTOR_UNAVAILABLE.to_string())?
}

/// Profile a PDF on the render actor thread. Blocking wrapper used by
/// `pdf_probe::profile_pdf_bytes` so its public signature stays unchanged.
pub(super) fn profile_pdf_via_actor(bytes: &[u8]) -> Result<DocumentProfile, String> {
    let (respond, response) = std_mpsc::channel();
    pdf_actor()
        .send(PdfRequest::ProfileDocument {
            bytes: bytes.to_vec(),
            respond,
        })
        .map_err(|_| PDF_ACTOR_UNAVAILABLE.to_string())?;
    response
        .recv()
        .map_err(|_| PDF_ACTOR_UNAVAILABLE.to_string())?
}

/// Implementation of [`render_pdf_pages_to_png_files`]; runs on the render
/// actor thread with the already-bound engine.
fn render_pdf_pages_with_engine(
    pdfium: &Pdfium,
    bytes: &[u8],
    out_dir: &Path,
    filename_prefix: &str,
) -> Result<Vec<PathBuf>, String> {
    use std::io::Write;

    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| format!("Failed to load PDF: {e}"))?;

    let pages = document.pages();
    let page_count: usize = pages.len().into();

    eprintln!("[render_pdf_pages] Rendering {page_count} pages from PDF");

    // Render at 300 DPI equivalent. A typical letter-size page is 8.5" × 11"
    // which at 300 DPI gives 2550 × 3300 pixels.
    let render_config = PdfRenderConfig::new()
        .set_target_width(2550)
        .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true);

    let mut file_paths: Vec<PathBuf> = Vec::with_capacity(page_count);

    for page_index in 0..page_count {
        let page_number = page_index + 1;
        let page = pages
            .get(PdfPageIndex::from(page_index as u16))
            .map_err(|e| format!("Failed to get page {page_number} from PDF: {e}"))?;

        let bitmap = page
            .render_with_config(&render_config)
            .map_err(|e| format!("Failed to render PDF page {page_number}: {e}"))?;

        // Convert to image::DynamicImage, then encode as PNG
        let mut png_bytes = Vec::new();
        bitmap
            .as_image()
            .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .map_err(|e| format!("Failed to encode page {page_number} as PNG: {e}"))?;

        let file_path = out_dir.join(format!("{filename_prefix}_page_{page_number}.png"));
        let mut file = std::fs::File::create(&file_path)
            .map_err(|e| format!("Failed to create PNG file for page {page_number}: {e}"))?;
        file.write_all(&png_bytes)
            .map_err(|e| format!("Failed to write PNG data for page {page_number}: {e}"))?;

        eprintln!("[render_pdf_pages] Rendered page {page_number}/{page_count}");

        file_paths.push(file_path);
    }

    Ok(file_paths)
}

/// Render the first page of a PDF to PNG bytes at thumbnail resolution (400px wide).
///
/// Intended for collection-view card previews. The output is a compact PNG
/// suitable for use as an `<img>` src via `convertFileSrc`.
///
/// Uses `pdfium-render` with a target width of 400px (roughly 50 DPI equivalent),
/// yielding small files that load fast in the UI. Blocks until the render
/// actor replies; call from a blocking-safe context.
pub fn render_pdf_thumbnail(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Err("PDF bytes are empty".to_string());
    }

    let (respond, response) = std_mpsc::channel();
    pdf_actor()
        .send(PdfRequest::RenderThumbnail {
            bytes: bytes.to_vec(),
            respond,
        })
        .map_err(|_| PDF_ACTOR_UNAVAILABLE.to_string())?;
    response
        .recv()
        .map_err(|_| PDF_ACTOR_UNAVAILABLE.to_string())?
}

/// Implementation of [`render_pdf_thumbnail`]; runs on the render actor
/// thread with the already-bound engine.
fn render_pdf_thumbnail_with_engine(pdfium: &Pdfium, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let document = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| format!("Failed to load PDF for thumbnail: {e}"))?;

    let pages = document.pages();
    if pages.is_empty() {
        return Err("PDF has no pages".to_string());
    }

    let page = pages
        .get(PdfPageIndex::from(0u16))
        .map_err(|e| format!("Failed to get first page from PDF: {e}"))?;

    let render_config = PdfRenderConfig::new()
        .set_target_width(400)
        .rotate_if_landscape(PdfPageRenderRotation::Degrees90, true);

    let bitmap = page
        .render_with_config(&render_config)
        .map_err(|e| format!("Failed to render PDF thumbnail: {e}"))?;

    let dynamic_image = bitmap.as_image();

    let mut png_bytes = Vec::new();
    dynamic_image
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode thumbnail as PNG: {e}"))?;

    Ok(png_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_pdfium_prefers_bundled_resource_path() {
        let resource_dir = tempdir().expect("resource dir");
        let manifest_dir = tempdir().expect("manifest dir");
        let bundled_dll = resource_dir
            .path()
            .join("resources")
            .join("lib")
            .join(Pdfium::pdfium_platform_library_name());
        let dev_dll = manifest_dir
            .path()
            .join("resources")
            .join("lib")
            .join(Pdfium::pdfium_platform_library_name());
        std::fs::create_dir_all(bundled_dll.parent().expect("lib parent"))
            .expect("create bundled lib dir");
        std::fs::create_dir_all(dev_dll.parent().expect("lib parent")).expect("create dev lib dir");
        std::fs::write(&bundled_dll, b"pdfium").expect("write bundled dll");
        std::fs::write(&dev_dll, b"pdfium").expect("write dev dll");

        let resolved = resolve_pdfium_dll_path(vec![bundled_dll.clone()], manifest_dir.path());

        assert_eq!(resolved, Some(bundled_dll));
    }

    #[test]
    fn resolve_pdfium_uses_dev_resource_when_bundle_missing() {
        let missing_resource_dir = tempdir().expect("resource dir");
        let manifest_dir = tempdir().expect("manifest dir");
        let missing_bundled_dll = missing_resource_dir
            .path()
            .join("resources")
            .join("lib")
            .join(Pdfium::pdfium_platform_library_name());
        let dev_dll = manifest_dir
            .path()
            .join("resources")
            .join("lib")
            .join(Pdfium::pdfium_platform_library_name());
        std::fs::create_dir_all(dev_dll.parent().expect("lib parent")).expect("create dev lib dir");
        std::fs::write(&dev_dll, b"pdfium").expect("write dev dll");

        let resolved = resolve_pdfium_dll_path(vec![missing_bundled_dll], manifest_dir.path());

        assert_eq!(resolved, Some(dev_dll));
    }

    #[test]
    fn resolve_pdfium_finds_linux_arch_specific_dev_resource() {
        let manifest_dir = tempdir().expect("manifest dir");
        let arch_specific = manifest_dir
            .path()
            .join("resources")
            .join("lib")
            .join("linux-x86_64")
            .join(Pdfium::pdfium_platform_library_name());
        std::fs::create_dir_all(arch_specific.parent().expect("parent")).expect("mkdir");
        std::fs::write(&arch_specific, b"pdfium").expect("write");

        let resolved = resolve_pdfium_dll_path(Vec::new(), manifest_dir.path());

        #[cfg(target_os = "linux")]
        assert_eq!(resolved, Some(arch_specific));
        #[cfg(not(target_os = "linux"))]
        assert_eq!(resolved, None);
    }

    #[test]
    fn pdfium_resource_relative_paths_include_linux_arch_specific_path() {
        let paths = pdfium_resource_relative_paths("libpdfium.so");
        #[cfg(target_os = "linux")]
        assert!(paths
            .iter()
            .any(|path| path == &PathBuf::from("resources/lib/linux-x86_64/libpdfium.so")));
        #[cfg(not(target_os = "linux"))]
        assert_eq!(paths, vec![PathBuf::from("resources/lib/libpdfium.so")]);
    }

    #[test]
    fn empty_text_is_not_quality() {
        assert!(!is_quality_text(""));
    }

    #[test]
    fn short_garbled_text_is_not_quality() {
        let garbled = "!@#$%^&*()_+-=[]{}|;':\",./<>? abc 123";
        assert!(!is_quality_text(garbled));
    }

    #[test]
    fn normal_text_is_quality() {
        let text = "This is a perfectly normal paragraph of text that contains well over fifty alphanumeric characters and should pass the quality heuristic with ease.";
        assert!(is_quality_text(text));
    }

    /// get_pdfium() must never panic — it should return Err when the native
    /// library is unavailable. This test runs in CI where pdfium.dll is often
    /// absent, so it exercises the unhappy path.
    #[test]
    fn get_pdfium_returns_error_without_native_library() {
        // If pdfium is installed, this will succeed — that's fine, we only
        // assert that it doesn't panic. If it's not installed, it must return Err.
        let result = get_pdfium();
        // Either outcome is acceptable; the important thing is NO PANIC.
        // When the library is missing, the error message must mention Pdfium.
        if let Err(msg) = &result {
            assert!(
                msg.contains("Pdfium") || msg.contains("pdfium"),
                "Error message should reference the Pdfium library, got: {msg}"
            );
        }
    }

    /// render_pdf_pages_to_png_files requires the pdfium native library which
    /// may not be available in unit test environments. Marked as ignored.
    #[test]
    #[ignore]
    fn render_pdf_pages_to_png_files_invalid_bytes() {
        // Invalid PDF bytes should return an error, not panic
        let out_dir = tempdir().expect("out dir");
        let result = render_pdf_pages_to_png_files(b"not a pdf", out_dir.path(), "doc");
        assert!(result.is_err(), "Expected error for invalid PDF bytes");
    }

    /// render_pdf_thumbnail requires the pdfium native library which may not be
    /// available in unit test environments. Marked as ignored.
    #[test]
    #[ignore]
    fn render_pdf_thumbnail_invalid_bytes() {
        // Invalid PDF bytes should return an error, not panic
        let result = render_pdf_thumbnail(b"not a pdf");
        assert!(
            result.is_err(),
            "Expected error for invalid PDF bytes in thumbnail"
        );
    }

    #[test]
    fn render_pdf_thumbnail_empty_bytes() {
        // Empty bytes should return an error (no pdfium needed for this check)
        let result = render_pdf_thumbnail(b"");
        assert!(result.is_err(), "Expected error for empty PDF bytes");
    }

    #[test]
    fn test_strip_windows_prefix() {
        // No prefix — should return unchanged
        let path = PathBuf::from(r"C:\Users\test\file.dll");
        assert_eq!(strip_windows_prefix(path.clone()), path);

        // With prefix — should strip it
        let prefixed = PathBuf::from(r"\\?\C:\Users\test\file.dll");
        let stripped = strip_windows_prefix(prefixed);
        assert_eq!(stripped, PathBuf::from(r"C:\Users\test\file.dll"));

        // Empty path — should be fine
        let empty = PathBuf::from("");
        assert_eq!(strip_windows_prefix(empty.clone()), empty);
    }

    #[test]
    fn test_dll_name_display() {
        // Just verify it returns a non-empty string
        let name = dll_name_display();
        assert!(
            !name.is_empty(),
            "dll_name_display should return a non-empty string"
        );
        assert!(
            name.contains("pdfium") || name.contains("Pdfium"),
            "dll_name_display should contain 'pdfium', got: {name}"
        );
    }

    // ---- Render actor request routing (no Pdfium binary required) ----

    /// Fake engine so routing can be exercised without the native library.
    #[derive(Default)]
    struct FakeEngine {
        panic_on_thumbnail: bool,
    }

    impl PdfEngineOps for FakeEngine {
        fn render_pages(
            &self,
            _bytes: &[u8],
            out_dir: &Path,
            filename_prefix: &str,
        ) -> Result<Vec<PathBuf>, String> {
            Ok(vec![out_dir.join(format!("{filename_prefix}_page_1.png"))])
        }

        fn render_thumbnail(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
            if self.panic_on_thumbnail {
                panic!("fake thumbnail panic");
            }
            Ok(bytes.to_vec())
        }

        fn profile_document(&self, _bytes: &[u8]) -> Result<DocumentProfile, String> {
            Ok(crate::ocr::pdf_probe::summarize_document(Vec::new()))
        }
    }

    #[test]
    fn pdf_request_routing_dispatches_each_variant_to_matching_operation() {
        let mut engine: Option<FakeEngine> = None;

        let (respond, thumbnail_response) = std_mpsc::channel();
        handle_pdf_request(
            &mut engine,
            || Ok(FakeEngine::default()),
            PdfRequest::RenderThumbnail {
                bytes: vec![1, 2, 3],
                respond,
            },
        );
        assert_eq!(
            thumbnail_response.recv().expect("thumbnail response"),
            Ok(vec![1, 2, 3])
        );

        let (respond, pages_response) = std_mpsc::channel();
        handle_pdf_request(
            &mut engine,
            || Ok(FakeEngine::default()),
            PdfRequest::RenderPages {
                bytes: vec![1],
                out_dir: PathBuf::from("out"),
                filename_prefix: "doc".to_string(),
                respond,
            },
        );
        assert_eq!(
            pages_response.recv().expect("pages response"),
            Ok(vec![PathBuf::from("out").join("doc_page_1.png")])
        );

        let (respond, profile_response) = std_mpsc::channel();
        handle_pdf_request(
            &mut engine,
            || Ok(FakeEngine::default()),
            PdfRequest::ProfileDocument {
                bytes: vec![1],
                respond,
            },
        );
        let profile = profile_response
            .recv()
            .expect("profile response")
            .expect("profile result");
        assert!(profile.pages.is_empty());
    }

    #[test]
    fn pdf_request_routing_retries_bind_after_failure() {
        let mut engine: Option<FakeEngine> = None;

        let (respond, response) = std_mpsc::channel();
        handle_pdf_request(
            &mut engine,
            || Err("no native library".to_string()),
            PdfRequest::RenderThumbnail {
                bytes: vec![1],
                respond,
            },
        );
        assert_eq!(
            response.recv().expect("response"),
            Err("no native library".to_string())
        );
        assert!(engine.is_none(), "failed bind must not cache an engine");

        let (respond, response) = std_mpsc::channel();
        handle_pdf_request(
            &mut engine,
            || Ok(FakeEngine::default()),
            PdfRequest::RenderThumbnail {
                bytes: vec![1],
                respond,
            },
        );
        assert!(
            response.recv().expect("response").is_ok(),
            "next request must retry the bind"
        );
        assert!(engine.is_some(), "successful bind is cached");
    }

    #[test]
    fn pdf_request_routing_binds_engine_exactly_once() {
        let mut engine: Option<FakeEngine> = None;
        let binds = std::cell::Cell::new(0_u32);

        for _ in 0..3 {
            let (respond, response) = std_mpsc::channel();
            handle_pdf_request(
                &mut engine,
                || {
                    binds.set(binds.get() + 1);
                    Ok(FakeEngine::default())
                },
                PdfRequest::RenderThumbnail {
                    bytes: vec![1],
                    respond,
                },
            );
            assert!(response.recv().expect("response").is_ok());
        }

        assert_eq!(binds.get(), 1, "engine binds exactly once across requests");
    }

    #[test]
    fn pdf_request_routing_survives_a_panicking_operation() {
        let mut engine = Some(FakeEngine {
            panic_on_thumbnail: true,
        });

        let (respond, response) = std_mpsc::channel();
        handle_pdf_request(
            &mut engine,
            || unreachable!("engine is already bound"),
            PdfRequest::RenderThumbnail {
                bytes: vec![1],
                respond,
            },
        );

        let result = response
            .recv()
            .expect("actor must reply even when the operation panics");
        assert!(result.is_err(), "panic becomes an error for that request");
        assert!(engine.is_some(), "engine survives the panic");
    }
}
