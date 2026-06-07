/**
 * Settings frontend client for EntropIA desktop app.
 * Wraps Tauri commands for the app_settings key-value store.
 */

import { invoke } from '@tauri-apps/api/core'

export interface SettingEntry {
  key: string
  value: string
}

export interface ModelInfo {
  id: string
  name: string
  context_length: number
}

// ---------------------------------------------------------------------------
// Settings CRUD
// ---------------------------------------------------------------------------

export function settingsGet(key: string): Promise<string | null> {
  return invoke<string | null>('settings_get', { key })
}

export function settingsSet(key: string, value: string): Promise<void> {
  return invoke<void>('settings_set', { key, value })
}

export function settingsGetAll(): Promise<SettingEntry[]> {
  return invoke<SettingEntry[]>('settings_get_all')
}

export function settingsDelete(key: string): Promise<void> {
  return invoke<void>('settings_delete', { key })
}

// ---------------------------------------------------------------------------
// OpenRouter-specific
// ---------------------------------------------------------------------------

export function testOpenrouterConnection(apiKey: string): Promise<ModelInfo[]> {
  return invoke<ModelInfo[]>('test_openrouter_connection', { apiKey })
}

export function testAssemblyaiConnection(apiKey: string): Promise<void> {
  return invoke<void>('test_assemblyai_connection', { apiKey })
}

export function testGlmOcrConnection(apiKey: string): Promise<void> {
  return invoke<void>('test_glm_ocr_connection', { apiKey })
}

// ---------------------------------------------------------------------------
// Well-known setting keys
// ---------------------------------------------------------------------------

export const SETTINGS_KEYS = {
  OPENROUTER_API_KEY: 'openrouter_api_key',
  OPENROUTER_MODEL: 'openrouter_model',
  OPENROUTER_EMBEDDING_MODEL: 'openrouter_embedding_model',
  LLM_MODE: 'llm_mode',
  EMBEDDING_PROVIDER: 'embedding_provider',
  LOCAL_EMBEDDING_MODEL_DIR: 'local_embedding_model_dir',
  ASSEMBLYAI_API_KEY: 'assemblyai_api_key',
  ASSEMBLYAI_SPEAKER_LABELS: 'assemblyai_role_speaker_identification',
  STT_MODE: 'stt_mode',
  GLM_OCR_API_KEY: 'glm_ocr_api_key',
  OCRH_MODE: 'ocrh_mode',
  LANGUAGE: 'language',
  OCR_CORRECTION_PROMPT: 'prompt_ocr_correction',
  SUMMARY_PROMPT: 'prompt_summary',
  NER_PROMPT: 'prompt_ner',
  TRIPLETS_PROMPT: 'prompt_triplets',
  LLM_TEMPERATURE: 'llm_temperature',
  LLM_MAX_TOKENS: 'llm_max_tokens',
  LLM_TOP_P: 'llm_top_p',
  LLM_TOP_K: 'llm_top_k',
  LLM_PRESENCE_PENALTY: 'llm_presence_penalty',
  LLM_FREQUENCY_PENALTY: 'llm_frequency_penalty',
  LLM_STOP_SEQUENCES: 'llm_stop_sequences',
  LLM_OCR_CORRECTION_TEMPERATURE: 'llm_ocr_correction_temperature',
  LLM_OCR_CORRECTION_MAX_TOKENS: 'llm_ocr_correction_max_tokens',
  LLM_OCR_CORRECTION_TOP_P: 'llm_ocr_correction_top_p',
  LLM_OCR_CORRECTION_TOP_K: 'llm_ocr_correction_top_k',
  LLM_OCR_CORRECTION_PRESENCE_PENALTY: 'llm_ocr_correction_presence_penalty',
  LLM_OCR_CORRECTION_FREQUENCY_PENALTY: 'llm_ocr_correction_frequency_penalty',
  LLM_OCR_CORRECTION_STOP_SEQUENCES: 'llm_ocr_correction_stop_sequences',
  LLM_SUMMARY_TEMPERATURE: 'llm_summary_temperature',
  LLM_SUMMARY_MAX_TOKENS: 'llm_summary_max_tokens',
  LLM_SUMMARY_TOP_P: 'llm_summary_top_p',
  LLM_SUMMARY_TOP_K: 'llm_summary_top_k',
  LLM_SUMMARY_PRESENCE_PENALTY: 'llm_summary_presence_penalty',
  LLM_SUMMARY_FREQUENCY_PENALTY: 'llm_summary_frequency_penalty',
  LLM_SUMMARY_STOP_SEQUENCES: 'llm_summary_stop_sequences',
  LLM_NER_TEMPERATURE: 'llm_ner_temperature',
  LLM_NER_MAX_TOKENS: 'llm_ner_max_tokens',
  LLM_NER_TOP_P: 'llm_ner_top_p',
  LLM_NER_TOP_K: 'llm_ner_top_k',
  LLM_NER_PRESENCE_PENALTY: 'llm_ner_presence_penalty',
  LLM_NER_FREQUENCY_PENALTY: 'llm_ner_frequency_penalty',
  LLM_NER_STOP_SEQUENCES: 'llm_ner_stop_sequences',
  LLM_TRIPLETS_TEMPERATURE: 'llm_triplets_temperature',
  LLM_TRIPLETS_MAX_TOKENS: 'llm_triplets_max_tokens',
  LLM_TRIPLETS_TOP_P: 'llm_triplets_top_p',
  LLM_TRIPLETS_TOP_K: 'llm_triplets_top_k',
  LLM_TRIPLETS_PRESENCE_PENALTY: 'llm_triplets_presence_penalty',
  LLM_TRIPLETS_FREQUENCY_PENALTY: 'llm_triplets_frequency_penalty',
  LLM_TRIPLETS_STOP_SEQUENCES: 'llm_triplets_stop_sequences',
} as const

export type LlmMode = 'openrouter'
export type EmbeddingProvider = 'api'
export type SttMode = 'assemblyai'
export type OcrhMode = 'glm_ocr'

export const DEFAULT_OPENROUTER_MODEL = 'google/gemma-3-4b-it'
export const DEFAULT_OPENROUTER_EMBEDDING_MODEL = 'baai/bge-m3'
export const DEFAULT_LLM_MODE: LlmMode = 'openrouter'
export const DEFAULT_EMBEDDING_PROVIDER: EmbeddingProvider = 'api'
export const DEFAULT_STT_MODE: SttMode = 'assemblyai'
export const DEFAULT_OCRH_MODE: OcrhMode = 'glm_ocr'

export const DEFAULT_PROMPTS = {
  ocrCorrectionPrompt: `Usa la imagen adjunta como referencia principal y el OCR como borrador inicial. Corrige errores, verifica coincidencia con la imagen y completa texto omitido si es claramente visible. Conserva idioma y estructura. No inventes contenido no visible. Devuelve sólo el texto final corregido.

Reglas obligatorias:
1. Contrastá cada fragmento del OCR contra la imagen del mismo asset.
2. Corregí sustituciones de caracteres, palabras mal leídas, espacios faltantes y cortes de línea cuando la imagen lo confirme.
3. Recuperá palabras, números, nombres, fechas o líneas omitidas sólo si son claramente legibles en la imagen.
4. Conservá idioma, ortografía histórica, nombres propios, puntuación significativa y estructura de párrafos/listas/tablas cuando sean visibles.
5. Si una zona es ilegible o ambigua, no la inventes: dejá el mejor texto verificable desde OCR/imagen o mantené el fragmento dudoso sin expandirlo.
6. No resumas, no modernices, no expliques y no agregues contenido fuera del documento.

Salida:
- Devolvé SOLO el texto final corregido.
- No agregues títulos, comentarios, markdown, comillas, bloques de código ni JSON.
- No repitas la consigna.

OCR borrador:
{text}`,
  summaryPrompt: `Resumí este texto de documento histórico en un ÚNICO párrafo conciso. El resumen debe:
- Tener entre 10 y 15 líneas
- Preservar nombres propios, fechas, lugares y eventos clave
- Estar escrito en el mismo idioma que el texto original (por defecto, español)
- SIEMPRE terminar con una oración completa que termine en punto

NO superes las 15 líneas. NO cortes a mitad de frase.

Texto:
{text}`,
  nerPrompt: `Extraé entidades nombradas del texto histórico. Devolvé SOLO JSON válido, sin markdown. Usá exclusivamente estas categorías: PER, LOC, ORG, DATE, MISC. Formato: [{"value":"...","type":"PER|LOC|ORG|DATE|MISC","start_offset":0,"end_offset":0,"confidence":0.95}]. Si no hay entidades, devolvé []. No inventes entidades ni uses categorías fuera del contrato.

Texto:
{text}`,
  tripletsPrompt: `Extraé triples semánticos (sujeto-predicado-objeto) de este texto de documento histórico.

Reglas obligatorias:
- Devolvé SOLO un array JSON válido.
- Cada elemento DEBE ser un objeto con EXACTAMENTE estas claves: "subject", "predicate", "object".
- Todos los valores DEBEN ser strings JSON válidos.
- No agregues claves extra.
- No agregues texto antes ni después del array.
- Si no encontrás relaciones confiables, devolvé [].
- Preferí sujetos y objetos completos, no fragmentos sueltos.
- Preservá literalmente nombres propios y marcadores como "1º" o "2ª".

Texto:
{text}`,
} as const

export const DEFAULT_MODEL_PARAMS = {
  temperature: '0.3',
  maxTokens: '',
  topP: '',
  topK: '',
  presencePenalty: '0',
  frequencyPenalty: '0',
  stopSequences: '',
} as const

export const DEFAULT_MODEL_PARAMS_BY_FLOW = {
  ocrCorrection: { ...DEFAULT_MODEL_PARAMS, maxTokens: '' },
  summary: { ...DEFAULT_MODEL_PARAMS, maxTokens: '' },
  ner: { ...DEFAULT_MODEL_PARAMS, maxTokens: '' },
  triplets: { ...DEFAULT_MODEL_PARAMS, maxTokens: '' },
} as const
