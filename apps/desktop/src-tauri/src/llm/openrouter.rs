use serde::{Deserialize, Serialize};
use std::time::Duration;

// ---------------------------------------------------------------------------
// OpenRouter API types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: ChatMessageContent,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ChatMessageContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ChatContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlContent },
}

#[derive(Serialize)]
struct ImageUrlContent {
    url: String,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: i32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stop: Vec<String>,
}

/// Lenient response envelope: OpenRouter legitimately returns `content: null`
/// (refusals, provider failures) and `{"error":{...}}` envelopes WITH HTTP 200
/// when the upstream provider fails mid-request. Every field that can be
/// absent is optional so a partial body never explodes into an opaque
/// "error decoding response body".
#[derive(Deserialize)]
struct ChatCompletionResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    error: Option<OpenRouterErrorBody>,
}

#[derive(Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: ChatResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct ChatResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    refusal: Option<String>,
}

#[derive(Deserialize)]
struct OpenRouterErrorBody {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<serde_json::Value>,
}

/// Outcome of a single chat-completion attempt, classified so the retry loop
/// can distinguish transient failures (provider 5xx/429/408, timeouts) from
/// deterministic ones (auth, refusals, malformed bodies).
#[derive(Debug)]
struct OpenRouterCallError {
    message: String,
    retryable: bool,
}

#[derive(Deserialize)]
struct OpenRouterModel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub context_length: Option<u64>,
}

#[derive(Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModel>,
}

#[derive(Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_length: u64,
}

#[derive(Clone, Debug)]
pub struct GenerationParams {
    pub temperature: f32,
    pub max_tokens: i32,
    pub top_p: Option<f32>,
    pub top_k: Option<i32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub stop_sequences: Vec<String>,
}

impl GenerationParams {
    pub fn with_defaults(max_tokens: i32, temperature: f32) -> Self {
        Self {
            temperature,
            max_tokens,
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            stop_sequences: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// OpenRouter client
// ---------------------------------------------------------------------------

pub struct OpenRouterClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl OpenRouterClient {
    /// Build a client with request/connect timeouts so a stalled connection
    /// cannot wedge the serial LLM worker. Surfaces HTTP-client init failures
    /// as `Err` instead of panicking (release builds use `panic = "abort"`).
    pub fn try_new(api_key: String, model: String) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent("EntropIA-Desktop/0.1 (historical-research-app)")
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| format!("Failed to build OpenRouter HTTP client: {e}"))?;
        Ok(Self {
            client,
            api_key,
            model,
        })
    }

    /// Returns the configured model's context window size.
    /// Uses a conservative default since we can't always query the API.
    pub fn n_ctx(&self) -> u32 {
        // Most OpenRouter models support at least 8k context
        8192
    }

    /// Generate a completion from the prompt text.
    /// The prompt should be the raw instruction text (NOT wrapped in Gemma format).
    #[allow(dead_code)]
    pub async fn generate(&self, prompt: &str, max_tokens: i32) -> Result<String, String> {
        self.generate_with_params(prompt, &GenerationParams::with_defaults(max_tokens, 0.3))
            .await
    }

    pub async fn generate_with_params(
        &self,
        prompt: &str,
        params: &GenerationParams,
    ) -> Result<String, String> {
        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: ChatMessageContent::Text(prompt.to_string()),
            }],
            max_tokens: params.max_tokens,
            temperature: params.temperature,
            top_p: params.top_p,
            top_k: params.top_k,
            presence_penalty: params.presence_penalty,
            frequency_penalty: params.frequency_penalty,
            stop: params.stop_sequences.clone(),
        };

        self.send_chat_completion(request).await
    }

    /// Generate a completion from one user message containing text and one image.
    #[allow(dead_code)]
    pub async fn generate_with_image(
        &self,
        prompt: &str,
        image_data_url: &str,
        max_tokens: i32,
    ) -> Result<String, String> {
        self.generate_with_image_params(
            prompt,
            image_data_url,
            &GenerationParams::with_defaults(max_tokens, 0.2),
        )
        .await
    }

    pub async fn generate_with_image_params(
        &self,
        prompt: &str,
        image_data_url: &str,
        params: &GenerationParams,
    ) -> Result<String, String> {
        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: ChatMessageContent::Parts(vec![
                    ChatContentPart::Text {
                        text: prompt.to_string(),
                    },
                    ChatContentPart::ImageUrl {
                        image_url: ImageUrlContent {
                            url: image_data_url.to_string(),
                        },
                    },
                ]),
            }],
            max_tokens: params.max_tokens,
            temperature: params.temperature,
            top_p: params.top_p,
            top_k: params.top_k,
            presence_penalty: params.presence_penalty,
            frequency_penalty: params.frequency_penalty,
            stop: params.stop_sequences.clone(),
        };

        self.send_chat_completion(request).await
    }

    /// Send a chat completion, retrying transient failures (provider
    /// 5xx/429/408, timeouts, connection resets) up to 3 total attempts with
    /// a short backoff. Non-retryable errors (auth, refusals, malformed
    /// bodies) return immediately.
    async fn send_chat_completion(&self, request: ChatCompletionRequest) -> Result<String, String> {
        const MAX_ATTEMPTS: u32 = 3;
        const BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

        // Serialize once so the body can be re-posted per attempt.
        let payload = serde_json::to_value(&request)
            .map_err(|e| format!("Failed to serialize OpenRouter request: {e}"))?;

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.send_chat_completion_once(&payload).await {
                Ok(content) => return Ok(content),
                Err(error) if error.retryable && attempt < MAX_ATTEMPTS => {
                    let delay = BACKOFF[(attempt - 1) as usize];
                    eprintln!(
                        "[llm/openrouter] attempt {attempt}/{MAX_ATTEMPTS} failed (retryable): {}; retrying in {}s",
                        error.message,
                        delay.as_secs()
                    );
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error.message),
            }
        }
    }

    async fn send_chat_completion_once(
        &self,
        payload: &serde_json::Value,
    ) -> Result<String, OpenRouterCallError> {
        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://hlab.com.ar/")
            .header("X-Title", "EntropIA")
            .json(payload)
            .send()
            .await
            .map_err(|e| OpenRouterCallError {
                message: format!("OpenRouter request failed: {}", error_chain(&e)),
                retryable: true,
            })?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(|e| OpenRouterCallError {
            message: format!("OpenRouter response body read failed: {}", error_chain(&e)),
            retryable: true,
        })?;

        interpret_chat_completion_body(status, &body)
    }

    /// Test the connection by listing available models.
    /// Returns Ok with a list of model IDs on success, Err on failure.
    pub async fn test_connection(&self) -> Result<Vec<ModelInfo>, String> {
        let response = self
            .client
            .get("https://openrouter.ai/api/v1/models")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| format!("OpenRouter connection test failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("OpenRouter API error ({status}): {body}"));
        }

        let parsed: OpenRouterModelsResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse OpenRouter models response: {e}"))?;

        Ok(parsed
            .data
            .into_iter()
            .map(|m| ModelInfo {
                id: m.id,
                name: m.name,
                context_length: m.context_length.unwrap_or(4096),
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Response interpretation (pure, unit-testable)
// ---------------------------------------------------------------------------

/// Walk the `source()` chain so reqwest errors keep their detail. Without
/// this, a timeout surfaces as the opaque "error decoding response body"
/// instead of "error decoding response body: operation timed out".
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut source = e.source();
    while let Some(inner) = source {
        parts.push(inner.to_string());
        source = inner.source();
    }
    parts.join(": ")
}

/// First ~300 chars of the body, trimmed, for inclusion in error messages.
fn body_snippet(body: &str) -> String {
    const MAX_CHARS: usize = 300;
    let trimmed = body.trim();
    match trimmed.char_indices().nth(MAX_CHARS) {
        Some((index, _)) => format!("{}…", &trimmed[..index]),
        None => trimmed.to_string(),
    }
}

fn status_is_retryable(status: u16) -> bool {
    matches!(status, 408 | 429 | 500..=599)
}

/// An OpenRouter error envelope is retryable when its code maps to a
/// transient HTTP class (408/429/5xx) or the message hints at a transient
/// provider condition (timeout, overloaded).
fn error_envelope_is_retryable(error: &OpenRouterErrorBody) -> bool {
    let code_retryable = match &error.code {
        Some(serde_json::Value::Number(number)) => number
            .as_u64()
            .is_some_and(|code| u16::try_from(code).is_ok_and(status_is_retryable)),
        Some(serde_json::Value::String(text)) => {
            text.trim().parse::<u16>().is_ok_and(status_is_retryable)
        }
        _ => false,
    };
    if code_retryable {
        return true;
    }

    error.message.as_deref().is_some_and(|message| {
        let lowered = message.to_lowercase();
        lowered.contains("timeout")
            || lowered.contains("timed out")
            || lowered.contains("overloaded")
    })
}

/// Decision ladder for a chat-completion HTTP exchange. Pure so the
/// classification (message + retryability) is unit-testable without HTTP
/// mocking.
fn interpret_chat_completion_body(status: u16, body: &str) -> Result<String, OpenRouterCallError> {
    // 1. Non-2xx: the status alone decides retryability.
    if !(200..300).contains(&status) {
        return Err(OpenRouterCallError {
            message: format!(
                "OpenRouter API error (HTTP {status}): {}",
                body_snippet(body)
            ),
            retryable: status_is_retryable(status),
        });
    }

    // 3. Unparseable body: include the serde detail and a snippet so the
    // failure is never opaque. The request itself succeeded — not retryable.
    let parsed: ChatCompletionResponse = match serde_json::from_str(body) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Err(OpenRouterCallError {
                message: format!(
                    "OpenRouter returned an unexpected body ({error}): {}",
                    body_snippet(body)
                ),
                retryable: false,
            });
        }
    };

    // 2. Error envelope with HTTP 200: documented OpenRouter behavior when
    // the upstream provider fails mid-request.
    if let Some(error) = &parsed.error {
        let detail = match (&error.message, &error.code) {
            (Some(message), Some(code)) => format!("{message} (code {code})"),
            (Some(message), None) => message.clone(),
            (None, Some(code)) => format!("code {code}"),
            (None, None) => "unspecified error".to_string(),
        };
        return Err(OpenRouterCallError {
            message: format!(
                "OpenRouter API error: {detail}. Body: {}",
                body_snippet(body)
            ),
            retryable: error_envelope_is_retryable(error),
        });
    }

    // 4. No choices, or content null/empty: a 200 with empty content is a
    // model decision, not a transport failure — never retryable here.
    let Some(choice) = parsed.choices.into_iter().next() else {
        return Err(OpenRouterCallError {
            message: "OpenRouter returned no choices".to_string(),
            retryable: false,
        });
    };

    let content = choice
        .message
        .content
        .map(|content| content.trim().to_string())
        .unwrap_or_default();

    if content.is_empty() {
        if let Some(refusal) = choice
            .message
            .refusal
            .as_deref()
            .map(str::trim)
            .filter(|refusal| !refusal.is_empty())
        {
            return Err(OpenRouterCallError {
                message: format!("OpenRouter model refused the request: {refusal}"),
                retryable: false,
            });
        }
        if choice.finish_reason.as_deref() == Some("length") {
            return Err(OpenRouterCallError {
                message: "OpenRouter returned no content (response truncated by max_tokens; \
                          increase max_tokens)"
                    .to_string(),
                retryable: false,
            });
        }
        return Err(OpenRouterCallError {
            message: "OpenRouter returned no content".to_string(),
            retryable: false,
        });
    }

    // 5. Happy path. Warn (but still return) when the model hit max_tokens.
    if choice.finish_reason.as_deref() == Some("length") {
        eprintln!(
            "[llm/openrouter] Warning: response truncated by max_tokens (finish_reason=length)"
        );
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interpret(status: u16, body: &str) -> Result<String, OpenRouterCallError> {
        interpret_chat_completion_body(status, body)
    }

    #[test]
    fn happy_path_returns_trimmed_content() {
        let body =
            r#"{"choices":[{"message":{"content":"  hola mundo  "},"finish_reason":"stop"}]}"#;
        assert_eq!(interpret(200, body).unwrap(), "hola mundo");
    }

    #[test]
    fn finish_reason_length_with_content_still_returns_content() {
        let body = r#"{"choices":[{"message":{"content":"parcial"},"finish_reason":"length"}]}"#;
        assert_eq!(interpret(200, body).unwrap(), "parcial");
    }

    #[test]
    fn null_content_with_refusal_is_non_retryable_and_mentions_refusal() {
        let body = r#"{"choices":[{"message":{"content":null,"refusal":"I cannot help with that"},"finish_reason":"stop"}]}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("I cannot help with that"));
    }

    #[test]
    fn null_content_without_refusal_reports_no_content() {
        let body = r#"{"choices":[{"message":{"content":null},"finish_reason":"stop"}]}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("OpenRouter returned no content"));
    }

    #[test]
    fn empty_content_with_finish_reason_length_mentions_truncation() {
        let body = r#"{"choices":[{"message":{"content":""},"finish_reason":"length"}]}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("max_tokens"));
    }

    #[test]
    fn empty_choices_is_non_retryable() {
        let error = interpret(200, r#"{"choices":[]}"#).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("OpenRouter returned no choices"));
    }

    #[test]
    fn error_envelope_with_http_200_and_code_502_is_retryable() {
        let body = r#"{"error":{"message":"Provider returned error","code":502}}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(error.retryable);
        assert!(error.message.contains("Provider returned error"));
        assert!(error.message.contains("code 502"));
    }

    #[test]
    fn error_envelope_with_code_400_is_non_retryable() {
        let body = r#"{"error":{"message":"Bad request","code":400}}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("Bad request"));
    }

    #[test]
    fn error_envelope_with_timeout_message_is_retryable() {
        let body = r#"{"error":{"message":"Provider request timed out"}}"#;
        let error = interpret(200, body).unwrap_err();
        assert!(error.retryable);
    }

    #[test]
    fn non_2xx_429_is_retryable_and_includes_status() {
        let error = interpret(429, r#"{"error":{"message":"rate limited"}}"#).unwrap_err();
        assert!(error.retryable);
        assert!(error.message.contains("HTTP 429"));
        assert!(error.message.contains("rate limited"));
    }

    #[test]
    fn non_2xx_401_is_non_retryable() {
        let error = interpret(401, r#"{"error":{"message":"User not found."}}"#).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("HTTP 401"));
        assert!(error.message.contains("User not found"));
    }

    #[test]
    fn unparseable_html_body_includes_snippet_and_serde_detail() {
        let body = "<html><body><h1>502 Bad Gateway</h1></body></html>";
        let error = interpret(200, body).unwrap_err();
        assert!(!error.retryable);
        assert!(error.message.contains("unexpected body"));
        assert!(error.message.contains("502 Bad Gateway"));
        // The serde detail must survive (kills the opaque case forever).
        assert!(error.message.contains("expected value"));
    }

    #[test]
    fn long_body_is_truncated_to_a_snippet() {
        let body = "x".repeat(2000);
        let error = interpret(500, &body).unwrap_err();
        assert!(error.retryable);
        assert!(error.message.len() < 500);
        assert!(error.message.contains('…'));
    }

    #[test]
    fn error_chain_joins_sources() {
        use std::fmt;

        #[derive(Debug)]
        struct Inner;
        impl fmt::Display for Inner {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "operation timed out")
            }
        }
        impl std::error::Error for Inner {}

        #[derive(Debug)]
        struct Outer(Inner);
        impl fmt::Display for Outer {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "error decoding response body")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }

        assert_eq!(
            error_chain(&Outer(Inner)),
            "error decoding response body: operation timed out"
        );
    }
}
