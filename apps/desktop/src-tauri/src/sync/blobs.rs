//! Blob upload side of the sync client (DESIGN §7, PROTOCOL "Blobs" +
//! "Transformación de assets"). Covers, for the PUSH direction only (download
//! lives in a later slice):
//!
//! - `rel_path` derivation from the local absolute `assets.path` (strip the
//!   app-data-dir prefix, normalize separators to `/`, require an `assets/`
//!   prefix). Paths outside the app-data dir are rejected so the caller can skip
//!   the row and journal `apply_error`.
//! - SHA-256 hashing of the local file, cached in `sync_blob_index` and
//!   invalidated by file mtime.
//! - The asset wire transformation: the payload's absolute `path` key is OMITTED
//!   and replaced with `rel_path` + `sha256` + `size` (PROTOCOL).
//!
//! This module's surface is driven by the engine slice (next slice); here it is
//! exercised only by unit tests, so the forward-looking API carries a
//! module-level `allow(dead_code)` (removed once the engine wires it up — same
//! convention as the C1 foundations).
#![allow(dead_code)]

use std::path::Path;

use rusqlite::Connection;
use sha2::{Digest, Sha256};

/// Lowercase hex encoding of raw bytes (mirrors `audio_preview::hex_lower`; kept
/// local to avoid a cross-module dependency on a private helper).
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Why a `rel_path` derivation failed. The caller maps these to a skipped row
/// plus a journaled `apply_error` (DESIGN §7).
#[derive(Debug, PartialEq, Eq)]
pub enum RelPathError {
    /// The local path is not inside the app-data dir (e.g. an external import
    /// that was never copied in). The row must be skipped, not pushed.
    OutsideAppData,
    /// After stripping the prefix the remainder did not begin with `assets/`.
    NotUnderAssets,
    /// The path was empty.
    Empty,
}

impl std::fmt::Display for RelPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelPathError::OutsideAppData => write!(f, "asset path is outside the app-data dir"),
            RelPathError::NotUnderAssets => write!(f, "asset path is not under assets/"),
            RelPathError::Empty => write!(f, "asset path is empty"),
        }
    }
}

/// Derives the wire `rel_path` from a local absolute `assets.path` (PROTOCOL
/// "Transformación de assets"):
///
/// 1. Strip the `app_data_dir` prefix.
/// 2. Normalize separators to `/`.
/// 3. Require the remainder to start with `assets/`.
///
/// Comparison is done on the string form normalized to `/` so a Windows
/// backslash path matches a forward-slash app-data dir. Rows whose path is
/// outside the app-data dir return [`RelPathError::OutsideAppData`] so the
/// caller skips + journals them (DESIGN §7).
pub fn derive_rel_path(abs_path: &str, app_data_dir: &Path) -> Result<String, RelPathError> {
    if abs_path.trim().is_empty() {
        return Err(RelPathError::Empty);
    }

    let normalize = |s: &str| s.replace('\\', "/");
    let path_norm = normalize(abs_path);
    let mut prefix_norm = normalize(&app_data_dir.to_string_lossy());
    if !prefix_norm.ends_with('/') {
        prefix_norm.push('/');
    }

    // Case-insensitive prefix match on Windows (drive letters/paths are
    // case-insensitive there); exact elsewhere.
    let starts_with_prefix = if cfg!(windows) {
        path_norm
            .to_ascii_lowercase()
            .starts_with(&prefix_norm.to_ascii_lowercase())
    } else {
        path_norm.starts_with(&prefix_norm)
    };
    if !starts_with_prefix {
        return Err(RelPathError::OutsideAppData);
    }

    // Slice off the matched prefix length from the ORIGINAL-normalized path so
    // the casing of the remainder (the assets/ subtree) is preserved verbatim.
    let rel = &path_norm[prefix_norm.len()..];
    let rel = rel.trim_start_matches('/');

    if !rel.starts_with("assets/") {
        return Err(RelPathError::NotUnderAssets);
    }

    Ok(rel.to_string())
}

/// Result of resolving a blob's hash/size for push, after consulting (and
/// refreshing) the `sync_blob_index` mtime cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobDigest {
    pub sha256: String,
    pub size: i64,
    /// Whether the cached entry was reused without re-hashing the file.
    pub from_cache: bool,
}

/// Returns the file's mtime in ms since the Unix epoch, or `0` if unavailable.
fn file_mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Hashes `path` with SHA-256 and returns the lowercase hex digest + byte size.
/// Streams the file in chunks so large blobs do not load fully into memory.
pub fn hash_file(path: &Path) -> Result<(String, i64), String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("[sync] failed to open blob {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: i64 = 0;
    loop {
        let read = std::io::Read::read(&mut file, &mut buf)
            .map_err(|e| format!("[sync] failed to read blob {}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
        total += read as i64;
    }
    Ok((hex_lower(&hasher.finalize()), total))
}

/// Resolves the SHA-256 + size for an asset's file, using the `sync_blob_index`
/// mtime cache (DESIGN §7). When the cached `file_mtime_ms` matches the file's
/// current mtime the cached hash is trusted; otherwise the file is re-hashed and
/// the cache row is upserted with `uploaded` left untouched on a cache hit and
/// reset to `0` on a re-hash (a changed file means the old blob is stale).
pub fn resolve_blob_digest(
    conn: &Connection,
    asset_id: &str,
    abs_path: &Path,
) -> Result<BlobDigest, String> {
    let meta = std::fs::metadata(abs_path)
        .map_err(|e| format!("[sync] failed to stat blob {}: {e}", abs_path.display()))?;
    let mtime = file_mtime_ms(&meta);

    let cached: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT sha256, size, file_mtime_ms FROM sync_blob_index WHERE asset_id = ?1",
            [asset_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    if let Some((sha256, size, cached_mtime)) = cached {
        if cached_mtime == mtime {
            return Ok(BlobDigest {
                sha256,
                size,
                from_cache: true,
            });
        }
    }

    // Cache miss or stale mtime: re-hash and refresh the cache. A re-hash means
    // the file changed under us, so the previously-uploaded blob no longer
    // matches — reset uploaded=0 to force a fresh HEAD/PUT (DESIGN §7).
    let (sha256, size) = hash_file(abs_path)?;
    conn.execute(
        "INSERT INTO sync_blob_index(asset_id, sha256, size, file_mtime_ms, uploaded)
         VALUES (?1, ?2, ?3, ?4, 0)
         ON CONFLICT(asset_id) DO UPDATE SET
           sha256 = excluded.sha256,
           size = excluded.size,
           file_mtime_ms = excluded.file_mtime_ms,
           uploaded = 0",
        rusqlite::params![asset_id, sha256, size, mtime],
    )
    .map_err(|e| format!("[sync] failed to update blob index for {asset_id}: {e}"))?;

    Ok(BlobDigest {
        sha256,
        size,
        from_cache: false,
    })
}

/// Reads the `uploaded` flag for an asset from `sync_blob_index` (DESIGN §6.3).
/// `uploaded=1` is only trusted after a HEAD re-confirm before a row push.
#[allow(dead_code)]
pub fn blob_uploaded(conn: &Connection, asset_id: &str) -> Result<bool, String> {
    let flag: Option<i64> = conn
        .query_row(
            "SELECT uploaded FROM sync_blob_index WHERE asset_id = ?1",
            [asset_id],
            |row| row.get(0),
        )
        .ok();
    Ok(flag == Some(1))
}

/// Marks an asset's blob as uploaded (`uploaded=1`) after a confirmed PUT/HEAD.
#[allow(dead_code)]
pub fn mark_blob_uploaded(conn: &Connection, asset_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE sync_blob_index SET uploaded = 1 WHERE asset_id = ?1",
        [asset_id],
    )
    .map(|_| ())
    .map_err(|e| format!("[sync] failed to mark blob uploaded for {asset_id}: {e}"))
}

/// Resets an asset's blob `uploaded` flag to `0` (DESIGN §6.3): used when a HEAD
/// returns 404 for a blob we believed was uploaded (server restore), forcing a
/// re-PUT.
#[allow(dead_code)]
pub fn reset_blob_uploaded(conn: &Connection, asset_id: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE sync_blob_index SET uploaded = 0 WHERE asset_id = ?1",
        [asset_id],
    )
    .map(|_| ())
    .map_err(|e| format!("[sync] failed to reset blob uploaded for {asset_id}: {e}"))
}

/// Transforms a raw `assets` row payload (read from the local table) into the
/// wire shape (PROTOCOL "Transformación de assets"): the absolute `path` key is
/// REMOVED and replaced with `rel_path` + `sha256` + `size`. All other keys
/// (id, item_id, type, sort_index, created_at, …) are preserved.
///
/// `size` always reflects the actual file size from the hash pass, overriding
/// any stale `assets.size` column value.
pub fn asset_payload_to_wire(
    mut payload: serde_json::Value,
    rel_path: &str,
    sha256: &str,
    size: i64,
) -> serde_json::Value {
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("path");
        obj.insert(
            "rel_path".to_string(),
            serde_json::Value::String(rel_path.to_string()),
        );
        obj.insert(
            "sha256".to_string(),
            serde_json::Value::String(sha256.to_string()),
        );
        obj.insert(
            "size".to_string(),
            serde_json::Value::Number(serde_json::Number::from(size)),
        );
    }
    payload
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::test_support::new_synced_test_db;
    use std::path::PathBuf;

    fn app_dir() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\ana\AppData\Roaming\com.entropia.lite")
        } else {
            PathBuf::from("/home/ana/.local/share/com.entropia.lite")
        }
    }

    fn abs(rel: &str) -> String {
        let mut p = app_dir().to_string_lossy().to_string();
        if cfg!(windows) {
            p.push('\\');
            p.push_str(&rel.replace('/', "\\"));
        } else {
            p.push('/');
            p.push_str(rel);
        }
        p
    }

    #[test]
    fn derive_rel_path_strips_prefix_and_normalizes() {
        let p = abs("assets/col-1/item-1/uuid_foto.png");
        let rel = derive_rel_path(&p, &app_dir()).expect("derive");
        assert_eq!(rel, "assets/col-1/item-1/uuid_foto.png");
    }

    #[test]
    fn derive_rel_path_handles_unicode_names() {
        let p = abs("assets/col-1/item-1/uuid_documentó_ñ.png");
        let rel = derive_rel_path(&p, &app_dir()).expect("derive");
        assert_eq!(rel, "assets/col-1/item-1/uuid_documentó_ñ.png");
    }

    #[test]
    fn derive_rel_path_rejects_outside_app_data() {
        let outside = if cfg!(windows) {
            r"D:\elsewhere\assets\x.png"
        } else {
            "/tmp/elsewhere/assets/x.png"
        };
        assert_eq!(
            derive_rel_path(outside, &app_dir()),
            Err(RelPathError::OutsideAppData)
        );
    }

    #[test]
    fn derive_rel_path_rejects_non_assets_subtree() {
        // Inside the app dir but not under assets/ (e.g. logs/).
        let p = abs("logs/entropia.log");
        assert_eq!(
            derive_rel_path(&p, &app_dir()),
            Err(RelPathError::NotUnderAssets)
        );
    }

    #[test]
    fn derive_rel_path_rejects_empty() {
        assert_eq!(derive_rel_path("", &app_dir()), Err(RelPathError::Empty));
        assert_eq!(derive_rel_path("   ", &app_dir()), Err(RelPathError::Empty));
    }

    #[test]
    fn hash_file_matches_known_sha256() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("blob.bin");
        std::fs::write(&file, b"hello").expect("write");
        let (sha, size) = hash_file(&file).expect("hash");
        // sha256("hello")
        assert_eq!(
            sha,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(size, 5);
    }

    #[test]
    fn resolve_blob_digest_uses_cache_then_invalidates_on_mtime() {
        let conn = new_synced_test_db();
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.bin");
        std::fs::write(&file, b"hello").expect("write");

        // First call: cache miss → hashes and stores uploaded=0.
        let first = resolve_blob_digest(&conn, "asset-1", &file).expect("first");
        assert!(!first.from_cache);
        assert_eq!(
            first.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );

        // Pretend the blob got uploaded.
        mark_blob_uploaded(&conn, "asset-1").expect("mark uploaded");
        assert!(blob_uploaded(&conn, "asset-1").unwrap());

        // Second call without touching the file: cache HIT (mtime unchanged),
        // uploaded flag preserved.
        let second = resolve_blob_digest(&conn, "asset-1", &file).expect("second");
        assert!(second.from_cache, "unchanged mtime should hit cache");
        assert_eq!(second.sha256, first.sha256);
        assert!(
            blob_uploaded(&conn, "asset-1").unwrap(),
            "cache hit must not reset uploaded"
        );

        // Change the file content AND its mtime: cache must invalidate, re-hash,
        // and reset uploaded=0.
        std::thread::sleep(std::time::Duration::from_millis(15));
        std::fs::write(&file, b"world!!").expect("rewrite");
        // Force a newer mtime explicitly so the test is robust on coarse clocks.
        let new_mtime = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        filetime_set(&file, new_mtime);

        let third = resolve_blob_digest(&conn, "asset-1", &file).expect("third");
        assert!(!third.from_cache, "changed mtime must miss cache");
        assert_ne!(third.sha256, first.sha256, "content changed → new hash");
        assert!(
            !blob_uploaded(&conn, "asset-1").unwrap(),
            "re-hash must reset uploaded"
        );
    }

    /// Sets a file's mtime using only std (no `filetime` crate): re-open and use
    /// `set_modified` (stable since Rust 1.75 via `File::set_modified`).
    fn filetime_set(path: &Path, when: std::time::SystemTime) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open for mtime");
        file.set_modified(when).expect("set mtime");
    }

    #[test]
    fn asset_payload_to_wire_omits_path_and_adds_integrity() {
        let payload = serde_json::json!({
            "id": "a1",
            "item_id": "i1",
            "path": "/abs/local/assets/c/i/x.png",
            "type": "image",
            "size": 0,
            "sort_index": 2,
            "created_at": 123
        });
        let wire = asset_payload_to_wire(payload, "assets/c/i/x.png", "deadbeef", 456);
        let obj = wire.as_object().expect("object");
        assert!(!obj.contains_key("path"), "absolute path must be omitted");
        assert_eq!(obj["rel_path"], "assets/c/i/x.png");
        assert_eq!(obj["sha256"], "deadbeef");
        assert_eq!(obj["size"], 456, "size reflects the real file size");
        // Untouched keys survive.
        assert_eq!(obj["id"], "a1");
        assert_eq!(obj["item_id"], "i1");
        assert_eq!(obj["type"], "image");
        assert_eq!(obj["sort_index"], 2);
    }

    #[test]
    fn reset_blob_uploaded_clears_flag() {
        let conn = new_synced_test_db();
        conn.execute(
            "INSERT INTO sync_blob_index(asset_id,sha256,size,file_mtime_ms,uploaded)
             VALUES('a1','h',1,1,1)",
            [],
        )
        .expect("seed");
        assert!(blob_uploaded(&conn, "a1").unwrap());
        reset_blob_uploaded(&conn, "a1").expect("reset");
        assert!(!blob_uploaded(&conn, "a1").unwrap());
    }
}
