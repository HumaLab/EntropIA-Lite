<script lang="ts">
  import type { Snippet } from 'svelte'
  import { getTermById, type GlossaryTerm } from '$lib/glossary'

  type Props = {
    id: string
    children: Snippet
  }

  const { id, children }: Props = $props()

  const term = $derived(getTermById(id))

  let tooltipVisible = $state(false)
  let popoverOpen = $state(false)

  let anchorEl = $state<HTMLSpanElement | undefined>(undefined)
  let popoverEl = $state<HTMLDivElement | undefined>(undefined)

  const termLabel = $derived.by(() => {
    if (!term) return ''
    return term.full ? `${term.term} — ${term.full}` : term.term
  })

  function openPopover() {
    tooltipVisible = false
    popoverOpen = true
  }

  function closePopover() {
    popoverOpen = false
  }

  function handleWindowClick(event: MouseEvent) {
    if (!popoverOpen) return
    const target = event.target
    if (
      target instanceof Node &&
      popoverEl &&
      !popoverEl.contains(target) &&
      anchorEl &&
      !anchorEl.contains(target)
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

{#if !term}
  {@render children()}
{:else}
  <span class="gt__root">
    <span
      bind:this={anchorEl}
      class="gt__anchor"
      role="button"
      tabindex="0"
      aria-haspopup="dialog"
      aria-expanded={popoverOpen}
      onmouseenter={() => { if (!popoverOpen) tooltipVisible = true }}
      onmouseleave={() => { tooltipVisible = false }}
      onclick={() => { if (!popoverOpen) { tooltipVisible = !tooltipVisible } }}
      onfocus={() => { if (!popoverOpen) tooltipVisible = true }}
      onblur={() => { tooltipVisible = false }}
    >
      {@render children()}
    </span>

    {#if tooltipVisible && !popoverOpen}
      <div class="gt__tooltip" role="tooltip">
        <p class="gt__tooltip-term">{termLabel}</p>
        <p class="gt__tooltip-short">{term.short}</p>
        <button type="button" class="gt__read-more" onclick={openPopover}>
          Leer más
        </button>
      </div>
    {/if}

    {#if popoverOpen}
      <div bind:this={popoverEl} class="gt__popover" role="dialog" aria-modal="true" aria-label={termLabel}>
        <div class="gt__popover-head">
          <p class="gt__popover-term">{termLabel}</p>
          <button
            type="button"
            class="gt__close"
            onclick={closePopover}
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
    display: inline;
  }

  .gt__anchor {
    display: inline;
    border-bottom: 1px dotted var(--color-accent);
    color: inherit;
    cursor: help;
    text-decoration: none;
  }

  .gt__anchor:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
    border-radius: 2px;
  }

  /* ── Tooltip ── */

  .gt__tooltip {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    z-index: 50;
    width: 260px;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-elevated);
    box-shadow: var(--shadow-md);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .gt__tooltip-term {
    margin: 0;
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text-primary);
    line-height: var(--line-height-tight);
  }

  .gt__tooltip-short {
    margin: 0;
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    line-height: var(--line-height-base);
  }

  .gt__read-more {
    align-self: flex-start;
    padding: 2px var(--space-2);
    border: none;
    border-radius: var(--radius-sm);
    background: var(--color-accent-soft);
    color: var(--color-accent);
    font-family: var(--font-sans);
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-medium);
    cursor: pointer;
    transition:
      background-color var(--transition-base),
      color var(--transition-base);
  }

  .gt__read-more:hover {
    background: var(--color-accent);
    color: var(--color-bg);
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
