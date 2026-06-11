use super::engine::{Segment, TranscriptionResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

const ASSEMBLYAI_API_BASE: &str = "https://api.assemblyai.com/v2";
const DEFAULT_SPEECH_MODELS: [&str; 2] = ["universal-3-pro", "universal-2"];
/// Audio uploads can be large; give them a generous per-request timeout that
/// overrides the client-wide default.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// Bound on transcript status polls (3 s apart, ~60 minutes total) so a
/// transcript stuck in 'queued'/'processing' on the provider side cannot
/// wedge the dedicated transcription worker thread forever.
const MAX_TRANSCRIPT_POLL_ATTEMPTS: u32 = 1200;

#[derive(Deserialize)]
struct AssemblyAiApiError {
    error: Option<String>,
}

#[derive(Deserialize)]
struct UploadResponse {
    upload_url: String,
}

#[derive(Serialize)]
struct CreateTranscriptRequest {
    audio_url: String,
    speech_models: [&'static str; 2],
    language_detection: bool,
    temperature: u8,
    speaker_labels: bool,
}

#[derive(Deserialize)]
struct CreateTranscriptResponse {
    id: String,
}

#[derive(Deserialize)]
struct TranscriptStatusResponse {
    status: String,
    text: Option<String>,
    error: Option<String>,
    language_code: Option<String>,
    audio_duration: Option<f64>,
    utterances: Option<Vec<TranscriptUtterance>>,
    speech_understanding: Option<SpeechUnderstandingStatus>,
}

#[derive(Deserialize)]
struct TranscriptUtterance {
    speaker: Option<String>,
    speaker_label: Option<String>,
    text: String,
}

#[derive(Deserialize)]
struct SpeechUnderstandingStatus {
    response: Option<SpeechUnderstandingResponse>,
}

#[derive(Deserialize)]
struct SpeechUnderstandingResponse {
    speaker_identification: Option<SpeakerIdentificationResponse>,
}

#[derive(Deserialize)]
struct SpeakerIdentificationResponse {
    mapping: Option<HashMap<String, String>>,
}

pub struct AssemblyAiClient {
    client: reqwest::Client,
    api_key: String,
}

impl AssemblyAiClient {
    /// Build a client with request/connect timeouts so a stalled connection
    /// cannot wedge the serial transcription worker. Surfaces HTTP-client
    /// init failures as `Err` instead of panicking (release builds use
    /// `panic = "abort"`).
    pub fn new(api_key: String) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent("EntropIA-Desktop/0.1 (historical-research-app)")
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| format!("Failed to build AssemblyAI HTTP client: {e}"))?;

        Ok(Self { client, api_key })
    }

    pub async fn test_connection(&self) -> Result<(), String> {
        let response = self
            .client
            .get(format!("{ASSEMBLYAI_API_BASE}/transcript?limit=1"))
            .header("Authorization", &self.api_key)
            .send()
            .await
            .map_err(|e| format!("AssemblyAI connection test failed: {e}"))?;

        Self::ensure_success(response, "AssemblyAI")
            .await
            .map(|_| ())
    }

    pub async fn transcribe_file<F>(
        &self,
        audio_path: &Path,
        enable_speaker_labels: bool,
        mut on_progress: F,
    ) -> Result<TranscriptionResult, String>
    where
        F: FnMut(u8, &str),
    {
        on_progress(20, "uploading");

        let audio_bytes = tokio::fs::read(audio_path)
            .await
            .map_err(|e| format!("Failed to read audio file {}: {e}", audio_path.display()))?;

        let upload_response = self
            .client
            .post(format!("{ASSEMBLYAI_API_BASE}/upload"))
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/octet-stream")
            .timeout(UPLOAD_TIMEOUT)
            .body(audio_bytes)
            .send()
            .await
            .map_err(|e| format!("AssemblyAI upload failed: {e}"))?;

        let upload: UploadResponse = Self::ensure_success(upload_response, "AssemblyAI")
            .await?
            .json()
            .await
            .map_err(|e| format!("Failed to parse AssemblyAI upload response: {e}"))?;

        on_progress(40, "submitting_remote");

        let transcript_response = self
            .client
            .post(format!("{ASSEMBLYAI_API_BASE}/transcript"))
            .header("Authorization", &self.api_key)
            .json(&CreateTranscriptRequest {
                audio_url: upload.upload_url,
                speech_models: DEFAULT_SPEECH_MODELS,
                language_detection: true,
                temperature: 0,
                speaker_labels: enable_speaker_labels,
            })
            .send()
            .await
            .map_err(|e| format!("AssemblyAI transcript request failed: {e}"))?;

        let created: CreateTranscriptResponse =
            Self::ensure_success(transcript_response, "AssemblyAI")
                .await?
                .json()
                .await
                .map_err(|e| format!("Failed to parse AssemblyAI transcript response: {e}"))?;

        let mut poll_attempt = 0_u32;
        loop {
            poll_attempt = poll_attempt.saturating_add(1);
            let progress =
                45_u32.saturating_add((poll_attempt.saturating_sub(1)).saturating_mul(5));
            on_progress(progress.min(90) as u8, "polling_remote");

            let status_response = self
                .client
                .get(format!("{ASSEMBLYAI_API_BASE}/transcript/{}", created.id))
                .header("Authorization", &self.api_key)
                .send()
                .await
                .map_err(|e| format!("AssemblyAI polling failed: {e}"))?;

            let transcript: TranscriptStatusResponse =
                Self::ensure_success(status_response, "AssemblyAI")
                    .await?
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse AssemblyAI polling response: {e}"))?;

            match transcript.status.as_str() {
                "completed" => {
                    let text = format_transcript_text(
                        transcript.text.unwrap_or_default(),
                        transcript.utterances,
                        transcript.speech_understanding,
                    );
                    let duration_ms = transcript
                        .audio_duration
                        .map(|seconds| (seconds * 1000.0).round() as u64)
                        .unwrap_or(0);
                    let segments = if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![Segment {
                            start: 0.0,
                            end: duration_ms as f64 / 1000.0,
                            text: text.clone(),
                        }]
                    };

                    return Ok(TranscriptionResult {
                        text,
                        language: transcript
                            .language_code
                            .unwrap_or_else(|| "auto".to_string()),
                        segments,
                        duration_ms,
                    });
                }
                "error" => {
                    return Err(transcript.error.unwrap_or_else(|| {
                        "AssemblyAI returned an unknown transcription error".to_string()
                    }))
                }
                _ => {
                    if poll_attempt >= MAX_TRANSCRIPT_POLL_ATTEMPTS {
                        return Err(format!(
                            "AssemblyAI transcript {} still '{}' after ~{} minutes of polling; aborting",
                            created.id,
                            transcript.status,
                            MAX_TRANSCRIPT_POLL_ATTEMPTS * 3 / 60
                        ));
                    }
                    tokio::time::sleep(Duration::from_secs(3)).await
                }
            }
        }
    }

    async fn ensure_success(
        response: reqwest::Response,
        provider_name: &str,
    ) -> Result<reqwest::Response, String> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let body = response.text().await.unwrap_or_default();
        let api_error = serde_json::from_str::<AssemblyAiApiError>(&body)
            .ok()
            .and_then(|parsed| parsed.error)
            .unwrap_or_else(|| body.trim().to_string());

        Err(format!("{provider_name} API error ({status}): {api_error}"))
    }
}

fn format_transcript_text(
    fallback_text: String,
    utterances: Option<Vec<TranscriptUtterance>>,
    speech_understanding: Option<SpeechUnderstandingStatus>,
) -> String {
    let speaker_mapping = speech_understanding
        .and_then(|status| status.response)
        .and_then(|response| response.speaker_identification)
        .and_then(|speaker_identification| speaker_identification.mapping);

    if let Some(utterances) = utterances {
        let formatted = format_utterances_with_speaker_labels(utterances);

        if !formatted.is_empty() {
            return formatted;
        }
    }

    let remapped = remap_speaker_prefixes(&fallback_text, speaker_mapping.as_ref());
    if !remapped.is_empty() {
        return remapped;
    }

    fallback_text.trim().to_string()
}

fn format_utterances_with_speaker_labels(utterances: Vec<TranscriptUtterance>) -> String {
    let mut speaker_numbers = HashMap::<String, usize>::new();
    let mut next_speaker_number = 1_usize;

    utterances
        .into_iter()
        .filter_map(|utterance| {
            let speaker_key = utterance.speaker.or(utterance.speaker_label)?;
            let text = utterance.text.trim();
            if text.is_empty() {
                return None;
            }

            let label = speaker_label_for_key(
                &speaker_key,
                &mut speaker_numbers,
                &mut next_speaker_number,
            )?;
            Some(format!("{label}: {text}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn remap_speaker_prefixes(text: &str, mapping: Option<&HashMap<String, String>>) -> String {
    let mut speaker_numbers = HashMap::<String, usize>::new();
    let mut next_speaker_number = 1_usize;
    let mut remapped_any_line = false;

    let remapped = text
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            let Some((speaker_key, content)) = trimmed.split_once(':') else {
                return trimmed.to_string();
            };

            let canonical_speaker_key = mapping
                .and_then(|mapping| mapping.get(speaker_key.trim()))
                .map(String::as_str)
                .unwrap_or_else(|| speaker_key.trim());

            if mapping.is_none() && !looks_like_speaker_key(canonical_speaker_key) {
                return trimmed.to_string();
            }

            let content = content.trim();
            let Some(label) = speaker_label_for_key(
                canonical_speaker_key,
                &mut speaker_numbers,
                &mut next_speaker_number,
            ) else {
                return trimmed.to_string();
            };

            remapped_any_line = true;
            if content.is_empty() {
                label
            } else {
                format!("{label}: {content}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if !remapped_any_line {
        return String::new();
    }

    remapped.trim().to_string()
}

fn speaker_label_for_key(
    speaker_key: &str,
    speaker_numbers: &mut HashMap<String, usize>,
    next_speaker_number: &mut usize,
) -> Option<String> {
    let normalized = speaker_key.trim();
    if normalized.is_empty() {
        return None;
    }

    let index = *speaker_numbers
        .entry(normalized.to_string())
        .or_insert_with(|| {
            let index = *next_speaker_number;
            *next_speaker_number += 1;
            index
        });
    Some(format!("Hablante {index}"))
}

fn looks_like_speaker_key(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.len() == 1 && trimmed.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return true;
    }

    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("speaker ")
        || lower.starts_with("speaker_")
        || lower.starts_with("speaker-")
        || lower.starts_with("spk_")
        || lower.starts_with("spk-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn failed_transcript_uses_remote_error_message() {
        let payload: TranscriptStatusResponse = serde_json::from_str(
            r#"{"status":"error","error":"Audio duration exceeds plan limit"}"#,
        )
        .expect("valid transcript error payload");

        assert_eq!(payload.status, "error");
        assert_eq!(
            payload.error.as_deref(),
            Some("Audio duration exceeds plan limit")
        );
    }

    #[test]
    fn transcript_request_sends_speaker_labels_false_when_disabled() {
        let payload = serde_json::to_value(CreateTranscriptRequest {
            audio_url: "https://example.test/audio.mp3".to_string(),
            speech_models: DEFAULT_SPEECH_MODELS,
            language_detection: true,
            temperature: 0,
            speaker_labels: false,
        })
        .expect("request serializes");

        assert_eq!(payload.get("speaker_labels"), Some(&json!(false)));
        assert_eq!(payload.get("speech_understanding"), None);
        assert_eq!(payload.get("speech_model"), None);
        assert_eq!(
            payload.get("speech_models"),
            Some(&json!(["universal-3-pro", "universal-2"]))
        );
    }

    #[test]
    fn transcript_request_includes_speaker_labels_when_enabled() {
        let payload = serde_json::to_value(CreateTranscriptRequest {
            audio_url: "https://example.test/audio.mp3".to_string(),
            speech_models: DEFAULT_SPEECH_MODELS,
            language_detection: true,
            temperature: 0,
            speaker_labels: true,
        })
        .expect("request serializes");

        assert_eq!(
            payload,
            json!({
                "audio_url": "https://example.test/audio.mp3",
                "speech_models": ["universal-3-pro", "universal-2"],
                "language_detection": true,
                "temperature": 0,
                "speaker_labels": true
            })
        );
    }

    #[test]
    fn formats_utterances_using_numbered_speaker_labels() {
        let formatted = format_transcript_text(
            "A: Hola\nB: Buen día\nC: ¿Cómo están?".to_string(),
            Some(vec![
                TranscriptUtterance {
                    speaker: Some("A".to_string()),
                    speaker_label: None,
                    text: "Hola".to_string(),
                },
                TranscriptUtterance {
                    speaker: Some("B".to_string()),
                    speaker_label: None,
                    text: "Buen día".to_string(),
                },
                TranscriptUtterance {
                    speaker: Some("C".to_string()),
                    speaker_label: None,
                    text: "¿Cómo están?".to_string(),
                },
                TranscriptUtterance {
                    speaker: Some("A".to_string()),
                    speaker_label: None,
                    text: "Bien".to_string(),
                },
            ]),
            None,
        );

        assert_eq!(
            formatted,
            "Hablante 1: Hola\nHablante 2: Buen día\nHablante 3: ¿Cómo están?\nHablante 1: Bien"
        );
    }

    #[test]
    fn remaps_existing_speaker_prefixes_to_numbered_labels() {
        let formatted = format_transcript_text(
            "A: Hola\nB: Buen día\nC: Tercera voz".to_string(),
            None,
            None,
        );

        assert_eq!(
            formatted,
            "Hablante 1: Hola\nHablante 2: Buen día\nHablante 3: Tercera voz"
        );
    }

    #[test]
    fn remaps_speech_understanding_roles_to_numbered_labels_for_legacy_responses() {
        let formatted = format_transcript_text(
            "A: Hola\nB: Buen día".to_string(),
            None,
            Some(SpeechUnderstandingStatus {
                response: Some(SpeechUnderstandingResponse {
                    speaker_identification: Some(SpeakerIdentificationResponse {
                        mapping: Some(HashMap::from([
                            ("A".to_string(), "role-one".to_string()),
                            ("B".to_string(), "role-two".to_string()),
                        ])),
                    }),
                }),
            }),
        );

        assert_eq!(formatted, "Hablante 1: Hola\nHablante 2: Buen día");
    }
}
