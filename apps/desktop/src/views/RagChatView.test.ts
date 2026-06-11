import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { locale } from '$lib/i18n'
import type { RagAnswer } from '$lib/rag'
import RagChatView from './RagChatView.svelte'

const { navigateMock } = vi.hoisted(() => ({
  navigateMock: vi.fn(),
}))

vi.mock('$lib/navigation', () => ({
  navigation: {
    navigate: navigateMock,
  },
}))

const mockInvoke = vi.mocked(invoke)

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })

  return { promise, resolve, reject }
}

const answerWithSources: RagAnswer = {
  answer: 'La huelga comenzó en junio de 1966 [1].',
  sources: [
    {
      index: 1,
      assetId: 'asset-1',
      itemId: 'item-1',
      itemTitle: 'Entrevista 12',
      collectionId: 'col-1',
      collectionName: 'Historia oral',
      snippet: 'la huelga comenzó cuando los obreros del SOIP...',
      score: 0.91,
      startSeconds: 65,
      endSeconds: 80,
    },
  ],
  model: 'test-model',
}

function getComposer() {
  return screen.getByRole('textbox', { name: 'Escribí tu pregunta…' })
}

async function sendQuestion(question: string) {
  const composer = getComposer()
  await fireEvent.input(composer, { target: { value: question } })
  await fireEvent.keyDown(composer, { key: 'Enter' })
}

describe('RagChatView', () => {
  beforeEach(() => {
    locale.set('es')
    navigateMock.mockReset()
    mockInvoke.mockReset()
  })

  it('renders the empty state with header copy and composer controls', () => {
    render(RagChatView)

    expect(screen.getByRole('heading', { name: 'Chat de investigación' })).toBeInTheDocument()
    expect(
      screen.getByText('Consultá la base de conocimiento de transcripciones')
    ).toBeInTheDocument()
    expect(
      screen.getByText(
        'Hacé una pregunta sobre tus transcripciones. Las respuestas citan las fuentes.'
      )
    ).toBeInTheDocument()
    expect(getComposer()).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Enviar' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Nueva conversación' })).toBeInTheDocument()
  })

  it('sends a question with Enter and renders the answer with its sources', async () => {
    mockInvoke.mockResolvedValueOnce(answerWithSources)

    render(RagChatView)
    await sendQuestion('¿Cuándo comenzó la huelga?')

    expect(mockInvoke).toHaveBeenCalledTimes(1)
    expect(mockInvoke).toHaveBeenCalledWith('rag_ask', {
      question: '¿Cuándo comenzó la huelga?',
      history: [],
      topK: undefined,
    })

    expect(screen.getByText('¿Cuándo comenzó la huelga?')).toBeInTheDocument()

    await waitFor(() => {
      expect(screen.getByText('La huelga comenzó en junio de 1966 [1].')).toBeInTheDocument()
    })

    expect(screen.getByText('Fuentes')).toBeInTheDocument()
    expect(screen.getByText('[1]')).toBeInTheDocument()
    expect(screen.getByText('Entrevista 12 (Historia oral)')).toBeInTheDocument()
    expect(screen.getByText('1:05–1:20')).toBeInTheDocument()
    expect(screen.getByText('la huelga comenzó cuando los obreros del SOIP...')).toBeInTheDocument()
  })

  it('does not send when Shift+Enter inserts a newline', async () => {
    render(RagChatView)

    const composer = getComposer()
    await fireEvent.input(composer, { target: { value: 'pregunta larga' } })
    await fireEvent.keyDown(composer, { key: 'Enter', shiftKey: true })

    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('does not send on Enter while IME composition is active', async () => {
    render(RagChatView)

    const composer = getComposer()
    await fireEvent.input(composer, { target: { value: 'にほんご' } })
    await fireEvent.keyDown(composer, { key: 'Enter', isComposing: true })

    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it('navigates to the cited item when a source is clicked', async () => {
    mockInvoke.mockResolvedValueOnce(answerWithSources)

    render(RagChatView)
    await sendQuestion('¿Cuándo comenzó la huelga?')

    const sourceButton = await screen.findByRole('button', {
      name: 'Abrir fuente: [1] Entrevista 12',
    })
    await fireEvent.click(sourceButton)

    expect(navigateMock).toHaveBeenCalledWith({
      name: 'item',
      collectionId: 'col-1',
      collectionName: 'Historia oral',
      itemId: 'item-1',
      itemTitle: 'Entrevista 12',
      assetId: 'asset-1',
    })
  })

  it('omits the timestamp when startSeconds is null', async () => {
    mockInvoke.mockResolvedValueOnce({
      ...answerWithSources,
      sources: [
        { ...answerWithSources.sources[0]!, startSeconds: null, endSeconds: null },
      ],
    })

    render(RagChatView)
    await sendQuestion('¿Cuándo comenzó la huelga?')

    await waitFor(() => {
      expect(screen.getByText('Entrevista 12 (Historia oral)')).toBeInTheDocument()
    })
    expect(screen.queryByText('1:05–1:20')).not.toBeInTheDocument()
  })

  it('shows the no-results copy as an assistant message without sources', async () => {
    mockInvoke.mockResolvedValueOnce({ answer: '', sources: [], model: 'test-model' })

    render(RagChatView)
    await sendQuestion('¿Algo sin contexto?')

    await waitFor(() => {
      expect(
        screen.getByText(
          'No encontré contenido relevante en las transcripciones para esa pregunta.'
        )
      ).toBeInTheDocument()
    })
    expect(screen.queryByText('Fuentes')).not.toBeInTheDocument()
  })

  it('shows backend errors inline as an alert', async () => {
    const backendError = 'Falta la API key de OpenRouter. Configurala en Configuración.'
    mockInvoke.mockRejectedValueOnce(backendError)

    render(RagChatView)
    await sendQuestion('¿Cuándo comenzó la huelga?')

    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent(backendError)
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })

  it('shows the thinking row and disables the composer while loading', async () => {
    const pending = deferred<RagAnswer>()
    mockInvoke.mockReturnValueOnce(pending.promise)

    render(RagChatView)
    await sendQuestion('¿Cuándo comenzó la huelga?')

    expect(screen.getByRole('status')).toHaveTextContent('Buscando en las transcripciones…')
    expect(getComposer()).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Enviar' })).toBeDisabled()

    pending.resolve(answerWithSources)

    await waitFor(() => {
      expect(screen.queryByRole('status')).not.toBeInTheDocument()
    })
    expect(getComposer()).toBeEnabled()
  })

  it('accumulates prior turns as history across sends', async () => {
    mockInvoke
      .mockResolvedValueOnce(answerWithSources)
      .mockResolvedValueOnce({ answer: 'Liderada por la comisión interna.', sources: [], model: 'test-model' })

    render(RagChatView)

    await sendQuestion('¿Cuándo comenzó la huelga?')
    await waitFor(() => {
      expect(screen.getByText('La huelga comenzó en junio de 1966 [1].')).toBeInTheDocument()
    })

    await sendQuestion('¿Quién la lideró?')

    expect(mockInvoke).toHaveBeenCalledTimes(2)
    expect(mockInvoke).toHaveBeenLastCalledWith('rag_ask', {
      question: '¿Quién la lideró?',
      history: [
        { role: 'user', content: '¿Cuándo comenzó la huelga?' },
        { role: 'assistant', content: 'La huelga comenzó en junio de 1966 [1].' },
      ],
      topK: undefined,
    })

    await waitFor(() => {
      expect(screen.getByText('Liderada por la comisión interna.')).toBeInTheDocument()
    })
  })

  it('starts a new conversation and ignores stale in-flight responses', async () => {
    const pending = deferred<RagAnswer>()
    mockInvoke.mockReturnValueOnce(pending.promise)

    render(RagChatView)
    await sendQuestion('¿Cuándo comenzó la huelga?')

    await fireEvent.click(screen.getByRole('button', { name: 'Nueva conversación' }))

    pending.resolve(answerWithSources)
    await Promise.resolve()

    expect(
      screen.getByText(
        'Hacé una pregunta sobre tus transcripciones. Las respuestas citan las fuentes.'
      )
    ).toBeInTheDocument()
    expect(
      screen.queryByText('La huelga comenzó en junio de 1966 [1].')
    ).not.toBeInTheDocument()
    expect(screen.queryByText('¿Cuándo comenzó la huelga?')).not.toBeInTheDocument()
  })
})
