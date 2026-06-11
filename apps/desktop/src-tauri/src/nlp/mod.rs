pub mod chunking;
pub mod commands;
pub mod embeddings;
pub mod fts;
pub mod ner;
pub mod text_provider;
pub(crate) mod vector;
// NOTE: `triples` module removed — semantic triples are now LLM-only via OpenRouter
// (see llm::LlmJob::ExtractTriples / ExtractTriplesAsset). The old NLP regex route has
// been retired to prevent low-quality triples from overwriting LLM results in the `triples` table.

use rusqlite::OptionalExtension;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use crate::llm::LlmQueue;
use embeddings::EmbeddingEngine;

struct CachedEmbeddingEngine {
    config_key: String,
    engine: Arc<EmbeddingEngine>,
}

// ── Event payloads ───────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct NlpProgressPayload {
    pub item_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    pub job: String,
    pub pct: u8,
}

#[derive(Clone, Serialize)]
pub struct NlpCompletePayload {
    pub item_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    pub job: String,
    /// Number of entities persisted by NER jobs. `None` for non-NER jobs (and
    /// for NER jobs skipped for lack of text), so other consumers are unaffected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_count: Option<usize>,
}

#[derive(Clone, Serialize)]
pub struct NlpErrorPayload {
    pub item_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    pub job: String,
    pub error: String,
}

// ── Job & Queue ──────────────────────────────────────────────────────────────

/// A single NLP work unit submitted to the background worker.
#[derive(Debug)]
pub enum NlpJob {
    IndexFts { item_id: String },
    ExtractEntities { item_id: String },
    EnrichItem { item_id: String },
    // Asset-level variants: process only the selected asset/page
    ComputeAssetEmbedding { item_id: String, asset_id: String },
    ExtractEntitiesForAsset { item_id: String, asset_id: String },
}

pub fn lookup_item_id_for_asset(
    conn: &rusqlite::Connection,
    asset_id: &str,
) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT item_id FROM assets WHERE id = ?1",
        rusqlite::params![asset_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("Failed to resolve item_id for asset {asset_id}: {e}"))
}

pub fn enqueue_entity_refresh_for_item(nlp_queue: &NlpQueue, item_id: &str) -> Result<(), String> {
    // Dedup: if this item is already pending or in-progress for NER, skip.
    if let Ok(mut pending) = nlp_queue.ner_pending.lock() {
        if pending.contains(item_id) {
            eprintln!("[nlp/ner] Skipping duplicate ExtractEntities enqueue for item_id={item_id}");
            return Ok(());
        }
        pending.insert(item_id.to_string());
    }
    let submit_result = nlp_queue.submit(NlpJob::ExtractEntities {
        item_id: item_id.to_string(),
    });

    if submit_result.is_err() {
        if let Ok(mut pending) = nlp_queue.ner_pending.lock() {
            pending.remove(item_id);
        }
    }

    submit_result
}

/// Handle for submitting NLP jobs to the background worker.
///
/// Managed as Tauri state — NLP commands grab this via `State<NlpQueue>`.
/// Includes a dedup set for ExtractEntities jobs to avoid processing the
/// same item_id twice in quick succession.
pub struct NlpQueue {
    sender: mpsc::Sender<NlpJob>,
    /// Set of item_ids currently pending or in-progress for ExtractEntities.
    /// Prevents duplicate NER work when OCR and transcription both trigger
    /// entity extraction for the same item.
    ner_pending: Arc<Mutex<HashSet<String>>>,
    /// Tracks queued/in-progress FTS jobs per item.
    /// `true` means another enqueue arrived while the current one was busy,
    /// so one extra rerun should happen after the current pass completes.
    fts_pending: Arc<Mutex<HashMap<String, bool>>>,
    /// Tracks queued/in-progress asset-level NER jobs per asset.
    asset_ner_pending: Arc<Mutex<HashSet<String>>>,
    /// Tracks queued/in-progress asset-level embedding jobs per asset.
    embedding_pending: Arc<Mutex<HashSet<String>>>,
}

impl NlpQueue {
    /// Create a new queue and return `(NlpQueue, Receiver)`.
    pub fn new() -> (Self, mpsc::Receiver<NlpJob>) {
        let (sender, receiver) = mpsc::channel::<NlpJob>(64);
        (
            Self {
                sender,
                ner_pending: Arc::new(Mutex::new(HashSet::new())),
                fts_pending: Arc::new(Mutex::new(HashMap::new())),
                asset_ner_pending: Arc::new(Mutex::new(HashSet::new())),
                embedding_pending: Arc::new(Mutex::new(HashSet::new())),
            },
            receiver,
        )
    }

    /// Submit a job to the queue. Returns immediately.
    pub fn submit(&self, job: NlpJob) -> Result<(), String> {
        let mut tracked_fts_item = None;
        let mut tracked_asset_ner = None;
        let mut tracked_embedding = None;

        match &job {
            NlpJob::IndexFts { item_id } => {
                if let Ok(mut pending) = self.fts_pending.lock() {
                    if let Some(needs_rerun) = pending.get_mut(item_id) {
                        *needs_rerun = true;
                        eprintln!(
                            "[nlp/fts] Coalescing duplicate IndexFts enqueue for item_id={item_id}"
                        );
                        return Ok(());
                    }
                    pending.insert(item_id.clone(), false);
                }
                tracked_fts_item = Some(item_id.clone());
            }
            NlpJob::ExtractEntitiesForAsset { asset_id, .. } => {
                if let Ok(mut pending) = self.asset_ner_pending.lock() {
                    if !pending.insert(asset_id.clone()) {
                        eprintln!(
                            "[nlp/ner] Coalescing duplicate ExtractEntitiesForAsset enqueue for asset_id={asset_id}"
                        );
                        return Ok(());
                    }
                }
                tracked_asset_ner = Some(asset_id.clone());
            }
            NlpJob::ComputeAssetEmbedding { asset_id, .. } => {
                if let Ok(mut pending) = self.embedding_pending.lock() {
                    if !pending.insert(asset_id.clone()) {
                        eprintln!(
                            "[nlp/embeddings] Coalescing duplicate ComputeAssetEmbedding enqueue for asset_id={asset_id}"
                        );
                        return Ok(());
                    }
                }
                tracked_embedding = Some(asset_id.clone());
            }
            _ => {}
        }

        self.sender.try_send(job).map_err(|e| {
            if let Some(item_id) = tracked_fts_item {
                if let Ok(mut pending) = self.fts_pending.lock() {
                    pending.remove(&item_id);
                }
            }
            if let Some(asset_id) = tracked_asset_ner {
                if let Ok(mut pending) = self.asset_ner_pending.lock() {
                    pending.remove(&asset_id);
                }
            }
            if let Some(asset_id) = tracked_embedding {
                if let Ok(mut pending) = self.embedding_pending.lock() {
                    pending.remove(&asset_id);
                }
            }
            format!("Failed to enqueue NLP job: {e}")
        })
    }

    /// Get a clone of the NER dedup set handle.
    /// Used by the worker to remove item_ids after processing completes.
    pub fn ner_pending_handle(&self) -> Arc<Mutex<HashSet<String>>> {
        Arc::clone(&self.ner_pending)
    }

    pub fn fts_pending_handle(&self) -> Arc<Mutex<HashMap<String, bool>>> {
        Arc::clone(&self.fts_pending)
    }

    pub fn asset_ner_pending_handle(&self) -> Arc<Mutex<HashSet<String>>> {
        Arc::clone(&self.asset_ner_pending)
    }

    pub fn embedding_pending_handle(&self) -> Arc<Mutex<HashSet<String>>> {
        Arc::clone(&self.embedding_pending)
    }

    /// Spawn the background worker loop on the Tokio runtime.
    ///
    /// The worker drains jobs serially and emits `nlp:progress`, `nlp:complete`,
    /// or `nlp:error` events per job.
    // One handle per pending-job dedup set; bundling them into a struct would obscure the wiring in lib.rs.
    #[allow(clippy::too_many_arguments)]
    pub fn start_worker(
        db_path: std::path::PathBuf,
        mut receiver: mpsc::Receiver<NlpJob>,
        app_handle: AppHandle,
        ner_pending: Arc<Mutex<HashSet<String>>>,
        fts_pending: Arc<Mutex<HashMap<String, bool>>>,
        asset_ner_pending: Arc<Mutex<HashSet<String>>>,
        embedding_pending: Arc<Mutex<HashSet<String>>>,
        _llm_queue: LlmQueue,
    ) {
        tauri::async_runtime::spawn(async move {
            // Open a dedicated SQLite connection for the NLP worker.
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => {
                    let _ = c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
                    c
                }
                Err(e) => {
                    eprintln!("[nlp] Failed to open worker DB connection: {e}");
                    return;
                }
            };

            if table_exists(&conn, "entities") {
                if let Err(e) = ensure_entities_schema(&conn) {
                    eprintln!("[nlp] Failed to migrate entities schema: {e}");
                }
            }

            // Create vec_assets storage for asset-level embeddings.
            if let Err(e) = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS vec_assets(
                    asset_id TEXT PRIMARY KEY,
                    item_id TEXT NOT NULL,
                    embedding BLOB NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_vec_assets_item_id ON vec_assets(item_id)",
            ) {
                eprintln!("[nlp] Failed to create embedding tables: {e} — embedding storage will be unavailable");
            }

            let mut embed_engine: Option<CachedEmbeddingEngine> = None;
            let mut last_embed_engine_init_error: Option<String> = None;

            while let Some(job) = receiver.recv().await {
                match job {
                    NlpJob::IndexFts { item_id } => {
                        emit_progress(&app_handle, &item_id, "fts", 10);
                        let result = run_coalesced_fts_reindex(&conn, &item_id, &fts_pending);
                        match result {
                            Ok(_) => {
                                eprintln!("[nlp/fts] Reindex complete: item_id={item_id}");
                                emit_progress(&app_handle, &item_id, "fts", 100);
                                emit_complete(&app_handle, &item_id, "fts");
                            }
                            Err(e) => emit_error(&app_handle, &item_id, "fts", &e),
                        }
                    }
                    NlpJob::ExtractEntities { item_id } => {
                        emit_progress(&app_handle, &item_id, "ner", 10);
                        let result =
                            ner::prepare_ner_candidates_for_item(&conn, &item_id).map(|input| {
                                if input.text.trim().is_empty() {
                                    None
                                } else {
                                    Some(input)
                                }
                            });
                        // Remove from dedup set so future enqueues for this item are allowed
                        if let Ok(mut pending) = ner_pending.lock() {
                            pending.remove(&item_id);
                        }

                        // Open a dedicated connection for the streaming NER job. The
                        // worker's `conn` is not Send, so we open our own here and
                        // use it through `block_in_place` after every async chunk
                        // call. This mirrors the pattern used by the FTS worker.
                        let job_conn = match rusqlite::Connection::open(&db_path) {
                            Ok(c) => {
                                let _ = c.execute_batch(
                                    "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;",
                                );
                                Some(c)
                            }
                            Err(e) => {
                                emit_error(&app_handle, &item_id, "ner", &e.to_string());
                                continue;
                            }
                        };

                        // Run the streaming NER job inside an async block so we can
                        // use `?` for early-return without polluting the outer
                        // match arm type. `job_conn` lives only inside this block.
                        // Returns the number of persisted entities, or `None` when
                        // the job was skipped for lack of text.
                        let item_id_for_job = item_id.clone();
                        let app_handle_for_job = app_handle.clone();
                        let job_result: Result<Option<usize>, String> = async {
                            let (input, mut job_conn) = match (result, job_conn) {
                                (Ok(Some(input)), Some(c)) => (input, c),
                                (Ok(None), _) => {
                                    crate::app_logs::warn(
                                        &app_handle_for_job,
                                        "nlp",
                                        format!("ner omitido para item_id={item_id_for_job}: sin texto para analizar"),
                                    );
                                    return Ok(None);
                                }
                                (Err(error), _) => {
                                    return Err(format!("NER extraction failed: {error}"));
                                }
                                (Ok(_), None) => unreachable!("job_conn always Some on Ok(_)"),
                            };

                            let config = ner_fallback_config(&job_conn).openrouter?;
                            let (api_key, model_name, prompt_template, params) = config;

                            // Accumulate the entities from every chunk in memory
                            // first; a chunk failure aborts here and leaves the
                            // previously stored entities untouched.
                            let entities = collect_ner_entities_for_chunks(
                                &app_handle_for_job,
                                &item_id_for_job,
                                None,
                                &input,
                                api_key,
                                model_name,
                                prompt_template,
                                params,
                            )
                            .await?;

                            // Swap (clear + append) in one transaction only after
                            // every chunk parsed successfully.
                            tokio::task::block_in_place(|| {
                                ner::replace_automatic_entities_for_item(
                                    &mut job_conn,
                                    &item_id_for_job,
                                    &entities,
                                )
                            })
                            .map_err(|e| {
                                crate::app_logs::warn(
                                    &app_handle_for_job,
                                    "nlp",
                                    format!("ner persistencia falló para item_id={item_id_for_job}: {e}"),
                                );
                                e
                            })?;
                            Ok(Some(entities.len()))
                        }
                        .await;

                        match job_result {
                            Ok(entity_count) => {
                                emit_progress(&app_handle, &item_id, "ner", 100);
                                emit_complete_payload(
                                    &app_handle,
                                    &item_id,
                                    None,
                                    "ner",
                                    entity_count,
                                );
                                // Auto-trigger geocoding for place entities, only
                                // after a successful swap (skipped jobs persist nothing).
                                if entity_count.is_some() {
                                    if let Err(e) = crate::geo::enqueue_geocoding_for_item(
                                        &app_handle.state::<crate::geo::GeoQueue>(),
                                        &item_id,
                                    ) {
                                        eprintln!(
                                            "[geo] Failed to auto-enqueue geocoding after NER: {e}"
                                        );
                                    }
                                }
                            }
                            Err(e) => emit_error(&app_handle, &item_id, "ner", &e),
                        }
                    }
                    NlpJob::EnrichItem { item_id } => {
                        // Run FTS first, then continue with remote NER. Semantic triples are LLM-only
                        // via the LLM pipeline.
                        emit_progress(&app_handle, &item_id, "fts", 10);

                        let db_for_fts = db_path.clone();
                        let item_for_fts = item_id.clone();
                        let fts_handle =
                            tokio::task::spawn_blocking(move || -> Result<(), String> {
                                let c = rusqlite::Connection::open(&db_for_fts)
                                    .map_err(|e| format!("Failed to open FTS connection: {e}"))?;
                                let _ = c.execute_batch(
                                    "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;",
                                );
                                fts::index_item_from_db(&c, &item_for_fts)
                            });

                        match fts_handle.await {
                            Ok(Ok(())) => {
                                emit_progress(&app_handle, &item_id, "fts", 100);
                                emit_complete(&app_handle, &item_id, "fts");
                            }
                            Ok(Err(e)) => emit_error(&app_handle, &item_id, "fts", &e),
                            Err(e) => emit_error(
                                &app_handle,
                                &item_id,
                                "fts",
                                &format!("FTS task panicked: {e}"),
                            ),
                        }

                        // NER sub-job: check dedup set — if ExtractEntities is already
                        // handling this item, skip NER here to avoid duplicate work.
                        let ner_already_pending = ner_pending
                            .lock()
                            .map(|p| p.contains(&item_id))
                            .unwrap_or(false);
                        if ner_already_pending {
                            eprintln!("[nlp/ner] Skipping NER in EnrichItem for item_id={item_id} — already queued or in progress");
                        } else {
                            // Register in dedup set before starting NER
                            if let Ok(mut pending) = ner_pending.lock() {
                                pending.insert(item_id.clone());
                            }
                            emit_progress(&app_handle, &item_id, "ner", 10);

                            // Prepare NER candidates and OpenRouter config on
                            // the worker connection (read-only) before opening
                            // a dedicated connection for the network-bound
                            // streaming loop. Doing this outside the async
                            // block keeps `&conn` out of the future's
                            // environment, which is required because rusqlite
                            // connections are not `Send`.
                            let prepared = ner::prepare_ner_candidates_for_item(&conn, &item_id);
                            let ner_config = ner_fallback_config(&conn).openrouter;

                            let item_id_for_job = item_id.clone();
                            let app_handle_for_job = app_handle.clone();
                            let db_path_for_job = db_path.clone();
                            let job_result: Result<Option<usize>, String> = async {
                                let input = match prepared {
                                    Ok(input) => input,
                                    Err(error) => {
                                        return Err(format!("NER extraction failed: {error}"));
                                    }
                                };
                                if input.text.trim().is_empty() {
                                    crate::app_logs::warn(
                                        &app_handle_for_job,
                                        "nlp",
                                        format!("ner omitido para item_id={item_id_for_job}: sin texto para analizar"),
                                    );
                                    return Ok(None);
                                }

                                let (api_key, model_name, prompt_template, params) =
                                    match ner_config {
                                        Ok(values) => values,
                                        Err(error) => {
                                            return Err(error);
                                        }
                                    };

                                let mut job_conn = rusqlite::Connection::open(&db_path_for_job)
                                    .map_err(|e| e.to_string())?;
                                let _ = job_conn.execute_batch(
                                    "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;",
                                );

                                // Accumulate the entities from every chunk in memory
                                // first; a chunk failure aborts here and leaves the
                                // previously stored entities untouched.
                                let entities = collect_ner_entities_for_chunks(
                                    &app_handle_for_job,
                                    &item_id_for_job,
                                    None,
                                    &input,
                                    api_key,
                                    model_name,
                                    prompt_template,
                                    params,
                                )
                                .await?;

                                // Swap (clear + append) in one transaction only after
                                // every chunk parsed successfully.
                                tokio::task::block_in_place(|| {
                                    ner::replace_automatic_entities_for_item(
                                        &mut job_conn,
                                        &item_id_for_job,
                                        &entities,
                                    )
                                })
                                .map_err(|e| {
                                    crate::app_logs::warn(
                                        &app_handle_for_job,
                                        "nlp",
                                        format!("ner persistencia falló para item_id={item_id_for_job}: {e}"),
                                    );
                                    e
                                })?;
                                Ok(Some(entities.len()))
                            }
                            .await;

                            // Remove from dedup set after NER completes
                            if let Ok(mut pending) = ner_pending.lock() {
                                pending.remove(&item_id);
                            }
                            match job_result {
                                Ok(entity_count) => {
                                    emit_progress(&app_handle, &item_id, "ner", 100);
                                    emit_complete_payload(
                                        &app_handle,
                                        &item_id,
                                        None,
                                        "ner",
                                        entity_count,
                                    );
                                    if entity_count.is_some() {
                                        if let Err(e) = crate::geo::enqueue_geocoding_for_item(
                                            &app_handle.state::<crate::geo::GeoQueue>(),
                                            &item_id,
                                        ) {
                                            eprintln!("[geo] Failed to auto-enqueue geocoding after NER (enrich): {e}");
                                        }
                                    }
                                }
                                Err(e) => emit_error(&app_handle, &item_id, "ner", &e),
                            }
                        }
                    }

                    // ── Asset-level processing ─────────────────────────────────────
                    // These variants process only the selected asset/page text,
                    // not the entire item. Results are stored with both item_id
                    // (for ownership/cascade) and asset_id (for filtering).
                    NlpJob::ComputeAssetEmbedding { item_id, asset_id } => {
                        eprintln!(
                            "[nlp/embeddings] EMBED job queued item_id={item_id} asset_id={asset_id}"
                        );
                        emit_asset_progress(&app_handle, &item_id, &asset_id, "embed", 10);
                        let engine = ensure_embed_engine_for_current_settings(
                            &conn,
                            &mut embed_engine,
                            &mut last_embed_engine_init_error,
                        );
                        match engine.as_deref() {
                            Some(engine) => eprintln!(
                                "[nlp/embeddings] EMBED job using provider={} item_id={item_id} asset_id={asset_id}",
                                engine.provider_name()
                            ),
                            None => eprintln!(
                                "[nlp/embeddings] EMBED job has no engine item_id={item_id} asset_id={asset_id}"
                            ),
                        }
                        let result = tokio::task::block_in_place(|| {
                            embeddings::compute_and_store_for_asset_with_unavailable_reason(
                                engine.as_deref(),
                                &conn,
                                &item_id,
                                &asset_id,
                                last_embed_engine_init_error.as_deref(),
                            )
                        });
                        if let Ok(mut pending) = embedding_pending.lock() {
                            pending.remove(&asset_id);
                        }
                        match result {
                            Ok(_) => match asset_embedding_exists(&conn, &asset_id) {
                                Ok(true) => {
                                    let provider = engine
                                        .as_deref()
                                        .map(|engine| engine.provider_name())
                                        .unwrap_or("none");
                                    eprintln!(
                                        "[nlp/embeddings] EMBED job complete provider={provider} item_id={item_id} asset_id={asset_id}"
                                    );
                                    emit_asset_progress(
                                        &app_handle,
                                        &item_id,
                                        &asset_id,
                                        "embed",
                                        100,
                                    );
                                    emit_asset_complete(&app_handle, &item_id, &asset_id, "embed");
                                }
                                Ok(false) => emit_asset_error(
                                    &app_handle,
                                    &item_id,
                                    &asset_id,
                                    "embed",
                                    "Asset embedding job completed but no vector was persisted",
                                ),
                                Err(e) => {
                                    emit_asset_error(&app_handle, &item_id, &asset_id, "embed", &e)
                                }
                            },
                            Err(e) => {
                                emit_asset_error(&app_handle, &item_id, &asset_id, "embed", &e)
                            }
                        }
                    }

                    NlpJob::ExtractEntitiesForAsset { item_id, asset_id } => {
                        emit_asset_progress(&app_handle, &item_id, &asset_id, "ner", 10);
                        let result =
                            ner::prepare_ner_candidates_for_asset(&conn, &item_id, &asset_id);
                        // Remove from asset-level dedup set so later OCR/transcription saves can refresh it.
                        if let Ok(mut pending) = asset_ner_pending.lock() {
                            pending.remove(&asset_id);
                        }
                        // Run the streaming NER-for-asset job inside an async block
                        // so we can use `?` freely without polluting the outer match
                        // arm type. Mirrors the item-level job above.
                        let item_id_for_job = item_id.clone();
                        let asset_id_for_job = asset_id.clone();
                        let app_handle_for_job = app_handle.clone();
                        let job_result: Result<Option<usize>, String> = async {
                            let input = match result {
                                Ok(input) => input,
                                Err(error) => {
                                    return Err(format!(
                                        "NER extraction for asset failed: {error}"
                                    ));
                                }
                            };
                            if input.text.trim().is_empty() {
                                crate::app_logs::warn(
                                    &app_handle_for_job,
                                    "nlp",
                                    format!("ner omitido para item_id={item_id_for_job} asset_id={asset_id_for_job}: sin texto para analizar"),
                                );
                                return Ok(None);
                            }

                            // Open a dedicated connection so the network-bound
                            // loop below does not touch the (non-Send) worker
                            // connection across await points.
                            let mut job_conn =
                                rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
                            let _ = job_conn
                                .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");

                            let config = ner_fallback_config(&job_conn).openrouter?;
                            let (api_key, model_name, prompt_template, params) = config;

                            // Accumulate the entities from every chunk in memory
                            // first; a chunk failure aborts here and leaves the
                            // previously stored entities untouched.
                            let entities = collect_ner_entities_for_chunks(
                                &app_handle_for_job,
                                &item_id_for_job,
                                Some(&asset_id_for_job),
                                &input,
                                api_key,
                                model_name,
                                prompt_template,
                                params,
                            )
                            .await?;

                            // Swap the automatic entities for this asset only
                            // (clear + append in one transaction) after every
                            // chunk parsed successfully.
                            tokio::task::block_in_place(|| {
                                ner::replace_automatic_entities_for_asset(
                                    &mut job_conn,
                                    &item_id_for_job,
                                    &asset_id_for_job,
                                    &entities,
                                )
                            })
                            .map_err(|e| {
                                crate::app_logs::warn(
                                    &app_handle_for_job,
                                    "nlp",
                                    format!("ner persistencia falló para item_id={item_id_for_job} asset_id={asset_id_for_job}: {e}"),
                                );
                                e
                            })?;
                            Ok(Some(entities.len()))
                        }
                        .await;
                        match job_result {
                            Ok(entity_count) => {
                                emit_asset_progress(&app_handle, &item_id, &asset_id, "ner", 100);
                                emit_complete_payload(
                                    &app_handle,
                                    &item_id,
                                    Some(&asset_id),
                                    "ner",
                                    entity_count,
                                );
                                if entity_count.is_some() {
                                    if let Err(e) = crate::geo::enqueue_geocoding_for_item(
                                        &app_handle.state::<crate::geo::GeoQueue>(),
                                        &item_id,
                                    ) {
                                        eprintln!("[geo] Failed to auto-enqueue geocoding after asset NER: {e}");
                                    }
                                }
                            }
                            Err(e) => emit_asset_error(&app_handle, &item_id, &asset_id, "ner", &e),
                        }
                    }
                }
            }
        });
    }
}

/// Attempt to initialize the selected embedding engine from app settings.
#[cfg(test)]
pub(crate) fn try_init_embed_engine(conn: &rusqlite::Connection) -> Option<Arc<EmbeddingEngine>> {
    try_init_embed_engine_result(conn).ok()
}

fn ensure_embed_engine_for_current_settings(
    conn: &rusqlite::Connection,
    cached: &mut Option<CachedEmbeddingEngine>,
    last_init_error: &mut Option<String>,
) -> Option<Arc<EmbeddingEngine>> {
    let config = match embeddings::config_from_settings(conn) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[nlp/embeddings] Engine init blocked: {error}");
            *cached = None;
            *last_init_error = Some(error);
            return None;
        }
    };

    let config_key = embeddings::config_cache_key(&config);
    if let Some(cached_engine) = cached.as_ref() {
        if cached_engine.config_key == config_key {
            return Some(Arc::clone(&cached_engine.engine));
        }

        eprintln!("[nlp/embeddings] Embedding settings changed; reinitializing engine");
    }

    match init_embed_engine_from_config(config) {
        Ok(engine) => {
            *last_init_error = None;
            *cached = Some(CachedEmbeddingEngine {
                config_key,
                engine: Arc::clone(&engine),
            });
            Some(engine)
        }
        Err(error) => {
            eprintln!("[nlp/embeddings] Engine unavailable: {error}");
            *cached = None;
            *last_init_error = Some(error);
            None
        }
    }
}

#[cfg(test)]
pub(crate) fn try_init_embed_engine_result(
    conn: &rusqlite::Connection,
) -> Result<Arc<EmbeddingEngine>, String> {
    let config = match embeddings::config_from_settings(conn) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[nlp/embeddings] Engine init blocked: {error}");
            return Err(error);
        }
    };

    init_embed_engine_from_config(config)
}

fn init_embed_engine_from_config(
    config: embeddings::EmbeddingConfig,
) -> Result<Arc<EmbeddingEngine>, String> {
    match EmbeddingEngine::init(config) {
        Ok(engine) => {
            eprintln!(
                "[nlp/embeddings] {} engine ready (lazy init)",
                engine.provider_name()
            );
            Ok(Arc::new(engine))
        }
        Err(e) => {
            eprintln!(
                "[nlp/embeddings] Engine init failed: {e} — embedding jobs will degrade gracefully"
            );
            Err(e)
        }
    }
}

pub(crate) fn ensure_nlp_runtime_ready(app_handle: &AppHandle) -> Result<(), String> {
    let _ = app_handle;
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Log identifier for an item-level or asset-level job. Including the asset_id
/// when present lets app_logs distinguish item NER from asset NER runs.
fn log_target(item_id: &str, asset_id: Option<&str>) -> String {
    match asset_id {
        Some(asset_id) => format!("item_id={item_id} asset_id={asset_id}"),
        None => format!("item_id={item_id}"),
    }
}

fn emit_progress(app_handle: &AppHandle, item_id: &str, job: &str, pct: u8) {
    emit_progress_payload(app_handle, item_id, None, job, pct);
}

fn emit_asset_progress(app_handle: &AppHandle, item_id: &str, asset_id: &str, job: &str, pct: u8) {
    emit_progress_payload(app_handle, item_id, Some(asset_id), job, pct);
}

fn emit_progress_payload(
    app_handle: &AppHandle,
    item_id: &str,
    asset_id: Option<&str>,
    job: &str,
    pct: u8,
) {
    if pct == 10 || pct == 100 {
        crate::app_logs::info(
            app_handle,
            "nlp",
            format!(
                "{job} {target} progreso={pct}%",
                target = log_target(item_id, asset_id)
            ),
        );
    }
    let _ = app_handle.emit(
        "nlp:progress",
        NlpProgressPayload {
            item_id: item_id.to_string(),
            asset_id: asset_id.map(str::to_string),
            job: job.to_string(),
            pct,
        },
    );
}

fn emit_complete(app_handle: &AppHandle, item_id: &str, job: &str) {
    emit_complete_payload(app_handle, item_id, None, job, None);
}

fn emit_asset_complete(app_handle: &AppHandle, item_id: &str, asset_id: &str, job: &str) {
    emit_complete_payload(app_handle, item_id, Some(asset_id), job, None);
}

fn emit_complete_payload(
    app_handle: &AppHandle,
    item_id: &str,
    asset_id: Option<&str>,
    job: &str,
    entity_count: Option<usize>,
) {
    let target = log_target(item_id, asset_id);
    let message = match entity_count {
        Some(count) => format!("{job} completado ({count} entidades) para {target}"),
        None => format!("{job} completado para {target}"),
    };
    crate::app_logs::info(app_handle, "nlp", message);
    let _ = app_handle.emit(
        "nlp:complete",
        NlpCompletePayload {
            item_id: item_id.to_string(),
            asset_id: asset_id.map(str::to_string),
            job: job.to_string(),
            entity_count,
        },
    );
}

fn emit_error(app_handle: &AppHandle, item_id: &str, job: &str, error: &str) {
    emit_error_payload(app_handle, item_id, None, job, error);
}

fn emit_asset_error(app_handle: &AppHandle, item_id: &str, asset_id: &str, job: &str, error: &str) {
    emit_error_payload(app_handle, item_id, Some(asset_id), job, error);
}

fn emit_error_payload(
    app_handle: &AppHandle,
    item_id: &str,
    asset_id: Option<&str>,
    job: &str,
    error: &str,
) {
    crate::app_logs::error(
        app_handle,
        "nlp",
        format!(
            "{job} falló para {target}: {error}",
            target = log_target(item_id, asset_id)
        ),
    );
    let _ = app_handle.emit(
        "nlp:error",
        NlpErrorPayload {
            item_id: item_id.to_string(),
            asset_id: asset_id.map(str::to_string),
            job: job.to_string(),
            error: error.to_string(),
        },
    );
}

/// Run NER for a single chunk and rebase entity offsets to the source
/// document. Returns entities with absolute (document-wide) offsets so the
/// caller can persist them as if the extraction had been a single pass.
async fn run_openrouter_ner_for_chunk(
    api_key: String,
    model_name: String,
    chunk: &chunking::TextChunk,
    protected_entities: &[ner::types::Entity],
    prompt_template: Option<String>,
    params: Option<crate::llm::openrouter::GenerationParams>,
) -> Result<Vec<ner::types::Entity>, String> {
    let mut entities = ner::openrouter::extract_entities_with_openrouter(
        api_key,
        model_name,
        &chunk.text,
        protected_entities,
        prompt_template,
        params,
    )
    .await
    .map_err(|error| format!("NER extraction failed: {error}"))?;

    if chunk.start > 0 {
        for entity in entities.iter_mut() {
            entity.start_offset = entity.start_offset.saturating_add(chunk.start);
            entity.end_offset = entity.end_offset.saturating_add(chunk.start);
        }
    }
    Ok(entities)
}

/// Fetch and parse NER entities for every chunk of `input`, accumulating the
/// results in memory. Entity lists are small, so the worker only persists once
/// the whole job succeeded: any chunk error propagates here and leaves the
/// previously stored entities untouched (write-then-swap).
// The OpenRouter call site needs every generation knob; bundling them into a
// struct would obscure the wiring with `run_openrouter_ner_for_chunk`.
#[allow(clippy::too_many_arguments)]
async fn collect_ner_entities_for_chunks(
    app_handle: &AppHandle,
    item_id: &str,
    asset_id: Option<&str>,
    input: &ner::NerExtractionInput,
    api_key: String,
    model_name: String,
    prompt_template: Option<String>,
    params: Option<crate::llm::openrouter::GenerationParams>,
) -> Result<Vec<ner::types::Entity>, String> {
    let chunks = chunking::chunk_text(&input.text);
    let total_chunks = chunks.len();
    if total_chunks > 1 {
        eprintln!(
            "[nlp/ner] text exceeded chunking threshold, splitting into {total_chunks} chunks (text_len={})",
            input.text.chars().count()
        );
    }

    let mut all_entities = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let entities = run_openrouter_ner_for_chunk(
            api_key.clone(),
            model_name.clone(),
            chunk,
            &input.protected_entities,
            prompt_template.clone(),
            params.clone(),
        )
        .await?;
        all_entities.extend(entities);

        let pct = if total_chunks <= 1 {
            90
        } else {
            10 + ((index + 1) * 80 / total_chunks)
        };
        emit_progress_payload(app_handle, item_id, asset_id, "ner", pct as u8);
    }
    Ok(all_entities)
}

struct NerFallbackConfig {
    openrouter: Result<
        (
            String,
            String,
            Option<String>,
            Option<crate::llm::openrouter::GenerationParams>,
        ),
        String,
    >,
}

fn ner_fallback_config(conn: &rusqlite::Connection) -> NerFallbackConfig {
    NerFallbackConfig {
        openrouter: ner::openrouter_settings(conn).map(|(api_key, model_name)| {
            (
                api_key,
                model_name,
                crate::settings::get_setting(conn, "prompt_ner"),
                ner::openrouter_generation_params(conn),
            )
        }),
    }
}

fn asset_embedding_exists(conn: &rusqlite::Connection, asset_id: &str) -> Result<bool, String> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM vec_assets WHERE asset_id = ?1 LIMIT 1",
            rusqlite::params![asset_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to verify persisted asset embedding: {e}"))?;

    Ok(found.is_some())
}

fn run_coalesced_fts_reindex(
    conn: &rusqlite::Connection,
    item_id: &str,
    fts_pending: &Arc<Mutex<HashMap<String, bool>>>,
) -> Result<(), String> {
    loop {
        eprintln!("[nlp/fts] Reindex start: item_id={item_id}");
        if let Err(error) = tokio::task::block_in_place(|| fts::index_item_from_db(conn, item_id)) {
            if let Ok(mut pending) = fts_pending.lock() {
                pending.remove(item_id);
            }
            return Err(error);
        }

        let should_rerun = match fts_pending.lock() {
            Ok(mut pending) => match pending.get_mut(item_id) {
                Some(needs_rerun) if *needs_rerun => {
                    *needs_rerun = false;
                    true
                }
                Some(_) => {
                    pending.remove(item_id);
                    false
                }
                None => false,
            },
            Err(_) => false,
        };

        if should_rerun {
            eprintln!(
                "[nlp/fts] Reindex rerun requested while busy: item_id={item_id} — processing latest state"
            );
            continue;
        }

        return Ok(());
    }
}

fn table_exists(conn: &rusqlite::Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
        rusqlite::params![table],
        |_| Ok(()),
    )
    .is_ok()
}

fn column_exists(conn: &rusqlite::Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("Failed to inspect {table}: {e}"))?;

    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("Failed to read {table} columns: {e}"))?;

    for existing in columns {
        if existing.map_err(|e| format!("Failed to decode column name: {e}"))? == column {
            return Ok(true);
        }
    }

    Ok(false)
}

fn ensure_entities_schema(conn: &rusqlite::Connection) -> Result<(), String> {
    if !column_exists(conn, "entities", "source")? {
        conn.execute("ALTER TABLE entities ADD COLUMN source TEXT", [])
            .map_err(|e| format!("Failed to add entities.source: {e}"))?;
    }

    if !column_exists(conn, "entities", "model_name")? {
        conn.execute("ALTER TABLE entities ADD COLUMN model_name TEXT", [])
            .map_err(|e| format!("Failed to add entities.model_name: {e}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    #[test]
    fn submit_coalesces_duplicate_fts_jobs_while_pending() {
        let (queue, mut receiver) = NlpQueue::new();

        queue
            .submit(NlpJob::IndexFts {
                item_id: "item-dup".to_string(),
            })
            .expect("first enqueue should succeed");
        queue
            .submit(NlpJob::IndexFts {
                item_id: "item-dup".to_string(),
            })
            .expect("duplicate enqueue should coalesce");

        let first = receiver.try_recv().expect("one FTS job should be queued");
        assert!(matches!(first, NlpJob::IndexFts { ref item_id } if item_id == "item-dup"));
        assert!(
            receiver.try_recv().is_err(),
            "duplicate should not queue a second FTS job"
        );
        assert_eq!(
            queue
                .fts_pending
                .lock()
                .expect("fts pending lock")
                .get("item-dup")
                .copied(),
            Some(true),
            "duplicate enqueue should mark the item for one rerun"
        );
    }

    #[test]
    fn submit_coalesces_duplicate_asset_ner_jobs_while_pending() {
        let (queue, mut receiver) = NlpQueue::new();

        queue
            .submit(NlpJob::ExtractEntitiesForAsset {
                item_id: "item-1".to_string(),
                asset_id: "asset-dup".to_string(),
            })
            .expect("first asset NER enqueue should succeed");
        queue
            .submit(NlpJob::ExtractEntitiesForAsset {
                item_id: "item-1".to_string(),
                asset_id: "asset-dup".to_string(),
            })
            .expect("duplicate asset NER enqueue should coalesce");

        let first = receiver
            .try_recv()
            .expect("one asset NER job should be queued");
        assert!(
            matches!(first, NlpJob::ExtractEntitiesForAsset { ref asset_id, .. } if asset_id == "asset-dup")
        );
        assert!(
            receiver.try_recv().is_err(),
            "duplicate should not queue a second asset NER job"
        );
        assert!(
            queue
                .asset_ner_pending
                .lock()
                .expect("asset ner pending lock")
                .contains("asset-dup"),
            "duplicate asset NER should keep one pending marker"
        );
    }

    #[test]
    fn submit_coalesces_duplicate_asset_embedding_jobs_while_pending() {
        let (queue, mut receiver) = NlpQueue::new();

        queue
            .submit(NlpJob::ComputeAssetEmbedding {
                item_id: "item-1".to_string(),
                asset_id: "asset-dup".to_string(),
            })
            .expect("first embedding enqueue should succeed");
        queue
            .submit(NlpJob::ComputeAssetEmbedding {
                item_id: "item-1".to_string(),
                asset_id: "asset-dup".to_string(),
            })
            .expect("duplicate embedding enqueue should coalesce");

        let first = receiver
            .try_recv()
            .expect("one embedding job should be queued");
        assert!(
            matches!(first, NlpJob::ComputeAssetEmbedding { ref asset_id, .. } if asset_id == "asset-dup")
        );
        assert!(
            receiver.try_recv().is_err(),
            "duplicate should not queue a second embedding job"
        );
        assert!(
            queue
                .embedding_pending
                .lock()
                .expect("embedding pending lock")
                .contains("asset-dup"),
            "duplicate embedding should keep one pending marker"
        );
    }

    fn run_job_without_events(conn: &Connection, job: &NlpJob) -> Result<(), String> {
        match job {
            NlpJob::IndexFts { item_id } => fts::index_item_from_db(conn, item_id),
            NlpJob::ExtractEntities { .. } => Ok(()),
            NlpJob::ComputeAssetEmbedding { item_id, asset_id } => {
                // No engine in test context → graceful degradation
                embeddings::compute_and_store_for_asset(None, conn, item_id, asset_id)
            }
            NlpJob::ExtractEntitiesForAsset { .. } => Ok(()),
            NlpJob::EnrichItem { item_id } => {
                // Unit-test the local part of EnrichItem without making remote OpenRouter calls.
                fts::index_item_from_db(conn, item_id)
            }
        }
    }

    fn setup_worker_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db should open");

        conn.execute_batch(
            r#"
            CREATE TABLE items (
              id TEXT PRIMARY KEY,
              collection_id TEXT,
              title TEXT NOT NULL,
              metadata TEXT
            );

            CREATE TABLE assets (
              id TEXT PRIMARY KEY,
              item_id TEXT NOT NULL,
              path TEXT NOT NULL,
              type TEXT NOT NULL,
              sort_index INTEGER NOT NULL DEFAULT 0,
              created_at INTEGER NOT NULL
            );

            CREATE TABLE extractions (
              id TEXT PRIMARY KEY,
              asset_id TEXT NOT NULL,
              text_content TEXT,
              created_at INTEGER NOT NULL
            );

            CREATE TABLE transcriptions (
              id TEXT PRIMARY KEY,
              asset_id TEXT NOT NULL,
              text_content TEXT NOT NULL,
              language TEXT,
              duration_ms INTEGER,
              model TEXT NOT NULL,
              segments TEXT,
              confidence REAL,
              created_at INTEGER NOT NULL
            );

            CREATE TABLE entities (
              id TEXT PRIMARY KEY,
              item_id TEXT NOT NULL,
              entity_type TEXT NOT NULL,
              value TEXT NOT NULL,
              start_offset INTEGER NOT NULL,
              end_offset INTEGER NOT NULL,
              confidence REAL NOT NULL,
              source TEXT,
              model_name TEXT,
              created_at INTEGER NOT NULL
            );

            CREATE TABLE triples (
              id TEXT PRIMARY KEY,
              item_id TEXT NOT NULL,
              subject TEXT NOT NULL,
              predicate TEXT NOT NULL,
              object TEXT NOT NULL,
              created_at INTEGER NOT NULL
            );

            CREATE VIRTUAL TABLE fts_items USING fts5(
              item_id UNINDEXED,
              title,
              metadata,
              extracted_text,
              content = ''
            );
            "#,
        )
        .expect("nlp worker schema should be created");

        ensure_entities_schema(&conn).expect("entities schema migration should succeed");

        conn
    }

    fn seed_item(conn: &Connection, item_id: &str, asset_id: &str, title: &str, text: &str) {
        conn.execute(
            "INSERT INTO items(id, collection_id, title, metadata) VALUES (?1, ?2, ?3, ?4)",
            params![item_id, "col-1", title, "{}"],
        )
        .expect("item should be inserted");

        conn.execute(
            "INSERT INTO assets(id, item_id, path, type, sort_index, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![asset_id, item_id, "asset.txt", "txt", 0_i64, 1_i64],
        )
        .expect("asset should be inserted");

        conn.execute(
            "INSERT INTO extractions(id, asset_id, text_content, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![format!("ext-{item_id}"), asset_id, text, 2_i64],
        )
        .expect("extraction should be inserted");
    }

    // ── EnrichItem integration tests ──────────────────────────────────────────

    #[test]
    fn enrich_item_runs_remaining_item_level_jobs() {
        let conn = setup_worker_test_db();
        seed_item(
            &conn,
            "item-enrich",
            "asset-enrich",
            "Acta Colonial",
            "Don Manuel Belgrano creó la Bandera en la ciudad de Buenos Aires.",
        );

        let result = run_job_without_events(
            &conn,
            &NlpJob::EnrichItem {
                item_id: "item-enrich".to_string(),
            },
        );
        assert!(
            result.is_ok(),
            "EnrichItem should succeed for remaining item-level jobs"
        );

        // FTS should have indexed the item
        let fts_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts_items", [], |row| row.get(0))
            .expect("fts count should be queryable");
        assert_eq!(fts_rows, 1, "FTS should index the item");

        let entity_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE item_id = ?1",
                params!["item-enrich"],
                |row| row.get(0),
            )
            .expect("entity count should be queryable");
        assert_eq!(entity_rows, 0, "test helper should not call remote NER");
    }

    #[test]
    fn enrich_item_continues_after_sub_job_failure() {
        // Run EnrichItem on an item — remaining item-level sub-jobs should still complete.
        let conn = setup_worker_test_db();
        seed_item(
            &conn,
            "item-partial",
            "asset-partial",
            "Acta Colonial",
            "Don Manuel Belgrano creó la Bandera en la ciudad de Buenos Aires.",
        );

        // Run EnrichItem — remaining item-level sub-jobs should still succeed
        let _result = run_job_without_events(
            &conn,
            &NlpJob::EnrichItem {
                item_id: "item-partial".to_string(),
            },
        );

        // FTS should still have indexed
        let fts_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts_items", [], |row| row.get(0))
            .expect("fts count should be queryable");
        assert_eq!(
            fts_rows, 1,
            "FTS should still index the item after partial failure"
        );

        let entity_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE item_id = ?1",
                params!["item-partial"],
                |row| row.get(0),
            )
            .expect("entity count should be queryable");
        assert_eq!(entity_rows, 0, "test helper should not call remote NER");
    }

    #[test]
    fn enrich_item_handles_item_with_transcription_text() {
        let conn = setup_worker_test_db();

        // Create item and asset with extraction + transcription
        conn.execute(
            "INSERT INTO items(id, collection_id, title, metadata) VALUES (?1, ?2, ?3, ?4)",
            params!["item-trans-enrich", "col-1", "Transcription Item", "{}"],
        )
        .expect("item insert");

        conn.execute(
            "INSERT INTO assets(id, item_id, path, type, sort_index, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "asset-trans-enrich",
                "item-trans-enrich",
                "audio.mp3",
                "audio",
                0_i64,
                1_i64
            ],
        )
        .expect("asset insert");

        // Transcription only
        conn.execute(
            "INSERT INTO transcriptions(id, asset_id, text_content, language, duration_ms, model, segments, confidence, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params!["trans-enrich-1", "asset-trans-enrich", "Don San Martín creó el Ejército.", "es", 5000_i64, "base", "[]", 0.9_f64, 10_i64],
        )
        .expect("transcription insert");

        let result = run_job_without_events(
            &conn,
            &NlpJob::EnrichItem {
                item_id: "item-trans-enrich".to_string(),
            },
        );
        assert!(
            result.is_ok(),
            "EnrichItem should complete for transcription-only text"
        );

        // FTS should find the transcription text
        let fts_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM fts_items", [], |row| row.get(0))
            .expect("fts count should be queryable");
        assert_eq!(
            fts_rows, 1,
            "FTS should index the item with transcription text"
        );
    }

    #[test]
    fn try_init_embed_engine_returns_none_when_openrouter_key_missing() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite should open");
        conn.execute_batch("CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .expect("settings table should be created");

        let result = try_init_embed_engine(&conn);
        assert!(
            result.is_none(),
            "Engine should be None when OpenRouter API key is missing"
        );
    }

    #[test]
    fn try_init_embed_engine_returns_some_when_openrouter_key_exists() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite should open");
        conn.execute_batch(
            "CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);\
             INSERT INTO app_settings(key, value) VALUES ('openrouter_api_key', 'sk-test');",
        )
        .expect("settings table should be created");

        let result = try_init_embed_engine(&conn);
        assert!(
            result.is_some(),
            "Engine should be Some when OpenRouter API key exists"
        );
    }

    #[test]
    fn embed_engine_retries_after_key_is_configured() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite should open");
        conn.execute_batch("CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .expect("settings table should be created");

        let first = try_init_embed_engine(&conn);
        assert!(
            first.is_none(),
            "First init should fail when OpenRouter key is unavailable"
        );

        conn.execute(
            "INSERT INTO app_settings(key, value) VALUES ('openrouter_api_key', 'sk-test')",
            [],
        )
        .expect("setting insert should succeed");

        let second = try_init_embed_engine(&conn);
        assert!(
            second.is_some(),
            "Second init should succeed after OpenRouter key is configured"
        );
    }

    #[test]
    fn try_init_embed_engine_error_rejects_local_provider_in_lite() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let db_path = temp.path().join("entropia.sqlite");
        let conn = Connection::open(&db_path).expect("sqlite file should open");
        conn.execute_batch(
            "CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);\
             INSERT INTO app_settings(key, value) VALUES ('embedding_provider', 'local');",
        )
        .expect("settings table should be created");

        let error = match try_init_embed_engine_result(&conn) {
            Ok(_) => panic!("local provider without required files should fail with diagnostics"),
            Err(error) => error,
        };

        assert!(error.contains("Proveedor de embeddings no disponible en EntropIA Lite"));
        assert!(error.contains("OpenRouter"));
    }

    #[test]
    fn cached_embed_engine_reinitializes_when_settings_change() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite should open");
        conn.execute_batch(
            "CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);\
             INSERT INTO app_settings(key, value) VALUES ('openrouter_api_key', 'sk-test');",
        )
        .expect("settings table should be created");
        let mut cached = None;
        let mut last_error = None;

        let first = ensure_embed_engine_for_current_settings(&conn, &mut cached, &mut last_error)
            .expect("api engine should initialize");
        assert_eq!(first.provider_name(), "api");

        conn.execute(
            "INSERT INTO app_settings(key, value) VALUES ('openrouter_embedding_model', 'custom/model')",
            [],
        )
        .expect("setting insert should succeed");

        let second = ensure_embed_engine_for_current_settings(&conn, &mut cached, &mut last_error)
            .expect("api engine should reinitialize after model change");

        assert!(
            !Arc::ptr_eq(&first, &second),
            "embedding engine cache must be invalidated when settings change"
        );
        assert!(last_error.is_none());
    }

    #[test]
    fn cached_embed_engine_does_not_use_stale_api_after_switching_to_invalid_local() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let db_path = temp.path().join("entropia.sqlite");
        let conn = Connection::open(&db_path).expect("sqlite file should open");
        conn.execute_batch(
            "CREATE TABLE app_settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);\
             INSERT INTO app_settings(key, value) VALUES ('openrouter_api_key', 'sk-test');",
        )
        .expect("settings table should be created");
        let mut cached = None;
        let mut last_error = None;

        let first = ensure_embed_engine_for_current_settings(&conn, &mut cached, &mut last_error)
            .expect("api engine should initialize");
        assert_eq!(first.provider_name(), "api");

        conn.execute(
            "INSERT OR REPLACE INTO app_settings(key, value) VALUES ('embedding_provider', 'local')",
            [],
        )
        .expect("provider update should succeed");

        let switched =
            ensure_embed_engine_for_current_settings(&conn, &mut cached, &mut last_error);

        assert!(
            switched.is_none(),
            "invalid local settings must not silently keep using stale API engine"
        );
        assert!(cached.is_none());
        let error = last_error.expect("local init error should be recorded");
        assert!(error.contains("Proveedor de embeddings no disponible en EntropIA Lite"));
    }
}
