import { invoke } from '@tauri-apps/api/core'

export interface RagChatTurn {
  role: 'user' | 'assistant'
  content: string
}

export interface RagSource {
  /** 1-based index matching [n] citations in the answer text. */
  index: number
  assetId: string
  itemId: string
  itemTitle: string
  collectionId: string
  collectionName: string
  snippet: string
  score: number
  startSeconds: number | null
  endSeconds: number | null
}

export interface RagAnswer {
  answer: string
  sources: RagSource[]
  model: string
}

export function ragAsk(
  question: string,
  history: RagChatTurn[],
  topK?: number
): Promise<RagAnswer> {
  return invoke<RagAnswer>('rag_ask', { question, history, topK })
}
