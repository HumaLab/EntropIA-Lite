use serde::{Deserialize, Serialize};

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
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
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

// ---------------------------------------------------------------------------
// OpenRouter client
// ---------------------------------------------------------------------------

pub struct OpenRouterClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl OpenRouterClient {
    pub fn new(api_key: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("EntropIA-Desktop/0.1 (historical-research-app)")
            .build()
            .expect("Failed to build reqwest client");
        Self {
            client,
            api_key,
            model,
        }
    }

    /// Returns the configured model's context window size.
    /// Uses a conservative default since we can't always query the API.
    pub fn n_ctx(&self) -> u32 {
        // Most OpenRouter models support at least 8k context
        8192
    }

    /// Generate a completion from the prompt text.
    /// The prompt should be the raw instruction text (NOT wrapped in Gemma format).
    pub async fn generate(&self, prompt: &str, max_tokens: i32) -> Result<String, String> {
        let request = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: ChatMessageContent::Text(prompt.to_string()),
            }],
            max_tokens,
            temperature: 0.3,
        };

        self.send_chat_completion(request).await
    }

    /// Generate a completion from one user message containing text and one image.
    pub async fn generate_with_image(
        &self,
        prompt: &str,
        image_data_url: &str,
        max_tokens: i32,
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
            max_tokens,
            temperature: 0.2,
        };

        self.send_chat_completion(request).await
    }

    async fn send_chat_completion(&self, request: ChatCompletionRequest) -> Result<String, String> {
        let response = self
            .client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("HTTP-Referer", "https://hlab.com.ar/")
            .header("X-Title", "EntropIA")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("OpenRouter request failed: {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("OpenRouter API error ({}): {}", status, body));
        }

        let parsed: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse OpenRouter response: {e}"))?;

        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .ok_or_else(|| "OpenRouter returned no choices".to_string())
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
            return Err(format!("OpenRouter API error ({}): {}", status, body));
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
