import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SettingsView from './SettingsView.svelte'
import { locale } from '$lib/i18n'

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
    expect(screen.getByRole('tab', { name: 'Logs' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Dependencias de IA' })).not.toBeInTheDocument()
  })

  it('edits and saves prompt and model parameter settings', async () => {
    render(SettingsView)

    await fireEvent.click(await screen.findByRole('tab', { name: 'Prompts' }))
    const ocrPrompt = screen.getByLabelText('OCR correction prompt')
    await fireEvent.input(ocrPrompt, { target: { value: 'Custom OCR {text}' } })

    await fireEvent.click(screen.getByRole('tab', { name: 'Model Params' }))
    await fireEvent.input(screen.getAllByLabelText('temperature (0-2)')[0], { target: { value: '0.6' } })
    await fireEvent.input(screen.getAllByLabelText('maxTokens (1-32000, vacío = default)')[0], {
      target: { value: '1234' },
    })

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(settingsSetMock).toHaveBeenCalledWith('prompt_ocr_correction', 'Custom OCR {text}')
    expect(settingsSetMock).toHaveBeenCalledWith('llm_ocr_correction_temperature', '0.6')
    expect(settingsSetMock).toHaveBeenCalledWith('llm_ocr_correction_max_tokens', '1234')
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
    expect(settingsSetMock).toHaveBeenCalledWith(
      'assemblyai_role_speaker_identification',
      'true'
    )
    expect(settingsSetMock).toHaveBeenCalledWith('ocrh_mode', 'glm_ocr')
  })

  it('loads AssemblyAI speaker labels enabled by default and saves it', async () => {
    render(SettingsView)

    const speakerSelect = await screen.findByLabelText('Identificación de hablantes')
    expect(speakerSelect).toHaveValue('true')

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(settingsSetMock).toHaveBeenCalledWith(
      'assemblyai_role_speaker_identification',
      'true'
    )
  })

  it('respects a saved false value for AssemblyAI speaker labels', async () => {
    settingsGetMock.mockImplementation(async (key: string) => {
      if (key === 'openrouter_api_key') return 'sk-or-v1-test-key'
      if (key === 'openrouter_model') return 'anthropic/claude-3.7-sonnet'
      if (key === 'openrouter_embedding_model') return 'baai/bge-m3'
      if (key === 'assemblyai_api_key') return 'aai-orig-test-1234'
      if (key === 'assemblyai_role_speaker_identification') return 'false'
      return null
    })

    render(SettingsView)

    const speakerSelect = await screen.findByLabelText('Identificación de hablantes')
    await waitFor(() => expect(speakerSelect).toHaveValue('false'))

    await fireEvent.click(screen.getByRole('button', { name: 'Guardar cambios' }))

    expect(settingsSetMock).toHaveBeenCalledWith(
      'assemblyai_role_speaker_identification',
      'false'
    )
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
