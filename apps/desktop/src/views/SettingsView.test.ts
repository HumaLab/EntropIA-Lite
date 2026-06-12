import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SettingsView, {
  buildSettingsSnapshot,
  hasUnsavedSettingsChanges,
  type SettingsSnapshotInput,
} from './SettingsView.svelte'
import { locale } from '$lib/i18n'
import { navigation } from '$lib/navigation'
import { setupKeyboardShortcuts } from '$lib/keyboard'

const {
  invokeMock,
  settingsGetMock,
  settingsGetAllMock,
  settingsSetMock,
  testOpenrouterConnectionMock,
  testAssemblyaiConnectionMock,
  testGlmOcrConnectionMock,
} =
  vi.hoisted(() => ({
    invokeMock: vi.fn(),
    settingsGetMock: vi.fn(),
    settingsGetAllMock: vi.fn(),
    settingsSetMock: vi.fn(),
    testOpenrouterConnectionMock: vi.fn(),
    testAssemblyaiConnectionMock: vi.fn(),
    testGlmOcrConnectionMock: vi.fn(),
  }))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('$lib/settings', async () => {
  const actual = await vi.importActual<typeof import('$lib/settings')>('$lib/settings')
  return {
    ...actual,
    settingsGet: settingsGetMock,
    settingsGetAll: settingsGetAllMock,
    settingsSet: settingsSetMock,
    testOpenrouterConnection: testOpenrouterConnectionMock,
    testAssemblyaiConnection: testAssemblyaiConnectionMock,
    testGlmOcrConnection: testGlmOcrConnectionMock,
  }
})

describe('SettingsView', () => {
  beforeEach(() => {
    locale.set('es')
    invokeMock.mockReset().mockResolvedValue(undefined)
    settingsGetMock.mockReset()
    settingsGetAllMock.mockReset().mockResolvedValue([])
    settingsSetMock.mockReset().mockResolvedValue(undefined)
    testOpenrouterConnectionMock.mockReset()
    testAssemblyaiConnectionMock.mockReset().mockResolvedValue(undefined)
    testGlmOcrConnectionMock.mockReset().mockResolvedValue(undefined)
    settingsGetMock.mockImplementation(async (key: string) => {
      if (key === 'openrouter_api_key') return 'sk-or-v1-test-key'
      if (key === 'openrouter_model') return 'anthropic/claude-3.7-sonnet'
      if (key === 'embedding_provider') return 'api'
      if (key === 'openrouter_embedding_model') return 'baai/bge-m3'
      if (key === 'llm_mode') return 'openrouter'
      if (key === 'assemblyai_api_key') return 'aai-orig-test-1234'
      if (key === 'stt_mode') return 'assemblyai'
      if (key === 'assemblyai_role_speaker_identification') return null
      if (key === 'language') return 'es'
      return null
    })
  })

  it('renders the API-only settings header and tabs', async () => {
    render(SettingsView)

    expect(await screen.findByText('Preferencias')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Configuración' })).toBeInTheDocument()
    expect(
      screen.getByText(
        'Configurá las claves y modelos de los proveedores remotos que usa EntropIA Lite.'
      )
    ).toBeInTheDocument()
    expect(
      screen.getByText('EntropIA Lite usa APIs remotas: OpenRouter, GLM-OCR y AssemblyAI.')
    ).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'APIs remotas' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Prompts' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Model Params' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'RAG Params' })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: 'Logs' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Dependencias de IA' })).not.toBeInTheDocument()
  })

  it('edits and saves prompt and model parameter settings', async () => {
    render(SettingsView)

    await fireEvent.click(await screen.findByRole('tab', { name: 'Prompts' }))
    const ocrPrompt = screen.getByLabelText('OCR correction prompt')
    await fireEvent.input(ocrPrompt, { target: { value: 'Custom OCR {text}' } })

    await fireEvent.click(screen.getByRole('tab', { name: 'Model Params' }))
    const temperatureInput = screen.getAllByLabelText('temperature (0-2)')[0]
    const maxTokensInput = screen.getAllByLabelText('maxTokens (1-32000, vacío = default)')[0]
    expect(temperatureInput).toBeDefined()
    expect(maxTokensInput).toBeDefined()
    await fireEvent.input(temperatureInput!, { target: { value: '0.6' } })
    await fireEvent.input(maxTokensInput!, {
      target: { value: '1234' },
    })

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(settingsSetMock).toHaveBeenCalledWith('prompt_ocr_correction', 'Custom OCR {text}')
    expect(settingsSetMock).toHaveBeenCalledWith('llm_ocr_correction_temperature', '0.6')
    expect(settingsSetMock).toHaveBeenCalledWith('llm_ocr_correction_max_tokens', '1234')
  })

  it('rejects model param formats that the Rust parser cannot parse', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'Model Params' }))

    const maxTokensInput = screen.getAllByLabelText('maxTokens (1-32000, vacío = default)')[0]
    const temperatureInput = screen.getAllByLabelText('temperature (0-2)')[0]

    // Number('12.0') es 12 para JS, pero "12.0".parse::<i32>() falla en Rust.
    await fireEvent.input(maxTokensInput!, { target: { value: '12.0' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(
      await screen.findAllByText('Parámetro inválido en OCR correction: maxTokens')
    ).not.toHaveLength(0)
    expect(settingsSetMock).not.toHaveBeenCalled()

    await fireEvent.input(maxTokensInput!, { target: { value: '1e3' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(settingsSetMock).not.toHaveBeenCalled()

    // '0x1' vale 1 para Number() (en rango 0-2), pero parse::<f32> lo rechaza.
    await fireEvent.input(maxTokensInput!, { target: { value: '' } })
    await fireEvent.input(temperatureInput!, { target: { value: '0x1' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(
      await screen.findAllByText('Parámetro inválido en OCR correction: temperature')
    ).not.toHaveLength(0)
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('normalizes model param text to plain numbers when saving', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'Model Params' }))

    const temperatureInput = screen.getAllByLabelText('temperature (0-2)')[0]
    const maxTokensInput = screen.getAllByLabelText('maxTokens (1-32000, vacío = default)')[0]
    await fireEvent.input(temperatureInput!, { target: { value: '.5' } })
    await fireEvent.input(maxTokensInput!, { target: { value: '007' } })

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    await waitFor(() =>
      expect(settingsSetMock).toHaveBeenCalledWith('llm_ocr_correction_temperature', '0.5')
    )
    expect(settingsSetMock).toHaveBeenCalledWith('llm_ocr_correction_max_tokens', '7')
  })

  it('switches to the Model Params tab when validation fails from another tab', async () => {
    invokeMock.mockImplementation(async (cmd: string) => (cmd === 'logs_get' ? [] : undefined))
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'Model Params' }))

    const maxTokensInput = screen.getAllByLabelText('maxTokens (1-32000, vacío = default)')[0]
    await fireEvent.input(maxTokensInput!, { target: { value: '12.0' } })

    // El error debe ser visible aunque el guardado se dispare desde otra tab.
    await fireEvent.click(screen.getByRole('tab', { name: 'Logs' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(
      await screen.findAllByText('Parámetro inválido en OCR correction: maxTokens')
    ).not.toHaveLength(0)
    expect(screen.getByRole('tab', { name: 'Model Params' })).toHaveAttribute(
      'aria-selected',
      'true'
    )
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('switches to the RAG params tab and shows defaults when no settings are stored', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))

    expect(screen.getByRole('heading', { name: 'RAG Params' })).toBeInTheDocument()
    expect(
      screen.getByText(
        'Estos parámetros ajustan la recuperación del chat de investigación. Los valores mostrados son los vigentes.'
      )
    ).toBeInTheDocument()
    expect(screen.getByLabelText('topK (1-20)')).toHaveValue('6')
    expect(screen.getByLabelText('minSimilarity (0-1, 0 = off)')).toHaveValue('0')
    expect(screen.getByLabelText('candidatesPerLeg (4-200)')).toHaveValue('24')
    expect(screen.getByLabelText('rrfK (1-500)')).toHaveValue('60')
    expect(screen.getByLabelText('snippetMaxChars (200-8000)')).toHaveValue('1600')
    expect(screen.getByLabelText('contextMaxChars (1000-60000)')).toHaveValue('10000')
    expect(screen.getByLabelText('historyTurns (0-20)')).toHaveValue('6')
    expect(screen.getByLabelText('historyTurnMaxChars (100-4000)')).toHaveValue('500')
    expect(screen.getByLabelText('temperature (0-2)')).toHaveValue('0.2')
    expect(screen.getByLabelText('maxTokens (64-32000)')).toHaveValue('1500')
  })

  it('shows stored RAG params overrides instead of defaults', async () => {
    settingsGetAllMock.mockResolvedValue([
      { key: 'rag_top_k', value: '12' },
      { key: 'rag_temperature', value: '0.7' },
    ])

    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))

    await waitFor(() => expect(screen.getByLabelText('topK (1-20)')).toHaveValue('12'))
    expect(screen.getByLabelText('temperature (0-2)')).toHaveValue('0.7')
    expect(screen.getByLabelText('rrfK (1-500)')).toHaveValue('60')
  })

  it('edits and saves RAG params, persisting defaults for untouched fields', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))

    await fireEvent.input(screen.getByLabelText('topK (1-20)'), { target: { value: '9' } })
    await fireEvent.input(screen.getByLabelText('contextMaxChars (1000-60000)'), {
      target: { value: '20000' },
    })

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    await waitFor(() => expect(settingsSetMock).toHaveBeenCalledWith('rag_top_k', '9'))
    expect(settingsSetMock).toHaveBeenCalledWith('rag_context_max_chars', '20000')
    expect(settingsSetMock).toHaveBeenCalledWith('rag_min_similarity', '0')
    expect(settingsSetMock).toHaveBeenCalledWith('rag_candidates_per_leg', '24')
    expect(settingsSetMock).toHaveBeenCalledWith('rag_rrf_k', '60')
    expect(settingsSetMock).toHaveBeenCalledWith('rag_snippet_max_chars', '1600')
    expect(settingsSetMock).toHaveBeenCalledWith('rag_history_turns', '6')
    expect(settingsSetMock).toHaveBeenCalledWith('rag_history_turn_max_chars', '500')
    expect(settingsSetMock).toHaveBeenCalledWith('rag_temperature', '0.2')
    expect(settingsSetMock).toHaveBeenCalledWith('rag_max_tokens', '1500')
  })

  it('blocks saving out-of-range RAG params and shows the validation error', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))

    await fireEvent.input(screen.getByLabelText('topK (1-20)'), { target: { value: '50' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(await screen.findAllByText('Parámetro RAG inválido: topK')).not.toHaveLength(0)
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('blocks saving when snippetMaxChars exceeds contextMaxChars', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))

    // Ambos dentro de su rango individual, pero snippet > context.
    await fireEvent.input(screen.getByLabelText('snippetMaxChars (200-8000)'), {
      target: { value: '5000' },
    })
    await fireEvent.input(screen.getByLabelText('contextMaxChars (1000-60000)'), {
      target: { value: '2000' },
    })
    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(
      await screen.findAllByText('snippetMaxChars no puede superar contextMaxChars.')
    ).not.toHaveLength(0)
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('rejects RAG numeric text that Rust cannot parse, like 12.0 for an integer', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))

    // Number('12.0') es 12 para JS, pero "12.0".parse::<usize>() falla en Rust.
    await fireEvent.input(screen.getByLabelText('topK (1-20)'), { target: { value: '12.0' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(await screen.findAllByText('Parámetro RAG inválido: topK')).not.toHaveLength(0)
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('normalizes RAG numeric text to its canonical form on save', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))

    await fireEvent.input(screen.getByLabelText('temperature (0-2)'), {
      target: { value: '0.20' },
    })
    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    await waitFor(() => expect(settingsSetMock).toHaveBeenCalledWith('rag_temperature', '0.2'))
  })

  it('switches to the RAG params tab when validation fails from another tab', async () => {
    invokeMock.mockImplementation(async (cmd: string) => (cmd === 'logs_get' ? [] : undefined))
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))
    await fireEvent.input(screen.getByLabelText('topK (1-20)'), { target: { value: '50' } })

    // El error debe ser visible aunque el guardado se dispare desde otra tab.
    await fireEvent.click(screen.getByRole('tab', { name: 'Logs' }))
    expect(screen.queryByRole('heading', { name: 'RAG Params' })).not.toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(await screen.findByRole('heading', { name: 'RAG Params' })).toBeInTheDocument()
    expect(screen.getAllByText('Parámetro RAG inválido: topK')).not.toHaveLength(0)
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('restores RAG params defaults from the tab button', async () => {
    render(SettingsView)

    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
    await fireEvent.click(screen.getByRole('tab', { name: 'RAG Params' }))

    await fireEvent.input(screen.getByLabelText('topK (1-20)'), { target: { value: '15' } })
    expect(screen.getByLabelText('topK (1-20)')).toHaveValue('15')

    await fireEvent.click(screen.getByRole('button', { name: 'Restaurar defaults' }))

    expect(screen.getByLabelText('topK (1-20)')).toHaveValue('6')
  })

  it('blocks saving OCR and Summary prompts without the text placeholder', async () => {
    render(SettingsView)

    await fireEvent.click(await screen.findByRole('tab', { name: 'Prompts' }))
    await fireEvent.input(screen.getByLabelText('OCR correction prompt'), {
      target: { value: 'Custom OCR without placeholder' },
    })

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(await screen.findByText('OCR correction prompt: Debe incluir el placeholder {text}.')).toBeInTheDocument()
    expect(settingsSetMock).not.toHaveBeenCalled()

    await fireEvent.input(screen.getByLabelText('OCR correction prompt'), {
      target: { value: 'Custom OCR {text}' },
    })
    await fireEvent.input(screen.getByLabelText('Summary prompt'), {
      target: { value: 'Summarize this document' },
    })

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(await screen.findByText('Summary prompt: Debe incluir el placeholder {text}.')).toBeInTheDocument()
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('blocks saving NER prompts missing required labels', async () => {
    render(SettingsView)

    await fireEvent.click(await screen.findByRole('tab', { name: 'Prompts' }))
    await fireEvent.input(screen.getByLabelText('NER prompt'), {
      target: { value: 'Extract PER, LOC, ORG and DATE from {text}' },
    })

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(await screen.findByText('NER prompt: NER debe conservar estas etiquetas: MISC.')).toBeInTheDocument()
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('blocks saving Triplets prompts missing required JSON keys', async () => {
    render(SettingsView)

    await fireEvent.click(await screen.findByRole('tab', { name: 'Prompts' }))
    await fireEvent.input(screen.getByLabelText('Triplets prompt'), {
      target: { value: 'Return subject and predicate for {text}' },
    })

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(await screen.findByText('Triplets prompt: Triplets debe conservar estas claves: object.')).toBeInTheDocument()
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('validates prompt edits without saving settings', async () => {
    render(SettingsView)

    await fireEvent.click(await screen.findByRole('tab', { name: 'Prompts' }))
    const ocrPrompt = screen.getByLabelText('OCR correction prompt')
    const ocrPromptCard = ocrPrompt.closest('.settings__prompt-card')
    expect(ocrPromptCard).not.toBeNull()

    await fireEvent.input(ocrPrompt, { target: { value: 'Missing placeholder' } })
    await fireEvent.click(within(ocrPromptCard as HTMLElement).getByRole('button', { name: 'Validar cambios' }))

    expect(await within(ocrPromptCard as HTMLElement).findByText('Debe incluir el placeholder {text}.')).toBeInTheDocument()
    expect(settingsSetMock).not.toHaveBeenCalled()

    await fireEvent.input(ocrPrompt, { target: { value: 'Correct {text}' } })
    await fireEvent.click(within(ocrPromptCard as HTMLElement).getByRole('button', { name: 'Validar cambios' }))

    expect(await within(ocrPromptCard as HTMLElement).findByText('Prompt válido.')).toBeInTheDocument()
    expect(settingsSetMock).not.toHaveBeenCalled()
  })

  it('groups OpenRouter generative and embedding models in one provider block', async () => {
    render(SettingsView)

    const openRouterHeading = await screen.findByRole('heading', { name: 'OpenRouter' })
    const openRouterSection = openRouterHeading.closest('section')
    expect(openRouterSection).not.toBeNull()

    expect(screen.queryByRole('heading', { name: 'Embeddings BGE-M3' })).not.toBeInTheDocument()
    expect(within(openRouterSection!).getByLabelText('Modelo generativo')).toBeInTheDocument()
    expect(within(openRouterSection!).getByLabelText('Modelo de embeddings')).toBeInTheDocument()
  })

  it('opens provider API key links through the desktop bridge', async () => {
    render(SettingsView)

    await fireEvent.click(await screen.findByRole('link', { name: /OpenRouter/ }))
    await fireEvent.click(screen.getByRole('link', { name: /AssemblyAI/ }))
    await fireEvent.click(screen.getByRole('link', { name: /Z\.ai/ }))

    expect(invokeMock).toHaveBeenCalledWith('open_external_url', {
      url: 'https://openrouter.ai/settings/keys',
    })
    expect(invokeMock).toHaveBeenCalledWith('open_external_url', {
      url: 'https://www.assemblyai.com/app/account',
    })
    expect(invokeMock).toHaveBeenCalledWith('open_external_url', {
      url: 'https://z.ai/manage-apikey/apikey-list',
    })
  })

  it('shows refined success feedback for connection checks and saves', async () => {
    testOpenrouterConnectionMock.mockResolvedValue([
      { id: 'google/gemma-4-26b-a4b-it', name: 'Gemma 4 26B', context_length: 8192 },
      { id: 'anthropic/claude-3.7-sonnet', name: 'Claude 3.7 Sonnet', context_length: 200000 },
    ])

    render(SettingsView)

    const testButtons = await screen.findAllByRole('button', { name: 'Probar conexión' })
    expect(testButtons).toHaveLength(3)

    const openrouterTestButton = testButtons[0]
    const assemblyaiTestButton = testButtons[1]
    const glmOcrTestButton = testButtons[2]
    expect(openrouterTestButton).toBeDefined()
    expect(assemblyaiTestButton).toBeDefined()
    expect(glmOcrTestButton).toBeDefined()

    await waitFor(() => expect(openrouterTestButton!).toBeEnabled())
    await fireEvent.click(openrouterTestButton!)

    expect(await screen.findByText('Conexión lista · 2 modelos disponibles.')).toBeInTheDocument()
    expect(testOpenrouterConnectionMock).toHaveBeenCalledWith('sk-or-v1-test-key')
    expect(screen.getByText('Modelos sugeridos desde OpenRouter')).toBeInTheDocument()

    await fireEvent.click(assemblyaiTestButton!)

    expect(
      await screen.findByText('Conexión lista · AssemblyAI validó tu cuenta.')
    ).toBeInTheDocument()
    expect(testAssemblyaiConnectionMock).toHaveBeenCalledWith('aai-orig-test-1234')
    expect(screen.getByText(/aai-o\*\*\*\*\.\.\.\*\*\*\*1234/)).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(
      await screen.findByText(
        'Configuración guardada. Ya podés usar esta preferencia en toda la app.'
      )
    ).toBeInTheDocument()
    expect(settingsSetMock).toHaveBeenCalledWith('embedding_provider', 'api')
    expect(settingsSetMock).toHaveBeenCalledWith('openrouter_embedding_model', 'baai/bge-m3')
    expect(settingsSetMock).toHaveBeenCalledWith('llm_mode', 'openrouter')
    expect(settingsSetMock).toHaveBeenCalledWith('stt_mode', 'assemblyai')
    expect(settingsSetMock).toHaveBeenCalledWith('assemblyai_role_speaker_identification', 'true')
    expect(settingsSetMock).toHaveBeenCalledWith('ocrh_mode', 'glm_ocr')
  })

  it('loads collection audio AssemblyAI speaker labels enabled by default and saves it', async () => {
    render(SettingsView)

    const speakerSelect = await screen.findByLabelText('Identificación de hablantes en audio de colección')
    expect(speakerSelect).toHaveValue('true')

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(settingsSetMock).toHaveBeenCalledWith(
      'assemblyai_role_speaker_identification',
      'true'
    )
  })

  it('respects a saved false value for collection audio AssemblyAI speaker labels', async () => {
    settingsGetMock.mockImplementation(async (key: string) => {
      if (key === 'openrouter_api_key') return 'sk-or-v1-test-key'
      if (key === 'openrouter_model') return 'anthropic/claude-3.7-sonnet'
      if (key === 'openrouter_embedding_model') return 'baai/bge-m3'
      if (key === 'assemblyai_api_key') return 'aai-orig-test-1234'
      if (key === 'assemblyai_role_speaker_identification') return 'false'
      return null
    })

    render(SettingsView)

    const speakerSelect = await screen.findByLabelText('Identificación de hablantes en audio de colección')
    await waitFor(() => expect(speakerSelect).toHaveValue('false'))

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(settingsSetMock).toHaveBeenCalledWith(
      'assemblyai_role_speaker_identification',
      'false'
    )
  })

  it('does not expose or persist a note dictation speaker labels setting', async () => {
    render(SettingsView)

    await screen.findByLabelText('Identificación de hablantes en audio de colección')

    expect(screen.queryByLabelText(/dictado/i)).not.toBeInTheDocument()
    expect(settingsGetMock).not.toHaveBeenCalledWith('dictation_speaker_labels')

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(settingsSetMock).not.toHaveBeenCalledWith('dictation_speaker_labels', expect.any(String))
  })

  it('enables connection tests for saved keyring credentials without retyping secrets', async () => {
    settingsGetMock.mockImplementation(async (key: string) => {
      if (key === 'openrouter_api_key') return 'secret_ref:openrouter_api_key'
      if (key === 'assemblyai_api_key') return 'secret_ref:assemblyai_api_key'
      if (key === 'glm_ocr_api_key') return 'secret_ref:glm_ocr_api_key'
      if (key === 'openrouter_model') return 'anthropic/claude-3.7-sonnet'
      if (key === 'openrouter_embedding_model') return 'baai/bge-m3'
      return null
    })
    testOpenrouterConnectionMock.mockResolvedValue([
      { id: 'google/gemma-4-26b-a4b-it', name: 'Gemma 4 26B', context_length: 8192 },
    ])

    render(SettingsView)

    const testButtons = await screen.findAllByRole('button', { name: 'Probar conexión' })
    expect(testButtons).toHaveLength(3)
    await waitFor(() => {
      expect(testButtons[0]).toBeEnabled()
      expect(testButtons[1]).toBeEnabled()
      expect(testButtons[2]).toBeEnabled()
    })

    await fireEvent.click(testButtons[0]!)
    await fireEvent.click(testButtons[1]!)
    await fireEvent.click(testButtons[2]!)

    expect(testOpenrouterConnectionMock).toHaveBeenCalledWith('')
    expect(testAssemblyaiConnectionMock).toHaveBeenCalledWith('')
    expect(testGlmOcrConnectionMock).toHaveBeenCalledWith('')
  })

  it('shows a retryable error when initial settings fail to load', async () => {
    settingsGetMock.mockRejectedValueOnce(new Error('credential store unavailable'))

    render(SettingsView)

    expect(
      await screen.findByText(
        'No se pudo cargar la configuración guardada: credential store unavailable'
      )
    ).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Reintentar carga' }))

    expect(await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)).toBeInTheDocument()
  })
})

describe('settings dirty detection helpers', () => {
  const baseInput: SettingsSnapshotInput = {
    apiKey: '',
    model: 'anthropic/claude-3.7-sonnet',
    embeddingModel: 'baai/bge-m3',
    assemblyAiApiKey: '',
    assemblyAiCollectionSpeakerLabels: true,
    glmOcrApiKey: '',
    ocrCorrectionPrompt: 'Correct {text}',
    summaryPrompt: 'Summarize {text}',
    nerPrompt: 'NER {text}',
    tripletsPrompt: 'Triples {text}',
    modelParamsByFlow: {
      summary: { temperature: '0.2', maxTokens: '' },
    },
    ragParams: { topK: '6', temperature: '0.2' },
  }

  it('is clean when the current snapshot matches the saved baseline', () => {
    const saved = buildSettingsSnapshot(baseInput)
    expect(hasUnsavedSettingsChanges(saved, buildSettingsSnapshot({ ...baseInput }))).toBe(false)
  })

  it('flags top-level and nested model param changes as dirty', () => {
    const saved = buildSettingsSnapshot(baseInput)

    expect(
      hasUnsavedSettingsChanges(
        saved,
        buildSettingsSnapshot({ ...baseInput, model: 'openai/gpt-test' })
      )
    ).toBe(true)
    expect(
      hasUnsavedSettingsChanges(
        saved,
        buildSettingsSnapshot({ ...baseInput, summaryPrompt: 'Edited {text}' })
      )
    ).toBe(true)
    expect(
      hasUnsavedSettingsChanges(
        saved,
        buildSettingsSnapshot({
          ...baseInput,
          modelParamsByFlow: { summary: { temperature: '0.9', maxTokens: '' } },
        })
      )
    ).toBe(true)
    expect(
      hasUnsavedSettingsChanges(
        saved,
        buildSettingsSnapshot({ ...baseInput, ragParams: { topK: '12', temperature: '0.2' } })
      )
    ).toBe(true)
  })

  it('never reports dirty before a baseline snapshot exists', () => {
    expect(hasUnsavedSettingsChanges(null, buildSettingsSnapshot(baseInput))).toBe(false)
  })
})

describe('SettingsView Escape behavior', () => {
  beforeEach(() => {
    locale.set('es')
    invokeMock.mockReset().mockResolvedValue(undefined)
    settingsGetMock.mockReset()
    settingsGetAllMock.mockReset().mockResolvedValue([])
    settingsSetMock.mockReset().mockResolvedValue(undefined)
    settingsGetMock.mockImplementation(async (key: string) => {
      if (key === 'openrouter_api_key') return 'sk-or-v1-test-key'
      if (key === 'openrouter_model') return 'anthropic/claude-3.7-sonnet'
      if (key === 'openrouter_embedding_model') return 'baai/bge-m3'
      if (key === 'assemblyai_api_key') return 'aai-orig-test-1234'
      return null
    })
  })

  async function renderLoadedSettings() {
    render(SettingsView)
    await screen.findByText(/sk-o\*\*\*\*\.\.\.\*\*\*\*-key/)
  }

  it('lets Escape navigate back when settings have no unsaved changes', async () => {
    const backSpy = vi.spyOn(navigation, 'back').mockImplementation(() => {})
    const cleanupKeyboard = setupKeyboardShortcuts()

    try {
      await renderLoadedSettings()

      await fireEvent.keyDown(window, { key: 'Escape' })

      expect(backSpy).toHaveBeenCalledTimes(1)
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    } finally {
      cleanupKeyboard()
      backSpy.mockRestore()
    }
  })

  it('asks before discarding unsaved changes on Escape and navigates only after confirming', async () => {
    const backSpy = vi.spyOn(navigation, 'back').mockImplementation(() => {})
    const cleanupKeyboard = setupKeyboardShortcuts()

    try {
      await renderLoadedSettings()

      await fireEvent.input(screen.getByLabelText('Modelo generativo'), {
        target: { value: 'openai/gpt-test' },
      })

      await fireEvent.keyDown(window, { key: 'Escape' })

      expect(backSpy).not.toHaveBeenCalled()
      expect(await screen.findByRole('dialog')).toBeInTheDocument()
      expect(screen.getByText('Descartar cambios')).toBeInTheDocument()

      // Keep editing → dialog closes, still on settings.
      await fireEvent.click(screen.getByRole('button', { name: 'Seguir editando' }))
      await waitFor(() => {
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
      })
      expect(backSpy).not.toHaveBeenCalled()

      // Escape again and confirm the discard → now it navigates back.
      await fireEvent.keyDown(window, { key: 'Escape' })
      await fireEvent.click(await screen.findByRole('button', { name: 'Descartar' }))

      expect(backSpy).toHaveBeenCalledTimes(1)
      await waitFor(() => {
        expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
      })
    } finally {
      cleanupKeyboard()
      backSpy.mockRestore()
    }
  })

  it('does not prompt on Escape after saving the edited settings', async () => {
    const backSpy = vi.spyOn(navigation, 'back').mockImplementation(() => {})
    const cleanupKeyboard = setupKeyboardShortcuts()

    try {
      await renderLoadedSettings()

      await fireEvent.input(screen.getByLabelText('Modelo generativo'), {
        target: { value: 'openai/gpt-test' },
      })
      await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))
      await screen.findByText(
        'Configuración guardada. Ya podés usar esta preferencia en toda la app.'
      )

      await fireEvent.keyDown(window, { key: 'Escape' })

      expect(backSpy).toHaveBeenCalledTimes(1)
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    } finally {
      cleanupKeyboard()
      backSpy.mockRestore()
    }
  })
})
