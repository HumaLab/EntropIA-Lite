//! Recuperación híbrida (vector + FTS5) sobre la base de transcripciones.
//!
//! Todo el pipeline opera sobre una `&Connection` para que el comando Tauri
//! pueda envolverlo en `spawn_blocking`. El embedding de la pregunta llega
//! como parámetro — este módulo nunca toca la red, lo que mantiene cada
//! función testeable contra una base en memoria.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

use super::RagSource;
use crate::nlp::vector::{cosine_distance, decode_embedding_blob};

/// Constante de amortiguación RRF: score(asset) = Σ 1 / (RRF_K + rank).
const RRF_K: f64 = 60.0;
/// Candidatos retenidos por pierna de recuperación antes de la fusión.
const LEG_CANDIDATES: usize = 24;
/// Máximo de caracteres por snippet de fuente.
const SNIPPET_MAX_CHARS: usize = 1600;
/// Máximo total de caracteres de contexto enviados al LLM.
const CONTEXT_MAX_CHARS: usize = 10_000;
/// Longitud mínima (en chars) de un término de la pregunta para anclar snippets.
const MIN_TERM_CHARS: usize = 4;

/// Metadatos de transcripción necesarios para construir la cita de un asset.
#[derive(Debug, Clone)]
pub(crate) struct SourceRecord {
    pub asset_id: String,
    pub item_id: String,
    pub item_title: String,
    pub collection_id: String,
    pub collection_name: String,
    pub text_content: String,
    pub segments_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TranscriptSegment {
    start: f64,
    end: f64,
    text: String,
}

/// Recuperación híbrida completa: pierna vectorial (si hay embedding de la
/// pregunta) + pierna léxica, fusión RRF, snippets/timestamps y tope de
/// contexto total.
pub(crate) fn hybrid_retrieve(
    conn: &Connection,
    question: &str,
    query_embedding: Option<&[f32]>,
    top_k: usize,
) -> Result<Vec<RagSource>, String> {
    let vector = match query_embedding {
        Some(embedding) => vector_leg(conn, embedding, LEG_CANDIDATES)?,
        None => Vec::new(),
    };
    let lexical = lexical_leg(conn, question, LEG_CANDIDATES)?;
    let fused = rrf_fuse(&[vector, lexical], top_k);

    let mut records = Vec::with_capacity(fused.len());
    for (asset_id, score) in fused {
        if let Some(record) = load_source_record(conn, &asset_id)? {
            records.push((record, score));
        }
    }

    Ok(build_sources(
        records,
        question,
        SNIPPET_MAX_CHARS,
        CONTEXT_MAX_CHARS,
    ))
}

/// Pierna vectorial: kNN por similitud coseno sobre `vec_assets`, restringida
/// a assets con transcripción. Devuelve asset_ids ordenados (mejor primero).
/// Embeddings con dimensión distinta a la del query se saltean.
pub(crate) fn vector_leg(
    conn: &Connection,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT v.asset_id, v.embedding
             FROM vec_assets v
             JOIN transcriptions t ON t.asset_id = v.asset_id",
        )
        .map_err(|e| format!("Failed to prepare RAG vector query: {e}"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(|e| format!("Failed to run RAG vector query: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read RAG vector rows: {e}"))?;

    let mut scored: Vec<(String, f64)> = rows
        .into_iter()
        .filter_map(|(asset_id, blob)| {
            let embedding = decode_embedding_blob(&blob).ok()?;
            if embedding.len() != query_embedding.len() {
                return None;
            }
            let distance = cosine_distance(query_embedding, &embedding)?;
            Some((asset_id, 1.0 - distance))
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored
        .into_iter()
        .take(limit)
        .map(|(asset_id, _)| asset_id)
        .collect())
}

/// Pierna léxica: BM25 a nivel ítem vía FTS5, aplanada a los assets con
/// transcripción de cada ítem preservando el orden de relevancia. El rank
/// léxico de un asset es su posición en esta lista aplanada.
pub(crate) fn lexical_leg(
    conn: &Connection,
    question: &str,
    limit: usize,
) -> Result<Vec<String>, String> {
    let items = crate::nlp::fts::fts_search(conn, question, None)?;

    let mut stmt = conn
        .prepare(
            "SELECT a.id
             FROM assets a
             JOIN transcriptions tr ON tr.asset_id = a.id
             WHERE a.item_id = ?1
             ORDER BY a.created_at ASC, a.id ASC",
        )
        .map_err(|e| format!("Failed to prepare RAG lexical asset query: {e}"))?;

    let mut assets = Vec::new();
    for item in items.iter().take(limit) {
        let ids = stmt
            .query_map(rusqlite::params![item.item_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| format!("Failed to run RAG lexical asset query: {e}"))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| format!("Failed to read RAG lexical asset rows: {e}"))?;

        for asset_id in ids {
            assets.push(asset_id);
            if assets.len() >= limit {
                return Ok(assets);
            }
        }
    }
    Ok(assets)
}

/// Reciprocal Rank Fusion: score(asset) = Σ sobre piernas de 1/(60 + rank),
/// con rank arrancando en 1. Orden descendente por score; los empates se
/// resuelven determinísticamente por asset_id ascendente.
pub(crate) fn rrf_fuse(legs: &[Vec<String>], top_k: usize) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for leg in legs {
        for (rank0, asset_id) in leg.iter().enumerate() {
            let rank = (rank0 + 1) as f64;
            *scores.entry(asset_id.clone()).or_default() += 1.0 / (RRF_K + rank);
        }
    }

    let mut fused: Vec<(String, f64)> = scores.into_iter().collect();
    fused.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    fused.truncate(top_k);
    fused
}

/// Carga los metadatos de citación de un asset transcrito. `None` si el asset
/// ya no existe (carrera con un borrado, por ejemplo).
pub(crate) fn load_source_record(
    conn: &Connection,
    asset_id: &str,
) -> Result<Option<SourceRecord>, String> {
    conn.query_row(
        "SELECT t.asset_id, a.item_id, i.title,
                COALESCE(i.collection_id, ''), COALESCE(c.name, ''),
                t.text_content, t.segments
         FROM transcriptions t
         JOIN assets a ON a.id = t.asset_id
         JOIN items i ON i.id = a.item_id
         LEFT JOIN collections c ON c.id = i.collection_id
         WHERE t.asset_id = ?1
         LIMIT 1",
        rusqlite::params![asset_id],
        |row| {
            Ok(SourceRecord {
                asset_id: row.get(0)?,
                item_id: row.get(1)?,
                item_title: row.get(2)?,
                collection_id: row.get(3)?,
                collection_name: row.get(4)?,
                text_content: row.get(5)?,
                segments_json: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(|e| format!("Failed to load RAG source metadata for asset '{asset_id}': {e}"))
}

/// Convierte los registros fusionados (en orden) en fuentes citables con
/// snippet y timestamps, frenando cuando el contexto total supera el tope.
pub(crate) fn build_sources(
    records: Vec<(SourceRecord, f64)>,
    question: &str,
    snippet_max_chars: usize,
    context_max_chars: usize,
) -> Vec<RagSource> {
    let terms = extract_query_terms(question);
    let mut sources: Vec<RagSource> = Vec::new();
    let mut total_chars = 0usize;

    for (record, score) in records {
        let (snippet, window_start) =
            snippet_window(&record.text_content, &terms, snippet_max_chars);
        let snippet_chars = snippet.chars().count();
        if total_chars + snippet_chars > context_max_chars {
            // La lista de fuentes debe reflejar exactamente lo que entra al prompt.
            break;
        }
        total_chars += snippet_chars;

        let timestamps = resolve_timestamps(record.segments_json.as_deref(), &terms, window_start);

        sources.push(RagSource {
            index: (sources.len() + 1) as u32,
            asset_id: record.asset_id,
            item_id: record.item_id,
            item_title: record.item_title,
            collection_id: record.collection_id,
            collection_name: record.collection_name,
            snippet,
            score,
            start_seconds: timestamps.map(|(start, _)| start),
            end_seconds: timestamps.map(|(_, end)| end),
        });
    }

    sources
}

/// Términos de la pregunta para anclar snippets: split por whitespace, se
/// recorta puntuación en los bordes, lowercase, se conservan términos de
/// 4+ chars, ordenados del más largo al más corto (sin duplicados).
pub(crate) fn extract_query_terms(question: &str) -> Vec<String> {
    let mut terms: Vec<String> = question
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|word| word.chars().count() >= MIN_TERM_CHARS)
        .collect();

    // Más largo primero: el término más específico ancla la ventana.
    terms.sort_by_key(|term| std::cmp::Reverse(term.chars().count()));

    let mut seen = HashSet::new();
    terms.retain(|term| seen.insert(term.clone()));
    terms
}

/// Ventana de snippet centrada en la primera ocurrencia (case-insensitive)
/// del término más largo encontrado; si ningún término aparece, arranca al
/// inicio del texto. Opera SIEMPRE sobre chars (texto Unicode en español).
///
/// Devuelve `(snippet, índice_de_char_donde_arranca_la_ventana)`.
pub(crate) fn snippet_window(text: &str, terms: &[String], max_chars: usize) -> (String, usize) {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return (text.to_string(), 0);
    }

    // Lowercase 1:1 por char para mantener alineados los índices (suficiente
    // para español: Á→á, Ñ→ñ son todos mapeos de un char).
    let lowered: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();

    let match_pos = terms.iter().find_map(|term| find_chars(&lowered, term));

    let start = match match_pos {
        Some(pos) => pos
            .saturating_sub(max_chars / 2)
            .min(chars.len().saturating_sub(max_chars)),
        None => 0,
    };

    let snippet: String = chars[start..].iter().take(max_chars).collect();
    (snippet, start)
}

/// Busca `needle` (ya en lowercase) dentro de `haystack` (chars en lowercase).
/// Devuelve el índice de char del primer match.
fn find_chars(haystack: &[char], needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle.as_slice())
}

/// Resuelve timestamps desde el JSON de `transcriptions.segments`:
/// 1. Primer segmento cuyo texto contiene un término buscado (case-insensitive).
/// 2. Si no, el segmento que solapa el inicio de la ventana por longitud
///    acumulada de texto.
/// 3. `None` si segments es NULL, JSON inválido o vacío.
pub(crate) fn resolve_timestamps(
    segments_json: Option<&str>,
    terms: &[String],
    window_start_char: usize,
) -> Option<(f64, f64)> {
    let raw = segments_json?.trim();
    if raw.is_empty() {
        return None;
    }
    let segments: Vec<TranscriptSegment> = serde_json::from_str(raw).ok()?;
    if segments.is_empty() {
        return None;
    }

    for segment in &segments {
        let lowered = segment.text.to_lowercase();
        if terms.iter().any(|term| lowered.contains(term.as_str())) {
            return Some((segment.start, segment.end));
        }
    }

    let mut cumulative = 0usize;
    for segment in &segments {
        let len = segment.text.chars().count();
        if window_start_char < cumulative + len {
            return Some((segment.start, segment.end));
        }
        cumulative += len;
    }

    None
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn floats_to_blob(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    fn setup_rag_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory DB failed");
        conn.execute_batch(
            r#"
            CREATE TABLE collections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );

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
                created_at INTEGER NOT NULL
            );

            CREATE TABLE transcriptions (
                id TEXT PRIMARY KEY,
                asset_id TEXT UNIQUE,
                text_content TEXT NOT NULL,
                language TEXT,
                duration_ms INTEGER,
                model TEXT,
                segments TEXT,
                confidence REAL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE vec_assets (
                asset_id TEXT PRIMARY KEY,
                item_id TEXT NOT NULL,
                embedding BLOB NOT NULL
            );

            CREATE VIRTUAL TABLE fts_items USING fts5(
                item_id UNINDEXED,
                title,
                metadata,
                extracted_text,
                tokenize = 'unicode61 remove_diacritics 1',
                content = ''
            );
            "#,
        )
        .expect("RAG test schema creation failed");
        conn
    }

    fn insert_doc(
        conn: &Connection,
        collection: (&str, &str),
        item: (&str, &str),
        asset_id: &str,
        text: &str,
        segments: Option<&str>,
        embedding: Option<&[f32]>,
    ) {
        conn.execute(
            "INSERT OR IGNORE INTO collections(id, name) VALUES (?1, ?2)",
            params![collection.0, collection.1],
        )
        .expect("collection insert");
        conn.execute(
            "INSERT INTO items(id, collection_id, title, metadata) VALUES (?1, ?2, ?3, '{}')",
            params![item.0, collection.0, item.1],
        )
        .expect("item insert");
        conn.execute(
            "INSERT INTO assets(id, item_id, path, type, created_at) VALUES (?1, ?2, 'audio.mp3', 'audio', 1)",
            params![asset_id, item.0],
        )
        .expect("asset insert");
        conn.execute(
            "INSERT INTO transcriptions(id, asset_id, text_content, model, segments, created_at)
             VALUES (?1, ?2, ?3, 'base', ?4, 1)",
            params![format!("tr-{asset_id}"), asset_id, text, segments],
        )
        .expect("transcription insert");
        if let Some(embedding) = embedding {
            conn.execute(
                "INSERT INTO vec_assets(asset_id, item_id, embedding) VALUES (?1, ?2, ?3)",
                params![asset_id, item.0, floats_to_blob(embedding)],
            )
            .expect("embedding insert");
        }
        crate::nlp::fts::fts_index_item(conn, item.0, item.1, "", text).expect("fts index");
    }

    fn record(asset_id: &str, text: &str) -> SourceRecord {
        SourceRecord {
            asset_id: asset_id.to_string(),
            item_id: format!("item-{asset_id}"),
            item_title: "Título".to_string(),
            collection_id: "col".to_string(),
            collection_name: "Colección".to_string(),
            text_content: text.to_string(),
            segments_json: None,
        }
    }

    // ── RRF fusion ───────────────────────────────────────────────────────────

    #[test]
    fn rrf_fuse_asset_in_both_legs_beats_single_leg() {
        let legs = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["a".to_string(), "c".to_string()],
        ];
        let fused = rrf_fuse(&legs, 10);
        assert_eq!(fused[0].0, "a");
        let expected = 2.0 / 61.0;
        assert!((fused[0].1 - expected).abs() < 1e-12);
        // b y c quedan detrás con un solo aporte de rank 2.
        assert!(fused[1].1 < fused[0].1);
    }

    #[test]
    fn rrf_fuse_orders_by_rank_within_leg() {
        let legs = vec![vec!["a".to_string(), "b".to_string(), "c".to_string()]];
        let fused = rrf_fuse(&legs, 10);
        let ids: Vec<&str> = fused.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
        assert!(fused[0].1 > fused[1].1 && fused[1].1 > fused[2].1);
    }

    #[test]
    fn rrf_fuse_breaks_ties_deterministically_by_asset_id() {
        // Mismo rank (1) en piernas distintas → mismo score → orden por id.
        let legs = vec![vec!["zeta".to_string()], vec!["alfa".to_string()]];
        let fused = rrf_fuse(&legs, 10);
        assert_eq!(fused[0].0, "alfa");
        assert_eq!(fused[1].0, "zeta");
        assert!((fused[0].1 - fused[1].1).abs() < 1e-12);
    }

    #[test]
    fn rrf_fuse_truncates_to_top_k() {
        let legs = vec![vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ]];
        let fused = rrf_fuse(&legs, 2);
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].0, "a");
    }

    // ── Query terms ──────────────────────────────────────────────────────────

    #[test]
    fn extract_query_terms_filters_lowercases_and_sorts_longest_first() {
        let terms = extract_query_terms("¿Cuándo llegó Belgrano al Cabildo de Buenos Aires?");
        assert!(terms.contains(&"belgrano".to_string()));
        assert!(terms.contains(&"cabildo".to_string()));
        assert!(terms.contains(&"cuándo".to_string()));
        assert!(!terms.iter().any(|t| t == "de" || t == "al"));

        let lengths: Vec<usize> = terms.iter().map(|t| t.chars().count()).collect();
        let mut sorted = lengths.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(lengths, sorted, "longest terms must come first");
    }

    #[test]
    fn extract_query_terms_dedupes_repeated_words() {
        let terms = extract_query_terms("cabildo CABILDO cabildo,");
        assert_eq!(terms, vec!["cabildo".to_string()]);
    }

    // ── Snippet window ───────────────────────────────────────────────────────

    #[test]
    fn snippet_window_centers_on_term_found_mid_text() {
        let pre = "relleno ".repeat(100); // 800 chars
        let post = " cola".repeat(100);
        let text = format!("{pre}cabildo{post}");
        let terms = vec!["cabildo".to_string()];
        let (snippet, start) = snippet_window(&text, &terms, 200);
        assert!(snippet.to_lowercase().contains("cabildo"));
        assert!(start > 0, "window should not start at text begin");
        assert!(snippet.chars().count() <= 200);
    }

    #[test]
    fn snippet_window_falls_back_to_text_start_when_term_missing() {
        let text = "x".repeat(500);
        let (snippet, start) = snippet_window(&text, &["cabildo".to_string()], 100);
        assert_eq!(start, 0);
        assert_eq!(snippet.chars().count(), 100);
    }

    #[test]
    fn snippet_window_short_text_returned_whole() {
        let text = "texto corto con ñandú";
        let (snippet, start) = snippet_window(text, &[], 100);
        assert_eq!(snippet, text);
        assert_eq!(start, 0);
    }

    #[test]
    fn snippet_window_is_multibyte_safe() {
        let text = format!("{}TÉRMINO{}🦉🦉🦉", "ñ".repeat(50), "á".repeat(400));
        let terms = vec!["término".to_string()];
        let (snippet, _start) = snippet_window(&text, &terms, 80);
        assert!(snippet.chars().count() <= 80);
        assert!(snippet.to_lowercase().contains("término"));
    }

    #[test]
    fn snippet_window_term_near_end_clamps_window() {
        let text = format!("{}objetivo🦉", "relleno ".repeat(50));
        let terms = vec!["objetivo".to_string()];
        let (snippet, start) = snippet_window(&text, &terms, 100);
        assert!(snippet.to_lowercase().contains("objetivo"));
        assert_eq!(snippet.chars().count(), 100);
        assert_eq!(start, text.chars().count() - 100);
    }

    // ── Segment timestamps ───────────────────────────────────────────────────

    #[test]
    fn resolve_timestamps_finds_segment_containing_term() {
        let segments = r#"[{"start":0.0,"end":2.0,"text":"hola mundo"},{"start":2.0,"end":5.5,"text":"el Cabildo abierto"}]"#;
        let result = resolve_timestamps(Some(segments), &["cabildo".to_string()], 0);
        assert_eq!(result, Some((2.0, 5.5)));
    }

    #[test]
    fn resolve_timestamps_falls_back_to_cumulative_window_overlap() {
        let segments = r#"[{"start":0.0,"end":2.0,"text":"0123456789"},{"start":2.0,"end":4.0,"text":"abcdefghij"}]"#;
        // La ventana arranca en el char 12 → cae dentro del segundo segmento.
        let result = resolve_timestamps(Some(segments), &["zzzz".to_string()], 12);
        assert_eq!(result, Some((2.0, 4.0)));
    }

    #[test]
    fn resolve_timestamps_none_on_null_or_garbage() {
        assert_eq!(resolve_timestamps(None, &[], 0), None);
        assert_eq!(resolve_timestamps(Some("not json"), &[], 0), None);
        assert_eq!(resolve_timestamps(Some("[]"), &[], 0), None);
        assert_eq!(resolve_timestamps(Some("   "), &[], 0), None);
        assert_eq!(resolve_timestamps(Some(r#"[{"foo": 1}]"#), &[], 0), None);
    }

    #[test]
    fn resolve_timestamps_none_when_window_beyond_segments() {
        let segments = r#"[{"start":0.0,"end":2.0,"text":"corto"}]"#;
        let result = resolve_timestamps(Some(segments), &["zzzz".to_string()], 999);
        assert_eq!(result, None);
    }

    // ── build_sources ────────────────────────────────────────────────────────

    #[test]
    fn build_sources_stops_when_context_budget_is_exceeded() {
        let records = vec![
            (record("a", &"a".repeat(40)), 0.9),
            (record("b", &"b".repeat(40)), 0.8),
            (record("c", &"c".repeat(40)), 0.7),
        ];
        // 40 + 40 = 80 entra; sumar el tercero (120) supera 90 → corta.
        let sources = build_sources(records, "pregunta", 100, 90);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].index, 1);
        assert_eq!(sources[1].index, 2);
    }

    #[test]
    fn build_sources_caps_each_snippet() {
        let records = vec![(record("a", &"palabra ".repeat(100)), 1.0)];
        let sources = build_sources(records, "pregunta", 50, 1000);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].snippet.chars().count(), 50);
    }

    // ── Retrieval integration (in-memory DB) ─────────────────────────────────

    #[test]
    fn hybrid_retrieve_fuses_vector_and_lexical_legs() {
        let conn = setup_rag_db();
        let query_embedding = [1.0_f32, 0.0, 0.0];

        // En ambas piernas: texto con el término (x2) + embedding cercano.
        insert_doc(
            &conn,
            ("col-1", "Archivo General"),
            ("item-both", "Acta del Cabildo"),
            "asset-both",
            "El cabildo abierto convocó al cabildo en mayo",
            Some(r#"[{"start":1.5,"end":4.0,"text":"El cabildo abierto"}]"#),
            Some(&[0.9, 0.1, 0.0]),
        );

        // Solo vectorial: máxima similitud pero sin el término.
        insert_doc(
            &conn,
            ("col-1", "Archivo General"),
            ("item-vec", "Memoria oral"),
            "asset-vec",
            "Una memoria sobre la vida cotidiana en la aldea",
            None,
            Some(&[1.0, 0.0, 0.0]),
        );

        // Solo léxica: contiene el término una vez, sin embedding.
        insert_doc(
            &conn,
            ("col-2", "Hemeroteca"),
            ("item-fts", "Crónica de mayo"),
            "asset-fts",
            "La crónica menciona el cabildo una vez",
            None,
            None,
        );

        // Dimensión incompatible: la pierna vectorial debe saltearlo.
        insert_doc(
            &conn,
            ("col-2", "Hemeroteca"),
            ("item-bad", "Vector corrupto"),
            "asset-bad",
            "Texto sin relación alguna",
            None,
            Some(&[0.5, 0.5]),
        );

        let sources = hybrid_retrieve(&conn, "cabildo", Some(&query_embedding), 6)
            .expect("hybrid retrieval should succeed");

        let ids: Vec<&str> = sources.iter().map(|s| s.asset_id.as_str()).collect();
        assert_eq!(ids, vec!["asset-both", "asset-vec", "asset-fts"]);

        // Índices 1-based contiguos y scores RRF descendentes.
        assert_eq!(sources[0].index, 1);
        assert_eq!(sources[1].index, 2);
        assert_eq!(sources[2].index, 3);
        assert!(sources[0].score > sources[1].score);
        assert!(sources[1].score > sources[2].score);

        // Metadatos de citación.
        assert_eq!(sources[0].item_id, "item-both");
        assert_eq!(sources[0].item_title, "Acta del Cabildo");
        assert_eq!(sources[0].collection_id, "col-1");
        assert_eq!(sources[0].collection_name, "Archivo General");
        assert!(sources[0].snippet.contains("cabildo"));

        // Timestamps desde segments; None cuando no hay segments.
        assert_eq!(sources[0].start_seconds, Some(1.5));
        assert_eq!(sources[0].end_seconds, Some(4.0));
        assert_eq!(sources[1].start_seconds, None);
        assert_eq!(sources[1].end_seconds, None);
    }

    #[test]
    fn hybrid_retrieve_without_embedding_degrades_to_fts_only() {
        let conn = setup_rag_db();
        insert_doc(
            &conn,
            ("col-1", "Archivo"),
            ("item-1", "Acta"),
            "asset-fts",
            "El cabildo sesionó en pleno",
            None,
            None,
        );

        let sources =
            hybrid_retrieve(&conn, "cabildo", None, 6).expect("fts-only retrieval should work");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].asset_id, "asset-fts");
    }

    #[test]
    fn hybrid_retrieve_empty_db_returns_no_sources() {
        let conn = setup_rag_db();
        let sources = hybrid_retrieve(&conn, "cabildo", Some(&[1.0, 0.0]), 6)
            .expect("empty retrieval should succeed");
        assert!(sources.is_empty());
    }

    #[test]
    fn vector_leg_only_considers_transcribed_assets() {
        let conn = setup_rag_db();
        // Asset con embedding pero sin transcripción → fuera de la base RAG.
        conn.execute(
            "INSERT INTO vec_assets(asset_id, item_id, embedding) VALUES ('a1', 'i1', ?1)",
            params![floats_to_blob(&[1.0, 0.0])],
        )
        .expect("embedding insert");

        let ranked = vector_leg(&conn, &[1.0, 0.0], 10).expect("vector leg should succeed");
        assert!(ranked.is_empty());
    }
}
