import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { ragAsk, type RagAnswer, type RagChatTurn } from './rag'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

const mockInvoke = vi.mocked(invoke)

const sampleAnswer: RagAnswer = {
  answer: 'La huelga comenzó en 1966 [1].',
  sources: [
    {
      index: 1,
      assetId: 'asset-1',
      itemId: 'item-1',
      itemTitle: 'Entrevista 12',
      collectionId: 'col-1',
      collectionName: 'Historia oral',
      snippet: 'la huelga comenzó cuando...',
      score: 0.91,
      startSeconds: 65,
      endSeconds: 80,
    },
  ],
  model: 'test-model',
}

describe('ragAsk', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('invokes rag_ask with the exact command and payload', async () => {
    mockInvoke.mockResolvedValueOnce(sampleAnswer)
    const history: RagChatTurn[] = [
      { role: 'user', content: '¿Cuándo comenzó la huelga?' },
      { role: 'assistant', content: 'En 1966 [1].' },
    ]

    const result = await ragAsk('¿Quién la lideró?', history)

    expect(mockInvoke).toHaveBeenCalledTimes(1)
    expect(mockInvoke).toHaveBeenCalledWith('rag_ask', {
      question: '¿Quién la lideró?',
      history,
      topK: undefined,
    })
    expect(result).toEqual(sampleAnswer)
  })

  it('forwards topK when provided', async () => {
    mockInvoke.mockResolvedValueOnce(sampleAnswer)

    await ragAsk('pregunta', [], 8)

    expect(mockInvoke).toHaveBeenCalledWith('rag_ask', {
      question: 'pregunta',
      history: [],
      topK: 8,
    })
  })

  it('propagates backend rejections untouched', async () => {
    const backendError = 'Falta la API key de OpenRouter. Configurala en Configuración.'
    mockInvoke.mockRejectedValueOnce(backendError)

    await expect(ragAsk('pregunta', [])).rejects.toBe(backendError)
  })
})
