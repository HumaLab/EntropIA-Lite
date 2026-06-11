import { beforeEach, describe, expect, it, vi } from 'vitest'
import { get } from 'svelte/store'

const { settingsGetMock, settingsSetMock } = vi.hoisted(() => ({
  settingsGetMock: vi.fn(),
  settingsSetMock: vi.fn(),
}))

vi.mock('$lib/settings', async () => {
  const actual = await vi.importActual<typeof import('$lib/settings')>('$lib/settings')
  return {
    ...actual,
    settingsGet: settingsGetMock,
    settingsSet: settingsSetMock,
  }
})

describe('i18n', () => {
  beforeEach(async () => {
    settingsGetMock.mockReset().mockResolvedValue(null)
    settingsSetMock.mockReset().mockResolvedValue(undefined)

    const { locale } = await import('./i18n')
    locale.set('es')
  })

  it('defaults to spanish when no preference is stored', async () => {
    const { initLocale, locale, t } = await import('./i18n')

    await initLocale()

    expect(settingsGetMock).toHaveBeenCalledWith('language')
    expect(get(locale)).toBe('es')
    expect(t('app.initializing')).toBe('Inicializando...')
  })

  it('loads a saved language preference when it exists', async () => {
    settingsGetMock.mockResolvedValueOnce('en')

    const { initLocale, locale, t } = await import('./i18n')

    await initLocale()

    expect(get(locale)).toBe('en')
    expect(t('app.initializing')).toBe('Initializing...')
  })

  it('persists locale changes through frontend settings', async () => {
    const { setLocale, locale } = await import('./i18n')

    await setLocale('en')

    expect(settingsSetMock).toHaveBeenCalledWith('language', 'en')
    expect(get(locale)).toBe('en')
  })

  it('exposes db browser action copy in both locales', async () => {
    const { locale, t } = await import('./i18n')

    expect(t('dbBrowser.copyCell')).toBe('Copiar')
    expect(t('dbBrowser.pageSizeLabel')).toBe('Filas por página')

    locale.set('en')

    expect(t('dbBrowser.copyCell')).toBe('Copy')
    expect(t('dbBrowser.pageSizeLabel')).toBe('Rows per page')
  })

  it('exposes rag chat copy in both locales', async () => {
    const { locale, t } = await import('./i18n')

    expect(t('nav.ragChat')).toBe('Chat')
    expect(t('topbar.ragChatAria')).toBe('Abrir chat de investigación')
    expect(t('topbar.ragChatTitle')).toBe('Chat de investigación')
    expect(t('ragChat.title')).toBe('Chat de investigación')
    expect(t('ragChat.subtitle')).toBe('Consultá la base de conocimiento de transcripciones')
    expect(t('ragChat.placeholder')).toBe('Escribí tu pregunta…')
    expect(t('ragChat.send')).toBe('Enviar')
    expect(t('ragChat.thinking')).toBe('Buscando en las transcripciones…')
    expect(t('ragChat.sources')).toBe('Fuentes')
    expect(t('ragChat.noResults')).toBe(
      'No encontré contenido relevante en las transcripciones para esa pregunta.'
    )
    expect(t('ragChat.emptyState')).toBe(
      'Hacé una pregunta sobre tus transcripciones. Las respuestas citan las fuentes.'
    )
    expect(t('ragChat.errorGeneric')).toBe('Ocurrió un error al consultar.')
    expect(t('ragChat.clear')).toBe('Nueva conversación')
    expect(t('ragChat.openSource')).toBe('Abrir fuente')

    locale.set('en')

    expect(t('nav.ragChat')).toBe('Chat')
    expect(t('topbar.ragChatAria')).toBe('Open research chat')
    expect(t('topbar.ragChatTitle')).toBe('Research chat')
    expect(t('ragChat.title')).toBe('Research chat')
    expect(t('ragChat.subtitle')).toBe('Query the transcription knowledge base')
    expect(t('ragChat.placeholder')).toBe('Type your question…')
    expect(t('ragChat.send')).toBe('Send')
    expect(t('ragChat.thinking')).toBe('Searching the transcriptions…')
    expect(t('ragChat.sources')).toBe('Sources')
    expect(t('ragChat.noResults')).toBe(
      'I did not find relevant content in the transcriptions for that question.'
    )
    expect(t('ragChat.emptyState')).toBe(
      'Ask a question about your transcriptions. Answers cite their sources.'
    )
    expect(t('ragChat.errorGeneric')).toBe('Something went wrong while querying.')
    expect(t('ragChat.clear')).toBe('New conversation')
    expect(t('ragChat.openSource')).toBe('Open source')
  })

  it('exposes settings prompts and model params copy in both locales', async () => {
    const { locale, t } = await import('./i18n')

    expect(t('settings.prompts.validate')).toBe('Validar cambios')
    expect(t('settings.promptValidation.valid')).toBe('Prompt válido.')
    expect(t('settings.promptValidation.missingText')).toBe('Debe incluir el placeholder {text}.')
    expect(t('settings.getApiKeyLink', { provider: 'OpenRouter' })).toBe(
      'Obtener API key en OpenRouter'
    )
    expect(t('settings.modelParams.invalidParam', { flow: 'Summary', param: 'topP' })).toBe(
      'Parámetro inválido en Summary: topP'
    )

    locale.set('en')

    expect(t('settings.prompts.validate')).toBe('Validate changes')
    expect(t('settings.promptValidation.valid')).toBe('Prompt is valid.')
    expect(t('settings.promptValidation.missingText')).toBe('It must include the {text} placeholder.')
    expect(t('settings.getApiKeyLink', { provider: 'OpenRouter' })).toBe(
      'Get an API key at OpenRouter'
    )
    expect(t('settings.modelParams.invalidParam', { flow: 'Summary', param: 'topP' })).toBe(
      'Invalid parameter in Summary: topP'
    )
  })
})
