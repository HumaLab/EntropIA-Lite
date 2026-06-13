//! HTTP transport for the cloud-sync client (PROTOCOL.md). Defines the
//! [`SyncApi`] trait (the wire contract the engine and push/pull paths consume)
//! plus the [`HttpSyncApi`] reqwest implementation and the request/response
//! DTOs. The trait is generic-friendly so the push/blob slices can be tested
//! against an in-memory mock without spinning up a server.
//!
//! House rules enforced here (DESIGN §8, PROTOCOL "Transporte"):
//! - TLS is mandatory: a non-`https` `server_url` is rejected unless the host is
//!   `127.0.0.1` / `::1` / `localhost`. Validated at config-time
//!   ([`validate_server_url`]) AND at use-time (inside [`HttpSyncApi::new`]).
//! - The Bearer token and the `X-Schema-Tag` header are attached to every
//!   authenticated request; the token is NEVER logged (DESIGN §8).
//!
//! Several DTO fields and trait methods (devices, revoke, usage, delete_account,
//! pull, blob_get, and the pull-response cursor fields) are consumed by the
//! engine and pull/apply slices (next slices); here they exist for the wire
//! contract and are only partially exercised. The module-level
//! `allow(dead_code)` is removed once those slices wire them up (same convention
//! as the C1 foundations).
#![allow(dead_code)]

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Connect timeout for every request (house style mirrors the AssemblyAI/GLM
/// clients which cap connection setup so a stalled socket cannot wedge a worker).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Default per-request timeout for the small JSON endpoints.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Generous timeout for blob HEAD/PUT/GET, which can move large files.
const BLOB_TIMEOUT: Duration = Duration::from_secs(600);

/// HTTP header carrying the local migration head (PROTOCOL "schema_tag").
const SCHEMA_TAG_HEADER: &str = "X-Schema-Tag";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Uniform error type for the sync transport. Surfaced as `String` at the Tauri
/// command boundary (house rule: commands return `Result<T, String>`), but kept
/// structured internally so the engine can branch on `Api { status, .. }` codes
/// (e.g. 426 → "update the app", 507 → "server storage full").
#[derive(Debug)]
pub enum SyncError {
    /// `server_url` failed the TLS rule (non-https to a non-loopback host) or is
    /// otherwise malformed.
    InvalidUrl(String),
    /// Transport-level failure (DNS, connect, timeout, TLS handshake, broken
    /// stream). Maps to the engine's `offline` state with backoff.
    Network(String),
    /// A structured `{ "error": { code, message } }` body with its HTTP status.
    Api {
        status: u16,
        code: String,
        message: String,
    },
    /// The body could not be (de)serialized as expected.
    Decode(String),
}

impl SyncError {
    /// The HTTP status if this is an [`SyncError::Api`], else `None`. Used by the
    /// engine to branch on 4xx codes without re-parsing the message.
    #[allow(dead_code)]
    pub fn status(&self) -> Option<u16> {
        match self {
            SyncError::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// The stable error code if this is an [`SyncError::Api`], else `None`.
    #[allow(dead_code)]
    pub fn api_code(&self) -> Option<&str> {
        match self {
            SyncError::Api { code, .. } => Some(code),
            _ => None,
        }
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncError::InvalidUrl(m) => write!(f, "invalid server url: {m}"),
            SyncError::Network(m) => write!(f, "network error: {m}"),
            SyncError::Api {
                status,
                code,
                message,
            } => write!(f, "api error {status} ({code}): {message}"),
            SyncError::Decode(m) => write!(f, "decode error: {m}"),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<SyncError> for String {
    fn from(error: SyncError) -> Self {
        error.to_string()
    }
}

/// The wire error envelope (`{ "error": { "code", "message" } }`, PROTOCOL).
#[derive(Debug, Deserialize)]
struct WireError {
    error: Option<WireErrorBody>,
}

#[derive(Debug, Deserialize)]
struct WireErrorBody {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

// ---------------------------------------------------------------------------
// TLS / URL validation (PROTOCOL "Transporte", DESIGN §8)
// ---------------------------------------------------------------------------

/// Returns true when `host` is a loopback address for which plain `http://` is
/// tolerated (PROTOCOL "Transporte"). Everything else demands TLS.
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "::1" | "localhost" | "[::1]")
}

/// Validates a `server_url` against the TLS rule. `https` is always accepted;
/// `http` only for loopback hosts. Returns the trimmed, trailing-slash-stripped
/// base URL on success. Called at config-time and at use-time so a stored URL
/// can never be used to ship credentials over cleartext.
pub fn validate_server_url(url: &str) -> Result<String, SyncError> {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(SyncError::InvalidUrl("empty server url".to_string()));
    }

    let (scheme, rest) = trimmed
        .split_once("://")
        .ok_or_else(|| SyncError::InvalidUrl(format!("missing scheme in '{trimmed}'")))?;

    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(SyncError::InvalidUrl(format!(
            "unsupported scheme '{scheme}' (only http/https)"
        )));
    }

    // Authority is everything up to the first '/', '?' or '#'.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("").to_string();
    if authority.is_empty() {
        return Err(SyncError::InvalidUrl(format!(
            "missing host in '{trimmed}'"
        )));
    }

    // Strip optional userinfo and port to isolate the host for the loopback check.
    let host_port = authority.rsplit('@').next().unwrap_or(&authority);
    let host = if let Some(stripped) = host_port.strip_prefix('[') {
        // Bracketed IPv6 literal, e.g. [::1]:8787 → keep the brackets so the
        // loopback check matches "[::1]".
        match stripped.split_once(']') {
            Some((inner, _)) => format!("[{inner}]"),
            None => host_port.to_string(),
        }
    } else {
        host_port
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(host_port)
            .to_string()
    };
    let host_for_match = host.trim_start_matches('[').trim_end_matches(']');

    if scheme == "http" && !is_loopback_host(&host) && !is_loopback_host(host_for_match) {
        return Err(SyncError::InvalidUrl(format!(
            "refusing http:// to non-loopback host '{host}' — TLS is mandatory"
        )));
    }

    Ok(trimmed.to_string())
}

// ---------------------------------------------------------------------------
// DTOs (PROTOCOL "Endpoints"). All response structs tolerate unknown fields by
// default (serde drops them) so additive `/v1` changes don't break the client.
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterResponse {
    pub account_id: String,
}

#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub device_name: String,
    pub platform: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub account_id: String,
    pub device_id: String,
    pub device_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub last_seen_at: i64,
    #[serde(default)]
    pub revoked: bool,
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Deserialize)]
pub struct DevicesResponse {
    #[serde(default)]
    pub devices: Vec<DeviceInfo>,
}

#[derive(Debug, Serialize)]
pub struct DeleteAccountRequest {
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct UsageResponse {
    #[serde(default)]
    pub rows: i64,
    #[serde(default)]
    pub blobs_count: i64,
    #[serde(default)]
    pub blobs_bytes: i64,
    #[serde(default)]
    pub quota_bytes: i64,
}

/// One outbound change in a push batch (PROTOCOL `POST /v1/sync/push`).
#[derive(Debug, Clone, Serialize)]
pub struct PushChange {
    pub table: String,
    pub row_id: String,
    /// `"upsert"` | `"delete"`.
    pub op: String,
    /// ms, already corrected by the clock offset (PROTOCOL "Reloj").
    pub changed_at: i64,
    /// Last server version seen of this row; `0` = never seen.
    pub base_seq: i64,
    /// Row payload as a JSON object for `upsert`; `None` (null) for `delete`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct PushRequest {
    pub changes: Vec<PushChange>,
}

/// One per-row result in a push response (PROTOCOL). Note the result schema has
/// no per-row error state by design: every well-formed change resolves.
#[derive(Debug, Clone, Deserialize)]
pub struct PushResult {
    pub table: String,
    pub row_id: String,
    /// `"applied"` | `"lww_won"` | `"lww_lost"`.
    pub status: String,
    pub server_seq: i64,
    /// On `lww_lost`: the winning row in pull format; else `None`.
    #[serde(default)]
    pub winner: Option<PullRow>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushResponse {
    #[serde(default)]
    pub results: Vec<PushResult>,
    #[serde(default)]
    pub max_server_seq: i64,
    #[serde(default)]
    pub server_epoch: String,
    #[serde(default)]
    pub server_now_ms: i64,
}

/// One row returned by a pull page (PROTOCOL `GET /v1/sync/pull`). Also reused
/// as the `winner` payload in `lww_lost` push results.
#[derive(Debug, Clone, Deserialize)]
pub struct PullRow {
    pub table: String,
    pub row_id: String,
    pub server_seq: i64,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub changed_at: i64,
    #[serde(default)]
    pub device_id: String,
    /// Row payload as a JSON object; `null` when `deleted`.
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullResponse {
    #[serde(default)]
    pub rows: Vec<PullRow>,
    #[serde(default)]
    pub next_since: i64,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub schema_tag: String,
    #[serde(default)]
    pub server_epoch: String,
    #[serde(default)]
    pub server_now_ms: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HealthLimits {
    #[serde(default)]
    pub max_push_bytes: i64,
    #[serde(default)]
    pub max_blob_mb: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HealthResponse {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub epoch: String,
    #[serde(default)]
    pub server_now_ms: i64,
    #[serde(default)]
    pub limits: HealthLimits,
}

/// Result of a blob HEAD probe: `true` when the server already holds the blob.
pub type BlobExists = bool;

// ---------------------------------------------------------------------------
// SyncApi trait
// ---------------------------------------------------------------------------

/// The wire contract between the client and `entropia-sync-server` (PROTOCOL).
///
/// Methods are `async`; callers drive them with `tauri::async_runtime::block_on`
/// inside the engine task (mirrors the existing OCR/transcription workers). The
/// trait is consumed generically (`<A: SyncApi>`) so the push/blob paths are
/// testable against an in-memory mock — no `dyn` / `async-trait` dependency.
///
/// Auth: every method except [`SyncApi::register`], [`SyncApi::login`] and
/// [`SyncApi::health`] attaches the device Bearer token. The `/sync/*` methods
/// also attach `X-Schema-Tag`.
#[allow(dead_code)]
pub trait SyncApi {
    fn register(
        &self,
        req: RegisterRequest,
    ) -> impl std::future::Future<Output = Result<RegisterResponse, SyncError>> + Send;

    fn login(
        &self,
        req: LoginRequest,
    ) -> impl std::future::Future<Output = Result<LoginResponse, SyncError>> + Send;

    fn logout(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;

    fn devices(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<DevicesResponse, SyncError>> + Send;

    fn revoke(
        &self,
        token: &str,
        device_id: &str,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;

    fn delete_account(
        &self,
        token: &str,
        req: DeleteAccountRequest,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;

    fn usage(
        &self,
        token: &str,
    ) -> impl std::future::Future<Output = Result<UsageResponse, SyncError>> + Send;

    fn health(&self)
        -> impl std::future::Future<Output = Result<HealthResponse, SyncError>> + Send;

    fn push(
        &self,
        token: &str,
        schema_tag: &str,
        req: PushRequest,
    ) -> impl std::future::Future<Output = Result<PushResponse, SyncError>> + Send;

    fn pull(
        &self,
        token: &str,
        schema_tag: &str,
        since: i64,
        limit: i64,
    ) -> impl std::future::Future<Output = Result<PullResponse, SyncError>> + Send;

    fn blob_head(
        &self,
        token: &str,
        sha256: &str,
    ) -> impl std::future::Future<Output = Result<BlobExists, SyncError>> + Send;

    fn blob_put(
        &self,
        token: &str,
        sha256: &str,
        bytes: Vec<u8>,
    ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send;

    fn blob_get(
        &self,
        token: &str,
        sha256: &str,
    ) -> impl std::future::Future<Output = Result<reqwest::Response, SyncError>> + Send;
}

// ---------------------------------------------------------------------------
// HttpSyncApi (reqwest implementation)
// ---------------------------------------------------------------------------

/// reqwest-backed [`SyncApi`]. Holds a validated base URL and a client with the
/// house-style timeouts. Blob ops override the per-request timeout with the
/// longer [`BLOB_TIMEOUT`].
pub struct HttpSyncApi {
    base_url: String,
    client: reqwest::Client,
}

impl HttpSyncApi {
    /// Builds a client for `server_url`. Re-validates the TLS rule at use-time
    /// (PROTOCOL "Transporte") so a stored URL can never be used over cleartext.
    pub fn new(server_url: &str) -> Result<Self, SyncError> {
        let base_url = validate_server_url(server_url)?;
        let client = reqwest::Client::builder()
            .user_agent("EntropIA-Desktop-Sync/1.0")
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| SyncError::Network(format!("failed to build HTTP client: {e}")))?;
        Ok(Self { base_url, client })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Maps a transport error into [`SyncError::Network`], distinguishing it from
    /// API-level errors which come from the response body.
    fn network_err(context: &str, error: reqwest::Error) -> SyncError {
        SyncError::Network(format!("{context}: {error}"))
    }
}

/// Inspects a response: on a 2xx returns it untouched; otherwise reads the body,
/// parses the `{ error: { code, message } }` envelope, and returns
/// [`SyncError::Api`]. NEVER logs the request — callers must not pass tokens here.
async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, SyncError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let code_num = status.as_u16();
    let body = response.text().await.unwrap_or_default();
    let (code, message) = match serde_json::from_str::<WireError>(&body) {
        Ok(WireError { error: Some(body) }) => (body.code, body.message),
        _ => (
            status
                .canonical_reason()
                .unwrap_or("unknown")
                .to_ascii_lowercase()
                .replace(' ', "_"),
            if body.is_empty() {
                status.to_string()
            } else {
                body
            },
        ),
    };
    Err(SyncError::Api {
        status: code_num,
        code,
        message,
    })
}

async fn parse_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, SyncError> {
    let response = ensure_success(response).await?;
    response
        .json::<T>()
        .await
        .map_err(|e| SyncError::Decode(format!("failed to parse response body: {e}")))
}

impl SyncApi for HttpSyncApi {
    async fn register(&self, req: RegisterRequest) -> Result<RegisterResponse, SyncError> {
        let response = self
            .client
            .post(self.url("/v1/auth/register"))
            .json(&req)
            .send()
            .await
            .map_err(|e| Self::network_err("register request", e))?;
        parse_json(response).await
    }

    async fn login(&self, req: LoginRequest) -> Result<LoginResponse, SyncError> {
        let response = self
            .client
            .post(self.url("/v1/auth/login"))
            .json(&req)
            .send()
            .await
            .map_err(|e| Self::network_err("login request", e))?;
        parse_json(response).await
    }

    async fn logout(&self, token: &str) -> Result<(), SyncError> {
        let response = self
            .client
            .post(self.url("/v1/auth/logout"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| Self::network_err("logout request", e))?;
        ensure_success(response).await.map(|_| ())
    }

    async fn devices(&self, token: &str) -> Result<DevicesResponse, SyncError> {
        let response = self
            .client
            .get(self.url("/v1/devices"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| Self::network_err("devices request", e))?;
        parse_json(response).await
    }

    async fn revoke(&self, token: &str, device_id: &str) -> Result<(), SyncError> {
        let response = self
            .client
            .delete(self.url(&format!("/v1/devices/{device_id}")))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| Self::network_err("revoke request", e))?;
        ensure_success(response).await.map(|_| ())
    }

    async fn delete_account(
        &self,
        token: &str,
        req: DeleteAccountRequest,
    ) -> Result<(), SyncError> {
        let response = self
            .client
            .delete(self.url("/v1/account"))
            .bearer_auth(token)
            .json(&req)
            .send()
            .await
            .map_err(|e| Self::network_err("delete account request", e))?;
        ensure_success(response).await.map(|_| ())
    }

    async fn usage(&self, token: &str) -> Result<UsageResponse, SyncError> {
        let response = self
            .client
            .get(self.url("/v1/usage"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| Self::network_err("usage request", e))?;
        parse_json(response).await
    }

    async fn health(&self) -> Result<HealthResponse, SyncError> {
        let response = self
            .client
            .get(self.url("/v1/health"))
            .send()
            .await
            .map_err(|e| Self::network_err("health request", e))?;
        parse_json(response).await
    }

    async fn push(
        &self,
        token: &str,
        schema_tag: &str,
        req: PushRequest,
    ) -> Result<PushResponse, SyncError> {
        let response = self
            .client
            .post(self.url("/v1/sync/push"))
            .bearer_auth(token)
            .header(SCHEMA_TAG_HEADER, schema_tag)
            .json(&req)
            .send()
            .await
            .map_err(|e| Self::network_err("push request", e))?;
        parse_json(response).await
    }

    async fn pull(
        &self,
        token: &str,
        schema_tag: &str,
        since: i64,
        limit: i64,
    ) -> Result<PullResponse, SyncError> {
        let response = self
            .client
            .get(self.url("/v1/sync/pull"))
            .bearer_auth(token)
            .header(SCHEMA_TAG_HEADER, schema_tag)
            .query(&[("since", since), ("limit", limit)])
            .send()
            .await
            .map_err(|e| Self::network_err("pull request", e))?;
        parse_json(response).await
    }

    async fn blob_head(&self, token: &str, sha256: &str) -> Result<BlobExists, SyncError> {
        let response = self
            .client
            .head(self.url(&format!("/v1/blobs/{sha256}")))
            .bearer_auth(token)
            .timeout(BLOB_TIMEOUT)
            .send()
            .await
            .map_err(|e| Self::network_err("blob head request", e))?;
        let status = response.status();
        if status.is_success() {
            Ok(true)
        } else if status == reqwest::StatusCode::NOT_FOUND {
            Ok(false)
        } else {
            Err(ensure_success(response).await.unwrap_err())
        }
    }

    async fn blob_put(&self, token: &str, sha256: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
        let response = self
            .client
            .put(self.url(&format!("/v1/blobs/{sha256}")))
            .bearer_auth(token)
            .header("Content-Type", "application/octet-stream")
            .timeout(BLOB_TIMEOUT)
            .body(bytes)
            .send()
            .await
            .map_err(|e| Self::network_err("blob put request", e))?;
        ensure_success(response).await.map(|_| ())
    }

    async fn blob_get(&self, token: &str, sha256: &str) -> Result<reqwest::Response, SyncError> {
        let response = self
            .client
            .get(self.url(&format!("/v1/blobs/{sha256}")))
            .bearer_auth(token)
            .timeout(BLOB_TIMEOUT)
            .send()
            .await
            .map_err(|e| Self::network_err("blob get request", e))?;
        ensure_success(response).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_server_url_accepts_https_anywhere() {
        assert_eq!(
            validate_server_url("https://sync.example.com").unwrap(),
            "https://sync.example.com"
        );
        assert_eq!(
            validate_server_url("https://sync.example.com:8443/").unwrap(),
            "https://sync.example.com:8443"
        );
    }

    #[test]
    fn validate_server_url_accepts_http_only_for_loopback() {
        for url in [
            "http://127.0.0.1:8787",
            "http://localhost:8787",
            "http://[::1]:8787",
            "http://localhost",
        ] {
            assert!(validate_server_url(url).is_ok(), "should accept {url}");
        }
    }

    #[test]
    fn validate_server_url_rejects_http_to_remote_host() {
        for url in [
            "http://sync.example.com",
            "http://192.168.1.10:8787",
            "http://example.com/v1",
            "http://127.0.0.1.evil.com",
        ] {
            let err = validate_server_url(url).unwrap_err();
            assert!(
                matches!(err, SyncError::InvalidUrl(_)),
                "should reject {url}, got {err:?}"
            );
        }
    }

    #[test]
    fn validate_server_url_rejects_malformed() {
        assert!(validate_server_url("").is_err());
        assert!(validate_server_url("   ").is_err());
        assert!(validate_server_url("ftp://host").is_err());
        assert!(validate_server_url("https://").is_err());
        assert!(validate_server_url("not-a-url").is_err());
    }

    #[test]
    fn validate_server_url_strips_trailing_slash() {
        assert_eq!(
            validate_server_url("https://host/").unwrap(),
            "https://host"
        );
        assert_eq!(
            validate_server_url("https://host///").unwrap(),
            "https://host"
        );
    }

    #[test]
    fn http_sync_api_new_enforces_tls_at_use_time() {
        assert!(HttpSyncApi::new("http://remote.example.com").is_err());
        assert!(HttpSyncApi::new("http://127.0.0.1:8787").is_ok());
        assert!(HttpSyncApi::new("https://sync.example.com").is_ok());
    }

    #[test]
    fn sync_error_exposes_status_and_code() {
        let api = SyncError::Api {
            status: 426,
            code: "schema_upgrade_required".to_string(),
            message: "upgrade".to_string(),
        };
        assert_eq!(api.status(), Some(426));
        assert_eq!(api.api_code(), Some("schema_upgrade_required"));

        let net = SyncError::Network("boom".to_string());
        assert_eq!(net.status(), None);
        assert_eq!(net.api_code(), None);
    }

    #[test]
    fn wire_error_envelope_tolerates_unknown_fields() {
        let body = r#"{"error":{"code":"clock_skew","message":"reloj","extra":1},"trace":"x"}"#;
        let parsed: WireError = serde_json::from_str(body).expect("parse");
        let inner = parsed.error.expect("error body");
        assert_eq!(inner.code, "clock_skew");
        assert_eq!(inner.message, "reloj");
    }

    #[test]
    fn pull_response_tolerates_unknown_fields_and_null_payload() {
        let body = r#"{
            "rows":[{"table":"assets","row_id":"a1","server_seq":88,"deleted":true,
                     "changed_at":1,"device_id":"d1","payload":null,"future_field":"ok"}],
            "next_since":88,"has_more":false,"schema_tag":"0023_sync_ids",
            "server_epoch":"e1","server_now_ms":2,"unknown":true
        }"#;
        let parsed: PullResponse = serde_json::from_str(body).expect("parse pull");
        assert_eq!(parsed.rows.len(), 1);
        assert!(parsed.rows[0].deleted);
        assert!(parsed.rows[0].payload.is_none());
        assert_eq!(parsed.next_since, 88);
    }

    #[test]
    fn push_change_omits_payload_for_delete() {
        let change = PushChange {
            table: "items".to_string(),
            row_id: "i1".to_string(),
            op: "delete".to_string(),
            changed_at: 10,
            base_seq: 3,
            payload: None,
        };
        let json = serde_json::to_value(&change).expect("serialize");
        assert!(
            json.get("payload").is_none(),
            "delete change must omit payload"
        );
        assert_eq!(json["op"], "delete");
    }

    #[test]
    fn push_response_tolerates_missing_winner_and_unknown_fields() {
        let body = r#"{
            "results":[{"table":"items","row_id":"i1","status":"applied","server_seq":5}],
            "max_server_seq":5,"server_epoch":"e1","server_now_ms":9,"unknown":42
        }"#;
        let parsed: PushResponse = serde_json::from_str(body).expect("parse push");
        assert_eq!(parsed.results.len(), 1);
        assert!(parsed.results[0].winner.is_none());
        assert_eq!(parsed.results[0].status, "applied");
    }
}
