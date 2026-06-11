<script lang="ts">
  import { navigation } from '$lib/navigation'
  import { locale, t } from '$lib/i18n'
  import { ragAsk, type RagChatTurn, type RagSource } from '$lib/rag'
  import { Button } from '@entropia/ui'

  interface ChatMessage {
    role: 'user' | 'assistant'
    content: string
    sources?: RagSource[]
  }

  let messages = $state<ChatMessage[]>([])
  let draft = $state('')
  let loading = $state(false)
  let errorMessage = $state<string | null>(null)
  let messagesEl = $state<HTMLDivElement | undefined>()
  let askRequestId = 0

  const currentLocale = locale
  const canSend = $derived(!loading && draft.trim().length > 0)

  function formatTimestamp(seconds: number): string {
    const total = Math.max(0, Math.floor(seconds))
    const minutes = Math.floor(total / 60)
    const rest = total % 60
    return `${minutes}:${String(rest).padStart(2, '0')}`
  }

  function sourceTimestamp(source: RagSource): string | null {
    if (source.startSeconds == null) return null
    const start = formatTimestamp(source.startSeconds)
    if (source.endSeconds == null) return start
    return `${start}–${formatTimestamp(source.endSeconds)}`
  }

  function describeError(error: unknown): string {
    if (typeof error === 'string' && error.trim()) return error
    if (error instanceof Error && error.message) return error.message
    return t('ragChat.errorGeneric')
  }

  async function handleSend() {
    const question = draft.trim()
    if (!question || loading) return

    const history: RagChatTurn[] = messages.map((message) => ({
      role: message.role,
      content: message.content,
    }))

    messages = [...messages, { role: 'user', content: question }]
    draft = ''
    errorMessage = null
    loading = true
    const requestId = ++askRequestId

    try {
      const response = await ragAsk(question, history)
      if (requestId !== askRequestId) return

      if (response.answer.trim() === '' && response.sources.length === 0) {
        messages = [...messages, { role: 'assistant', content: t('ragChat.noResults') }]
      } else {
        messages = [
          ...messages,
          { role: 'assistant', content: response.answer, sources: response.sources },
        ]
      }
    } catch (error) {
      if (requestId !== askRequestId) return
      errorMessage = describeError(error)
    } finally {
      if (requestId === askRequestId) {
        loading = false
      }
    }
  }

  function handleClear() {
    askRequestId += 1
    messages = []
    draft = ''
    errorMessage = null
    loading = false
  }

  function handleComposerKeydown(event: KeyboardEvent) {
    // keyCode 229 cubre WKWebView, donde isComposing puede no reportarse durante IME.
    if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault()
      void handleSend()
    }
  }

  function openSource(source: RagSource) {
    navigation.navigate({
      name: 'item',
      collectionId: source.collectionId,
      collectionName: source.collectionName,
      itemId: source.itemId,
      itemTitle: source.itemTitle,
      assetId: source.assetId,
    })
  }

  $effect(() => {
    void messages.length
    void loading
    if (messagesEl) {
      messagesEl.scrollTop = messagesEl.scrollHeight
    }
  })
</script>

<div class="rag-chat page-shell">
  <section class="page-header rag-chat__header" aria-labelledby="rag-chat-title">
    <div class="page-header__content">
      <h1 id="rag-chat-title">{$currentLocale && t('ragChat.title')}</h1>
      <p>{$currentLocale && t('ragChat.subtitle')}</p>
    </div>
    <div class="page-toolbar">
      <Button variant="ghost" onclick={handleClear}>
        {$currentLocale && t('ragChat.clear')}
      </Button>
    </div>
  </section>

  <div
    class="rag-chat__messages"
    bind:this={messagesEl}
    role="log"
    aria-live="polite"
    aria-label={$currentLocale && t('ragChat.title')}
  >
    {#if messages.length === 0 && !loading}
      <p class="surface-message surface-message--center rag-chat__empty">
        {$currentLocale && t('ragChat.emptyState')}
      </p>
    {/if}

    {#each messages as message, index (index)}
      <article
        class="rag-chat__bubble"
        class:rag-chat__bubble--user={message.role === 'user'}
        class:rag-chat__bubble--assistant={message.role === 'assistant'}
      >
        <p class="rag-chat__content">{message.content}</p>

        {#if message.sources && message.sources.length > 0}
          <section class="rag-chat__sources" aria-label={$currentLocale && t('ragChat.sources')}>
            <h2 class="rag-chat__sources-title">{$currentLocale && t('ragChat.sources')}</h2>
            <ul class="rag-chat__sources-list">
              {#each message.sources as source (`${source.index}-${source.assetId}`)}
                {@const timestamp = sourceTimestamp(source)}
                <li>
                  <button
                    type="button"
                    class="rag-chat__source"
                    onclick={() => openSource(source)}
                    aria-label={$currentLocale &&
                      `${t('ragChat.openSource')}: [${source.index}] ${source.itemTitle}`}
                    title={$currentLocale && t('ragChat.openSource')}
                  >
                    <span class="rag-chat__source-heading">
                      <span class="rag-chat__source-ref">[{source.index}]</span>
                      <span class="rag-chat__source-name"
                        >{source.itemTitle} ({source.collectionName})</span
                      >
                      {#if timestamp}
                        <span class="rag-chat__source-time">{timestamp}</span>
                      {/if}
                    </span>
                    <span class="rag-chat__source-snippet">{source.snippet}</span>
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </article>
    {/each}

    {#if loading}
      <p class="rag-chat__thinking" role="status">
        {$currentLocale && t('ragChat.thinking')}
      </p>
    {/if}
  </div>

  {#if errorMessage}
    <p class="surface-message surface-message--error" role="alert">{errorMessage}</p>
  {/if}

  <form
    class="rag-chat__composer"
    onsubmit={(event) => {
      event.preventDefault()
      void handleSend()
    }}
  >
    <textarea
      class="rag-chat__input"
      rows="2"
      maxlength="4000"
      bind:value={draft}
      placeholder={$currentLocale && t('ragChat.placeholder')}
      aria-label={$currentLocale && t('ragChat.placeholder')}
      onkeydown={handleComposerKeydown}
      disabled={loading}
    ></textarea>
    <Button variant="primary" type="submit" disabled={!canSend}>
      {$currentLocale && t('ragChat.send')}
    </Button>
  </form>
</div>

<style>
  .rag-chat {
    height: 100%;
    min-height: 0;
  }

  .rag-chat__header {
    flex-shrink: 0;
  }

  .rag-chat__messages {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-2) var(--space-1);
  }

  .rag-chat__empty {
    margin: auto;
    max-width: 48ch;
  }

  .rag-chat__bubble {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: min(100%, 72ch);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-dialog);
  }

  .rag-chat__bubble--user {
    align-self: flex-end;
    background: color-mix(in srgb, var(--color-accent) 14%, var(--color-surface-glass));
    border-color: color-mix(in srgb, var(--color-accent) 24%, var(--border-subtle));
  }

  .rag-chat__bubble--assistant {
    align-self: flex-start;
    background: var(--surface-panel);
  }

  .rag-chat__content {
    margin: 0;
    color: var(--color-text-primary);
    font-size: var(--font-size-sm);
    line-height: var(--line-height-base, 1.5);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .rag-chat__sources {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid var(--border-subtle);
  }

  .rag-chat__sources-title {
    margin: 0;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    letter-spacing: 0.075em;
    text-transform: uppercase;
    color: var(--color-text-muted);
  }

  .rag-chat__sources-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .rag-chat__source {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-sans);
    transition:
      background-color var(--transition-base),
      border-color var(--transition-base);
  }

  .rag-chat__source:hover {
    background: var(--surface-toolbar);
    border-color: var(--border-subtle);
  }

  .rag-chat__source:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .rag-chat__source-heading {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2);
  }

  .rag-chat__source-ref {
    color: var(--color-accent);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
  }

  .rag-chat__source-name {
    color: var(--color-text-primary);
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
  }

  .rag-chat__source-time {
    color: var(--color-text-muted);
    font-size: var(--font-size-xs);
    font-variant-numeric: tabular-nums;
  }

  .rag-chat__source-snippet {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
  }

  .rag-chat__thinking {
    align-self: flex-start;
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    background: var(--surface-toolbar);
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
  }

  .rag-chat__composer {
    display: flex;
    align-items: flex-end;
    gap: var(--space-2);
    flex-shrink: 0;
    padding-top: var(--space-2);
    border-top: 1px solid var(--border-subtle);
  }

  .rag-chat__input {
    flex: 1;
    min-height: var(--control-height-lg);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-input);
    background: var(--surface-input);
    color: var(--color-text-primary);
    font-family: var(--font-sans);
    font-size: var(--font-size-sm);
    resize: vertical;
  }

  .rag-chat__input:focus {
    outline: none;
    border-color: var(--color-accent);
    box-shadow: var(--focus-ring);
    background: var(--surface-panel);
  }

  .rag-chat__input:disabled {
    opacity: 0.6;
  }

  @media (max-width: 720px) {
    .rag-chat__composer {
      flex-direction: column;
      align-items: stretch;
    }
  }
</style>
