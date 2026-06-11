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
