<script lang="ts">
  import { getTermById } from '$lib/glossary'

  type Props = {
    id: string
  }

  const { id }: Props = $props()

  const term = $derived(getTermById(id))

  let popoverOpen = $state(false)

  let btnEl = $state<HTMLButtonElement | undefined>(undefined)
  let popoverEl = $state<HTMLDivElement | undefined>(undefined)

  const termLabel = $derived.by(() => {
    if (!term) return ''
    return term.full ? `${term.term} — ${term.full}` : term.term
  })

  function toggle() {
    popoverOpen = !popoverOpen
  }

  function close() {
    popoverOpen = false
  }

  function handleWindowClick(event: MouseEvent) {
    if (!popoverOpen) return
    const target = event.target
    if (
      target instanceof Node &&
      popoverEl &&
      !popoverEl.contains(target) &&
      btnEl &&
      !btnEl.contains(target)
    ) {
      popoverOpen = false
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && popoverOpen) {
      popoverOpen = false
    }
  }
</script>

<svelte:window onclick={handleWindowClick} onkeydown={handleKeydown} />

{#if term}
  <span class="gt__root">
    <button
      bind:this={btnEl}
      type="button"
      class="gt__help-btn"
      class:gt__help-btn--active={popoverOpen}
      onclick={toggle}
      aria-haspopup="dialog"
      aria-expanded={popoverOpen}
      aria-label="Ayuda: {term.term}"
    >?</button>

    {#if popoverOpen}
      <div
        bind:this={popoverEl}
        class="gt__popover"
        role="dialog"
        aria-modal="true"
        aria-label={termLabel}
      >
        <div class="gt__popover-head">
          <p class="gt__popover-term">{termLabel}</p>
          <button
            type="button"
            class="gt__close"
            onclick={close}
            aria-label="Cerrar"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M18 6 6 18" />
              <path d="m6 6 12 12" />
            </svg>
          </button>
        </div>
        <p class="gt__popover-def">{term.definition}</p>
        {#if term.sources && term.sources.length > 0}
          <ul class="gt__sources">
            {#each term.sources as src}
              <li>
                <a href={src} target="_blank" rel="noopener noreferrer" class="gt__source-link">
                  {src}
                </a>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </span>
{/if}

<style>
  .gt__root {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .gt__help-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: none;
    color: var(--color-text-muted);
    font-family: var(--font-sans);
    font-size: var(--font-size-2xs, 11px);
    font-weight: var(--font-weight-semibold);
    line-height: 1;
    cursor: pointer;
    transition:
      color var(--transition-base),
      border-color var(--transition-base),
      background-color var(--transition-base);
  }

  .gt__help-btn:hover,
  .gt__help-btn--active {
    color: var(--color-accent);
    border-color: var(--color-accent);
    background: var(--color-accent-faint);
  }

  .gt__help-btn:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  /* ── Popover ── */

  .gt__popover {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    z-index: 60;
    width: 320px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-surface);
    background: var(--color-surface-raised);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
  }

  .gt__popover-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-3) var(--space-2);
    border-bottom: 1px solid var(--color-border-subtle);
  }

  .gt__popover-term {
    margin: 0;
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text-primary);
    line-height: var(--line-height-tight);
  }

  .gt__close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--color-text-muted);
    cursor: pointer;
    transition: color var(--transition-base), background-color var(--transition-base);
  }

  .gt__close:hover {
    color: var(--color-text-primary);
    background: var(--color-accent-faint);
  }

  .gt__close:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .gt__popover-def {
    margin: 0;
    padding: var(--space-3);
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    line-height: var(--line-height-base);
  }

  .gt__sources {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: var(--space-2) var(--space-3) var(--space-3);
    border-top: 1px solid var(--color-border-subtle);
    list-style: none;
  }

  .gt__source-link {
    font-size: var(--font-size-xs);
    color: var(--color-accent);
    text-decoration: none;
    word-break: break-all;
  }

  .gt__source-link:hover {
    text-decoration: underline;
    color: var(--color-accent-hover);
  }
</style>
