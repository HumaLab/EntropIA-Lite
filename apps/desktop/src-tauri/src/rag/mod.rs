//! RAG (Retrieval-Augmented Generation) chat sobre la base de transcripciones.
//!
//! Recuperación híbrida (embeddings + FTS5 fusionados con Reciprocal Rank
//! Fusion) que alimenta un prompt de fragmentos numerados para que el modelo
//! responda con citas `[n]`.

pub mod commands;
pub(crate) mod retrieval;

use serde::{Deserialize, Serialize};

/// Un turno previo de la conversación, provisto por el frontend.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RagChatTurn {
    pub role: String,
    pub content: String,
}

/// Respuesta final que recibe el frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RagAnswer {
    pub answer: String,
    pub sources: Vec<RagSource>,
    pub model: String,
}

/// Una fuente citada. `index` es 1-based y coincide con las citas `[n]`
/// incluidas en el texto de la respuesta.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RagSource {
    pub index: u32,
    pub asset_id: String,
    pub item_id: String,
    pub item_title: String,
    pub collection_id: String,
    pub collection_name: String,
    pub snippet: String,
    pub score: f64,
    pub start_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
}
