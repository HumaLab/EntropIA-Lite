import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import SettingsView from './SettingsView.svelte'
import { locale } from '$lib/i18n'

const {
  settingsGetMock,
  settingsSetMock,
  testOpenrouterConnectionMock,
  testAssemblyaiConnectionMock,
  testGlmOcrConnectionMock,
} =
  vi.hoisted(() => ({
    settingsGetMock: vi.fn(),
    settingsSetMock: vi.fn(),
    testOpenrouterConnectionMock: vi.fn(),
    testAssemblyaiConnectionMock: vi.fn(),
    testGlmOcrConnectionMock: vi.fn(),
  }))

vi.mock('$lib/settings', async () => {
  const actual = await vi.importActual<typeof import('$lib/settings')>('$lib/settings')
  return {
    ...actual,
    settingsGet: settingsGetMock,
    settingsSet: settingsSetMock,
    testOpenrouterConnection: testOpenrouterConnectionMock,
    testAssemblyaiConnection: testAssemblyaiConnectionMock,
    testGlmOcrConnection: testGlmOcrConnectionMock,
  }
})

describe('SettingsView', () => {
  beforeEach(() => {
    locale.set('es')
    settingsGetMock.mockReset()
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
    expect(screen.getByRole('button', { name: 'APIs remotas' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Logs' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Dependencias de IA' })).not.toBeInTheDocument()
  })

  it('shows refined success feedback for connection checks and saves', async () => {
    testOpenrouterConnectionMock.mockResolvedValue([
      { id: 'google/gemma-3-4b-it', name: 'Gemma 3 4B', context_length: 8192 },
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

    await fireEvent.click(openrouterTestButton!)

    expect(await screen.findByText('Conexión lista · 2 modelos disponibles.')).toBeInTheDocument()
    expect(screen.getByText('Modelos sugeridos desde OpenRouter')).toBeInTheDocument()

    await fireEvent.click(assemblyaiTestButton!)

    expect(
      await screen.findByText('Conexión lista · AssemblyAI validó tu cuenta.')
    ).toBeInTheDocument()
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
    expect(settingsSetMock).toHaveBeenCalledWith('ocrh_mode', 'glm_ocr')
  })
})
