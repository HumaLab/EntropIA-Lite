<script lang="ts">
  import { onMount } from 'svelte'
  import { locale, t } from '$lib/i18n'
  import {
    settingsGet,
    settingsSet,
    testOpenrouterConnection,
    testAssemblyaiConnection,
    testGlmOcrConnection,
    SETTINGS_KEYS,
    DEFAULT_OPENROUTER_MODEL,
    DEFAULT_OPENROUTER_EMBEDDING_MODEL,
    type ModelInfo,
  } from '$lib/settings'
  import { Button, Card, Input } from '@entropia/ui'
  import LogsTab from './LogsTab.svelte'

  let activeTab = $state<'api' | 'logs'>('api')

  // State
  let apiKey = $state('')
  let maskedApiKey = $state('')
  let showApiKey = $state(false)
  let model = $state(DEFAULT_OPENROUTER_MODEL)
  let embeddingModel = $state(DEFAULT_OPENROUTER_EMBEDDING_MODEL)
  let assemblyAiApiKey = $state('')
  let maskedAssemblyAiApiKey = $state('')
  let showAssemblyAiApiKey = $state(false)
  let glmOcrApiKey = $state('')
  let maskedGlmOcrApiKey = $state('')
  let showGlmOcrApiKey = $state(false)

  // Test connection state
  let testing = $state(false)
  let testResult = $state<{ success: boolean; message: string } | null>(null)
  let testingAssemblyAi = $state(false)
  let assemblyAiTestResult = $state<{ success: boolean; message: string } | null>(null)
  let testingGlmOcr = $state(false)
  let glmOcrTestResult = $state<{ success: boolean; message: string } | null>(null)
  let availableModels = $state<ModelInfo[]>([])

  const SECRET_REF_PREFIX = 'secret_ref:'
  const PROVIDER_LINKS = {
    openrouter: 'https://openrouter.ai/',
    assemblyai: 'https://www.assemblyai.com/',
    glmOcr: 'https://z.ai/manage-apikey/apikey-list',
  } as const

  // Save state
  let saving = $state(false)
  let saveFeedback = $state<{ tone: 'success' | 'error'; text: string } | null>(null)

  const activeLocale = $derived($locale)

  onMount(async () => {
    const [
      storedKey,
      storedModel,
      storedEmbeddingModel,
      storedAssemblyAiKey,
      storedGlmOcrKey,
    ] = await Promise.all([
      settingsGet(SETTINGS_KEYS.OPENROUTER_API_KEY),
      settingsGet(SETTINGS_KEYS.OPENROUTER_MODEL),
      settingsGet(SETTINGS_KEYS.OPENROUTER_EMBEDDING_MODEL),
      settingsGet(SETTINGS_KEYS.ASSEMBLYAI_API_KEY),
      settingsGet(SETTINGS_KEYS.GLM_OCR_API_KEY),
    ])

    if (storedKey?.startsWith(SECRET_REF_PREFIX)) {
      apiKey = ''
      maskedApiKey = 'Clave guardada en Windows Credential Manager'
    } else if (storedKey) {
      apiKey = storedKey
      maskedApiKey = maskKey(storedKey)
    }
    if (storedModel) model = storedModel
    if (storedEmbeddingModel) embeddingModel = storedEmbeddingModel
    if (storedAssemblyAiKey?.startsWith(SECRET_REF_PREFIX)) {
      assemblyAiApiKey = ''
      maskedAssemblyAiApiKey = 'Clave guardada en Windows Credential Manager'
    } else if (storedAssemblyAiKey) {
      assemblyAiApiKey = storedAssemblyAiKey
      maskedAssemblyAiApiKey = maskKey(storedAssemblyAiKey, 5)
    }
    if (storedGlmOcrKey?.startsWith(SECRET_REF_PREFIX)) {
      glmOcrApiKey = ''
      maskedGlmOcrApiKey = 'Clave guardada en Windows Credential Manager'
    } else if (storedGlmOcrKey) {
      glmOcrApiKey = storedGlmOcrKey
      maskedGlmOcrApiKey = maskKey(storedGlmOcrKey, 0)
    }
  })

  function maskKey(key: string, prefixLength = 4): string {
    const trimmed = key.trim()
    if (!trimmed) return ''
    if (trimmed.length <= prefixLength + 4) return '*'.repeat(trimmed.length)
    return `${trimmed.slice(0, prefixLength)}****...****${trimmed.slice(-4)}`
  }

  async function handleTestConnection() {
    if (!apiKey.trim()) {
      testResult = { success: false, message: t('settings.enterApiKey') }
      return
    }
    testing = true
    testResult = null
    try {
      const models = await testOpenrouterConnection(apiKey.trim())
      availableModels = models
      testResult = {
        success: true,
        message: t('settings.connectionReady', { count: models.length }),
      }
    } catch (e) {
      testResult = {
        success: false,
        message: e instanceof Error ? e.message : String(e),
      }
    } finally {
      testing = false
    }
  }

  async function handleTestAssemblyAiConnection() {
    if (!assemblyAiApiKey.trim()) {
      assemblyAiTestResult = { success: false, message: t('settings.enterAssemblyAiApiKey') }
      return
    }

    testingAssemblyAi = true
    assemblyAiTestResult = null
    try {
      await testAssemblyaiConnection(assemblyAiApiKey.trim())
      assemblyAiTestResult = {
        success: true,
        message: t('settings.assemblyAiConnectionReady'),
      }
    } catch (e) {
      assemblyAiTestResult = {
        success: false,
        message: e instanceof Error ? e.message : String(e),
      }
    } finally {
      testingAssemblyAi = false
    }
  }

  async function handleTestGlmOcrConnection() {
    if (!glmOcrApiKey.trim()) {
      glmOcrTestResult = { success: false, message: t('settings.enterGlmOcrApiKey') }
      return
    }

    testingGlmOcr = true
    glmOcrTestResult = null
    try {
      await testGlmOcrConnection(glmOcrApiKey.trim())
      glmOcrTestResult = {
        success: true,
        message: t('settings.glmOcrConnectionReady'),
      }
    } catch (e) {
      glmOcrTestResult = {
        success: false,
        message: e instanceof Error ? e.message : String(e),
      }
    } finally {
      testingGlmOcr = false
    }
  }

  async function handleSave() {
    saving = true
    saveFeedback = null
    try {
      const writes: Promise<void>[] = [
        settingsSet(SETTINGS_KEYS.OPENROUTER_MODEL, model),
        settingsSet(SETTINGS_KEYS.EMBEDDING_PROVIDER, 'api'),
        settingsSet(SETTINGS_KEYS.OPENROUTER_EMBEDDING_MODEL, embeddingModel.trim() || DEFAULT_OPENROUTER_EMBEDDING_MODEL),
        settingsSet(SETTINGS_KEYS.LLM_MODE, 'openrouter'),
        settingsSet(SETTINGS_KEYS.STT_MODE, 'assemblyai'),
        settingsSet(SETTINGS_KEYS.OCRH_MODE, 'glm_ocr'),
      ]
      if (apiKey.trim()) writes.push(settingsSet(SETTINGS_KEYS.OPENROUTER_API_KEY, apiKey.trim()))
      if (assemblyAiApiKey.trim()) writes.push(settingsSet(SETTINGS_KEYS.ASSEMBLYAI_API_KEY, assemblyAiApiKey.trim()))
      if (glmOcrApiKey.trim()) writes.push(settingsSet(SETTINGS_KEYS.GLM_OCR_API_KEY, glmOcrApiKey.trim()))
      await Promise.all(writes)
      if (apiKey.trim()) maskedApiKey = maskKey(apiKey)
      if (assemblyAiApiKey.trim()) maskedAssemblyAiApiKey = maskKey(assemblyAiApiKey, 5)
      if (glmOcrApiKey.trim()) maskedGlmOcrApiKey = maskKey(glmOcrApiKey, 0)
      saveFeedback = {
        tone: 'success',
        text: t('settings.saved'),
      }
      setTimeout(() => {
        saveFeedback = null
      }, 3000)
    } catch (e) {
      saveFeedback = {
        tone: 'error',
        text: `Error: ${e instanceof Error ? e.message : String(e)}`,
      }
    } finally {
      saving = false
    }
  }

  function handleModelSelect(modelId: string) {
    model = modelId
  }
</script>

{#key activeLocale}
  <div class="settings-view page-shell" data-locale={activeLocale}>
    <section class="page-header settings-view__header">
      <div class="page-header__content">
        <span class="page-header__eyebrow">{t('settings.preferences')}</span>
        <h1>{t('settings.title')}</h1>
        <p>{t('settings.subtitle')}</p>
        <span class="page-header__meta">{t('settings.remoteOnlyMeta')}</span>
      </div>

      <div class="page-toolbar settings-view__toolbar">
        <Button variant="primary" onclick={handleSave} disabled={saving}>
          {saving ? t('settings.saving') : t('settings.save')}
        </Button>
      </div>
    </section>

    <!-- Tab navigation -->
    <nav class="settings-tabs" aria-label="Secciones de configuración">
      <button
        class="settings-tab"
        class:settings-tab--active={activeTab === 'api'}
        type="button"
        onclick={() => (activeTab = 'api')}
      >
        APIs remotas
      </button>
      <button
        class="settings-tab"
        class:settings-tab--active={activeTab === 'logs'}
        type="button"
        onclick={() => (activeTab = 'logs')}
      >
        Logs
      </button>
    </nav>

    {#if activeTab === 'api'}
    <p class="settings__hint settings__hint--privacy">
      {t('settings.apiPrivacyNotice')}
    </p>

    {#if saveFeedback}
      <p
        class="surface-message"
        class:surface-message--error={saveFeedback.tone === 'error'}
        class:surface-message--success={saveFeedback.tone === 'success'}
      >
        {saveFeedback.text}
      </p>
    {/if}

    <Card>
      <section class="settings-card-section">
        <div class="settings-card-section__copy">
          <h2>{t('settings.embeddingProvider.title')}</h2>
          <p>{t('settings.embeddingProvider.description')}</p>
        </div>

        <div class="settings__field settings__field--stacked">
          <Input
            label={t('settings.embeddingProvider.model')}
            type="text"
            bind:value={embeddingModel}
            placeholder={DEFAULT_OPENROUTER_EMBEDDING_MODEL}
          />
          <p class="settings__hint">{t('settings.embeddingProvider.modelHint')}</p>
        </div>
      </section>
    </Card>

    <Card>
      <section class="settings-card-section">
        <div class="settings-card-section__copy">
          <h2>{t('settings.openrouter.title')}</h2>
          <p>{t('settings.openrouter.description')}</p>
          <a class="settings__provider-link" href={PROVIDER_LINKS.openrouter} target="_blank" rel="noreferrer">
            Obtener API key en OpenRouter ↗
          </a>
        </div>

        <div class="settings__field settings__field--stacked">
          <label class="settings__label" for="api-key">{t('settings.apiKey')}</label>
          <div class="settings__input-row">
            {#if showApiKey}
              <input
                id="api-key"
                type="text"
                class="settings__input"
                bind:value={apiKey}
                placeholder={t('settings.apiKeyPlaceholder')}
              />
            {:else}
              <input
                id="api-key"
                type="password"
                class="settings__input"
                bind:value={apiKey}
                placeholder={t('settings.apiKeyPlaceholder')}
              />
            {/if}
            <button
              class="settings__icon-btn"
              type="button"
              onclick={() => (showApiKey = !showApiKey)}
              title={showApiKey ? t('settings.hideApiKey') : t('settings.showApiKey')}
              aria-label={showApiKey ? t('settings.hideApiKey') : t('settings.showApiKey')}
            >
              {showApiKey ? '🙈' : '👁'}
            </button>
            <Button
              variant="secondary"
              size="sm"
              onclick={handleTestConnection}
              disabled={testing || !apiKey.trim()}
            >
              {testing ? t('settings.testingConnection') : t('settings.testConnection')}
            </Button>
          </div>

          {#if maskedApiKey}
            <p class="settings__hint">{t('settings.loadedKey', { key: maskedApiKey })}</p>
          {/if}

          {#if testResult}
            <p
              class="surface-message settings__feedback"
              class:surface-message--success={testResult.success}
              class:surface-message--error={!testResult.success}
            >
              {testResult.message}
            </p>
          {/if}
        </div>

        <div class="settings__field settings__field--stacked">
          <Input
            label={t('settings.model')}
            type="text"
            bind:value={model}
            placeholder={t('settings.modelPlaceholder')}
          />

          {#if availableModels.length > 0}
            <div class="settings__model-list">
              <p class="settings__model-list-title">{t('settings.suggestedModels')}</p>
              {#each availableModels
                .filter((m) => m.id.includes('gemma') || m.id.includes('llama') || m.id.includes('mistral') || m.id.includes('qwen') || m.id.includes('claude') || m.id.includes('gpt'))
                .slice(0, 15) as m (m.id)}
                <button
                  class="settings__model-option"
                  type="button"
                  class:selected={model === m.id}
                  onclick={() => handleModelSelect(m.id)}
                >
                  <span class="settings__model-id">{m.id}</span>
                  <span class="settings__model-ctx">{Math.round(m.context_length / 1024)}k ctx</span
                  >
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </section>
    </Card>

    <Card>
      <section class="settings-card-section">
        <div class="settings-card-section__copy">
          <h2>{t('settings.assemblyai.title')}</h2>
          <p>{t('settings.assemblyai.description')}</p>
          <a class="settings__provider-link" href={PROVIDER_LINKS.assemblyai} target="_blank" rel="noreferrer">
            Obtener API key en AssemblyAI ↗
          </a>
        </div>

        <div class="settings__field settings__field--stacked">
          <label class="settings__label" for="assemblyai-api-key">{t('settings.apiKey')}</label>
          <div class="settings__input-row">
            <input
              id="assemblyai-api-key"
              type={showAssemblyAiApiKey ? 'text' : 'password'}
              class="settings__input"
              bind:value={assemblyAiApiKey}
              placeholder={t('settings.assemblyAiApiKeyPlaceholder')}
            />
            <button
              class="settings__icon-btn"
              type="button"
              onclick={() => (showAssemblyAiApiKey = !showAssemblyAiApiKey)}
              title={showAssemblyAiApiKey ? t('settings.hideApiKey') : t('settings.showApiKey')}
              aria-label={showAssemblyAiApiKey ? t('settings.hideApiKey') : t('settings.showApiKey')}
            >
              {showAssemblyAiApiKey ? '🙈' : '👁'}
            </button>
            <Button
              variant="secondary"
              size="sm"
              onclick={handleTestAssemblyAiConnection}
              disabled={testingAssemblyAi || !assemblyAiApiKey.trim()}
            >
              {testingAssemblyAi ? t('settings.testingConnection') : t('settings.testConnection')}
            </Button>
          </div>

          {#if maskedAssemblyAiApiKey}
            <p class="settings__hint">{t('settings.loadedKey', { key: maskedAssemblyAiApiKey })}</p>
          {/if}

          {#if assemblyAiTestResult}
            <p
              class="surface-message settings__feedback"
              class:surface-message--success={assemblyAiTestResult.success}
              class:surface-message--error={!assemblyAiTestResult.success}
            >
              {assemblyAiTestResult.message}
            </p>
          {/if}
        </div>
      </section>
    </Card>

    <Card>
      <section class="settings-card-section">
        <div class="settings-card-section__copy">
          <h2>{t('settings.glmOcr.title')}</h2>
          <p>{t('settings.glmOcr.description')}</p>
          <a class="settings__provider-link" href={PROVIDER_LINKS.glmOcr} target="_blank" rel="noreferrer">
            Obtener API key en Z.ai ↗
          </a>
        </div>

        <div class="settings__field settings__field--stacked">
          <label class="settings__label" for="glm-ocr-api-key">{t('settings.apiKey')}</label>
          <div class="settings__input-row">
            <input
              id="glm-ocr-api-key"
              type={showGlmOcrApiKey ? 'text' : 'password'}
              class="settings__input"
              bind:value={glmOcrApiKey}
              placeholder={t('settings.glmOcrApiKeyPlaceholder')}
            />
            <button
              class="settings__icon-btn"
              type="button"
              onclick={() => (showGlmOcrApiKey = !showGlmOcrApiKey)}
              title={showGlmOcrApiKey ? t('settings.hideApiKey') : t('settings.showApiKey')}
              aria-label={showGlmOcrApiKey ? t('settings.hideApiKey') : t('settings.showApiKey')}
            >
              {showGlmOcrApiKey ? '🙈' : '👁'}
            </button>
            <Button
              variant="secondary"
              size="sm"
              onclick={handleTestGlmOcrConnection}
              disabled={testingGlmOcr || !glmOcrApiKey.trim()}
            >
              {testingGlmOcr ? t('settings.testingConnection') : t('settings.testConnection')}
            </Button>
          </div>

          {#if maskedGlmOcrApiKey}
            <p class="settings__hint">{t('settings.loadedKey', { key: maskedGlmOcrApiKey })}</p>
          {/if}

          {#if glmOcrTestResult}
            <p
              class="surface-message settings__feedback"
              class:surface-message--success={glmOcrTestResult.success}
              class:surface-message--error={!glmOcrTestResult.success}
            >
              {glmOcrTestResult.message}
            </p>
          {/if}
        </div>
      </section>
    </Card>

    {:else if activeTab === 'logs'}
    <LogsTab />
    {/if}
  </div>
{/key}

<style>
  .settings-view {
    min-height: 100%;
  }

  /* Tab navigation */
  .settings-tabs {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--color-border-subtle);
    margin-bottom: var(--space-1);
  }

  .settings-tab {
    padding: var(--space-2) var(--space-5);
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    color: var(--color-text-secondary);
    transition:
      color 0.15s ease,
      border-color 0.15s ease;
    margin-bottom: -1px;
  }

  .settings-tab:hover {
    color: var(--color-text-primary);
  }

  .settings-tab--active {
    color: var(--color-accent);
    border-bottom-color: var(--color-accent);
  }

  .settings-view__toolbar {
    justify-content: flex-end;
    flex: 1;
    align-self: center;
  }

  .settings-view__header {
    border-bottom-color: color-mix(in srgb, var(--color-success) 18%, var(--border-subtle));
    background: linear-gradient(
      180deg,
      color-mix(in srgb, var(--color-success-soft) 34%, transparent),
      transparent 72%
    );
  }

  .settings-view__header .page-header__eyebrow {
    color: color-mix(in srgb, var(--color-success) 78%, white 22%);
  }

  .settings-view__header .page-header__meta {
    color: var(--color-text-secondary);
    line-height: 1.5;
  }

  .settings-view :global(.card) {
    border-color: color-mix(in srgb, var(--color-success) 14%, var(--color-hairline));
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--color-success-soft) 34%, transparent), transparent 72%),
      color-mix(in srgb, var(--color-surface-glass) 74%, transparent);
    box-shadow: var(--shadow-sm);
    backdrop-filter: blur(10px);
  }

  .settings-view :global(.card__header),
  .settings-view :global(.card__footer) {
    background-color: color-mix(in srgb, var(--color-surface-glass) 70%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 12%, var(--color-hairline));
  }

  .settings-view :global(.card__body) {
    background: transparent;
  }

  .settings-card-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  .settings-card-section__copy {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .settings-card-section__copy h2 {
    margin: 0;
    font-size: var(--font-size-base);
    font-weight: var(--font-weight-semibold);
    letter-spacing: -0.01em;
  }

  .settings-card-section__copy p,
  .settings__hint {
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    line-height: 1.6;
    margin: 0;
  }

  .settings__provider-link {
    display: inline-flex;
    width: fit-content;
    color: var(--color-accent);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    text-decoration: none;
  }

  .settings__provider-link:hover {
    text-decoration: underline;
  }

  .settings__field {
    margin-bottom: var(--space-1);
  }

  .settings__field--stacked {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .settings__label {
    display: block;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    color: color-mix(in srgb, var(--color-text-secondary) 86%, white 14%);
    margin-bottom: var(--space-1);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .settings__input-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
  }

  .settings__input {
    flex: 1;
    min-height: var(--control-height-md);
    padding: 0 var(--space-3);
    border: 1px solid color-mix(in srgb, var(--color-hairline) 78%, transparent);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-surface-glass) 78%, transparent);
    color: var(--color-text-primary);
    font-family: var(--font-mono, monospace);
    font-size: var(--font-size-sm);
  }

  .settings__input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: var(--focus-ring);
    background: color-mix(in srgb, var(--color-surface-glass) 88%, transparent);
  }

  .settings__icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: var(--control-height-md);
    height: var(--control-height-md);
    border: 1px solid color-mix(in srgb, var(--color-hairline) 78%, transparent);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-surface-glass) 78%, transparent);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: 14px;
  }

  .settings__icon-btn:hover {
    border-color: color-mix(in srgb, var(--color-accent) 18%, var(--color-hairline));
    background: color-mix(in srgb, var(--color-surface-glass) 88%, transparent);
  }

  .settings-view :global(.input-field__input) {
    border-color: color-mix(in srgb, var(--color-hairline) 78%, transparent);
    background-color: color-mix(in srgb, var(--color-surface-glass) 78%, transparent);
  }

  .settings-view :global(.input-field__input:focus),
  .settings-view :global(.input-field__input:focus-visible) {
    background-color: color-mix(in srgb, var(--color-surface-glass) 88%, transparent);
  }

  .settings-view :global(.btn--secondary) {
    border-color: color-mix(in srgb, var(--color-hairline) 78%, transparent);
    background:
      linear-gradient(180deg, rgba(255, 255, 255, 0.04), transparent 55%),
      color-mix(in srgb, var(--color-surface-glass) 78%, transparent);
    box-shadow: none;
  }

  .settings-view :global(.btn--secondary:hover:not(:disabled)) {
    border-color: color-mix(in srgb, var(--color-accent) 18%, var(--color-hairline));
    background-color: color-mix(in srgb, var(--color-surface-glass) 88%, transparent);
  }

  .settings__feedback {
    margin: 0;
    line-height: 1.55;
  }

  .settings__hint--privacy {
    margin: 0;
    padding: var(--space-3);
    border: 1px solid color-mix(in srgb, var(--color-warning) 35%, transparent);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-warning) 10%, var(--color-surface-glass));
  }

  .settings__model-list {
    max-height: 240px;
    overflow-y: auto;
    border: 1px solid color-mix(in srgb, var(--color-hairline) 78%, transparent);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--color-surface-glass) 72%, transparent);
  }

  .settings__model-list-title {
    padding: var(--space-2) var(--space-3);
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    border-bottom: 1px solid color-mix(in srgb, var(--color-hairline) 72%, transparent);
  }
  .settings__model-option {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: none;
    background: transparent;
    cursor: pointer;
    font-family: var(--font-sans);
    font-size: var(--font-size-sm);
    text-align: left;
    transition: background-color var(--transition-smooth);
  }
  .settings__model-option:hover {
    background: color-mix(in srgb, var(--color-surface-glass) 82%, transparent);
  }

  .settings__model-option.selected {
    background: color-mix(in srgb, var(--color-accent) 10%, var(--color-surface-glass));
    font-weight: var(--font-weight-medium);
  }

  .settings__model-option + .settings__model-option {
    border-top: 1px solid var(--color-border-subtle);
  }

  .settings__model-id {
    color: var(--color-text-primary);
  }

  .settings__model-ctx {
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
  }

  @media (max-width: 720px) {
    .settings-view__toolbar,
    .settings__input-row {
      width: 100%;
    }

    .settings-view__toolbar :global(.btn),
    .settings__input-row :global(.btn) {
      width: 100%;
    }

    .settings__icon-btn {
      flex: 0 0 auto;
    }
  }
</style>
