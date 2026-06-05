<script lang="ts">
  import ActionIcon from '../Button/ActionIcon.svelte'
  import type { ActionIconName } from '../Button/ActionIcon.types'
  import type { AnnotationTool, EditTool } from '../DocumentViewer/DocumentViewer.types'

  export interface AnnotationColorOption {
    value: string
    label: string
  }

  interface AnnotationToolbarProps {
    tool: AnnotationTool
    editTool: EditTool
    color: string
    hasSelection: boolean
    canUndo?: boolean
    panActive?: boolean
    colors: AnnotationColorOption[]
    onToolChange?: (tool: AnnotationTool) => void
    onEditToolChange?: (tool: EditTool) => void
    onPanToggle?: () => void
    onColorChange?: (color: string) => void
    onDeleteSelected?: () => void
    onRotateLeft?: () => void
    onRotateRight?: () => void
    onUndo?: () => void
    zoomPercent?: number | null
    canZoomOut?: boolean
    canZoomIn?: boolean
    onZoomOut?: () => void
    onZoomIn?: () => void
    labels?: Partial<AnnotationToolbarLabels>
  }

  interface AnnotationToolbarLabels {
    expandToolbar: string
    expandToolbarTitle: string
    collapseToolbar: string
    collapseToolbarTitle: string
    toolbarAriaLabel: string
    undo: string
    undoTitle: string
    panTool: string
    rectangleTool: string
    underlineTool: string
    cropTool: string
    eraseTool: string
    rotateLeft: string
    rotateRight: string
    zoomOut: string
    zoomIn: string
    deleteSelected: string
    colorAriaLabel: (label: string) => string
  }

  const defaultLabels: AnnotationToolbarLabels = {
    expandToolbar: 'Expand annotation toolbar',
    expandToolbarTitle: 'Expand toolbar',
    collapseToolbar: 'Collapse annotation toolbar',
    collapseToolbarTitle: 'Collapse toolbar',
    toolbarAriaLabel: 'Image editing tools',
    undo: 'Undo last edit',
    undoTitle: 'Undo',
    panTool: 'Pan image (hand tool)',
    rectangleTool: 'Rectangle annotation tool',
    underlineTool: 'Underline annotation tool',
    cropTool: 'Crop to selection',
    eraseTool: 'Erase region (white fill)',
    rotateLeft: 'Rotate 90° left',
    rotateRight: 'Rotate 90° right',
    zoomOut: 'Zoom out',
    zoomIn: 'Zoom in',
    deleteSelected: 'Delete selected annotation',
    colorAriaLabel: (label: string) => `${label} annotation color`,
  }

  let {
    tool,
    editTool,
    color,
    hasSelection,
    canUndo = false,
    panActive = false,
    colors,
    onToolChange = () => {},
    onEditToolChange = () => {},
    onPanToggle = () => {},
    onColorChange = () => {},
    onDeleteSelected = () => {},
    onRotateLeft = () => {},
    onRotateRight = () => {},
    onUndo = () => {},
    zoomPercent = null,
    canZoomOut = false,
    canZoomIn = false,
    onZoomOut = () => {},
    onZoomIn = () => {},
    labels: labelsProp = {},
  }: AnnotationToolbarProps = $props()

  const labels = $derived({ ...defaultLabels, ...labelsProp })

  let collapsed = $state(false)

  const toolOptions = $derived.by(
    (): Array<{
      value: Exclude<AnnotationTool, 'select'>
      label: string
      icon: ActionIconName
    }> => [
      { value: 'rectangle', label: labels.rectangleTool, icon: 'rectangle' },
      { value: 'underline', label: labels.underlineTool, icon: 'underline' },
    ]
  )

  const editToolOptions = $derived.by(
    (): Array<{
      value: Exclude<EditTool, 'none'>
      label: string
      icon: ActionIconName
    }> => [
      { value: 'crop', label: labels.cropTool, icon: 'crop' },
      { value: 'erase', label: labels.eraseTool, icon: 'eraser' },
    ]
  )

  function handleToolClick(option: (typeof toolOptions)[number]) {
    if (tool === option.value) {
      onToolChange('select')
    } else {
      onToolChange(option.value)
    }
  }

  function handleEditToolClick(option: (typeof editToolOptions)[number]) {
    if (editTool === option.value) {
      onEditToolChange('none')
    } else {
      onEditToolChange(option.value)
    }
  }

</script>

{#if collapsed}
  <button
    type="button"
    class="annotation-toolbar__fab"
    data-testid="annotation-toolbar-fab"
    aria-label={labels.expandToolbar}
    title={labels.expandToolbarTitle}
    onclick={() => (collapsed = false)}
  >
    <ActionIcon name="pencil" size={18} />
  </button>
{:else}
  <div
    class="annotation-toolbar"
    data-testid="annotation-toolbar"
    role="toolbar"
    aria-orientation="vertical"
    aria-label={labels.toolbarAriaLabel}
  >
      <button
        type="button"
        class="annotation-toolbar__button"
        aria-label={labels.undo}
        title={labels.undoTitle}
        disabled={!canUndo}
        onclick={onUndo}
      >
        <ActionIcon name="undo" size={18} />
      </button>
      <button
        type="button"
        class="annotation-toolbar__button"
        class:annotation-toolbar__button--active={panActive}
        aria-label={labels.panTool}
        aria-pressed={panActive}
        title={labels.panTool}
        onclick={onPanToggle}
      >
        <ActionIcon name="hand" size={18} />
      </button>
      {#each toolOptions as option (option.value)}
        <button
          type="button"
          class="annotation-toolbar__button"
          class:annotation-toolbar__button--active={tool === option.value}
          aria-label={option.label}
          aria-pressed={tool === option.value}
          title={option.label}
          onclick={() => handleToolClick(option)}
        >
          <ActionIcon name={option.icon} size={18} />
        </button>
      {/each}
      {#each editToolOptions as option (option.value)}
        <button
          type="button"
          class="annotation-toolbar__button"
          class:annotation-toolbar__button--active={editTool === option.value}
          aria-label={option.label}
          aria-pressed={editTool === option.value}
          title={option.label}
          onclick={() => handleEditToolClick(option)}
        >
          <ActionIcon name={option.icon} size={18} />
        </button>
      {/each}
      <button
        type="button"
        class="annotation-toolbar__button"
        aria-label={labels.rotateLeft}
        title={labels.rotateLeft}
        onclick={onRotateLeft}
      >
        <ActionIcon name="rotate-ccw" size={18} />
      </button>

      <button
        type="button"
        class="annotation-toolbar__button"
        aria-label={labels.rotateRight}
        title={labels.rotateRight}
        onclick={onRotateRight}
      >
        <ActionIcon name="rotate-cw" size={18} />
      </button>

      {#if zoomPercent !== null}
        <button
          type="button"
          class="annotation-toolbar__button"
          aria-label={labels.zoomOut}
          title={labels.zoomOut}
          disabled={!canZoomOut}
          onclick={onZoomOut}
        >
          <svg
            class="annotation-toolbar__icon"
            viewBox="0 0 24 24"
            aria-hidden="true"
            focusable="false"
          >
            <circle cx="11" cy="11" r="6.5" fill="none" stroke="currentColor" stroke-width="1.8" />
            <path
              d="M16 16 21 21"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
            />
            <path
              d="M8.5 11h5"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
            />
          </svg>
        </button>

        <span class="annotation-toolbar__zoom" data-testid="toolbar-zoom-info">{zoomPercent}%</span>

        <button
          type="button"
          class="annotation-toolbar__button"
          aria-label={labels.zoomIn}
          title={labels.zoomIn}
          disabled={!canZoomIn}
          onclick={onZoomIn}
        >
          <svg
            class="annotation-toolbar__icon"
            viewBox="0 0 24 24"
            aria-hidden="true"
            focusable="false"
          >
            <circle cx="11" cy="11" r="6.5" fill="none" stroke="currentColor" stroke-width="1.8" />
            <path
              d="M16 16 21 21"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
            />
            <path
              d="M8.5 11h5"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
            />
            <path
              d="M11 8.5v5"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
            />
          </svg>
        </button>
      {/if}

      {#each colors as option (option.value)}
        <button
          type="button"
          class="annotation-toolbar__swatch"
          class:annotation-toolbar__swatch--active={color === option.value}
          aria-label={labels.colorAriaLabel(option.label)}
          aria-pressed={color === option.value}
          title={option.label}
          onclick={() => onColorChange(option.value)}
        >
          <span class="annotation-toolbar__swatch-fill" style={`background:${option.value}`}></span>
        </button>
      {/each}

    <button
      type="button"
      class="annotation-toolbar__button annotation-toolbar__button--danger"
      aria-label={labels.deleteSelected}
      title={labels.deleteSelected}
      disabled={!hasSelection}
      onclick={onDeleteSelected}
    >
      <ActionIcon name="delete" size={18} />
    </button>

    <button
      type="button"
      class="annotation-toolbar__button annotation-toolbar__button--collapse"
      aria-label={labels.collapseToolbar}
      title={labels.collapseToolbarTitle}
      onclick={() => (collapsed = true)}
    >
      <ActionIcon name="chevron-right" size={18} />
    </button>
  </div>
{/if}

<style>
  .annotation-toolbar,
  .annotation-toolbar__fab {
    --annotation-toolbar-scale: 1;
    --annotation-toolbar-control-size: calc(30px * var(--annotation-toolbar-scale));
    --annotation-toolbar-icon-size: calc(17px * var(--annotation-toolbar-scale));
    --annotation-toolbar-padding: calc(6px * var(--annotation-toolbar-scale));
    --annotation-toolbar-gap: calc(4px * var(--annotation-toolbar-scale));
    --annotation-toolbar-swatch-size: calc(13px * var(--annotation-toolbar-scale));
    --annotation-toolbar-radius: calc(8px * var(--annotation-toolbar-scale));
  }

  .annotation-toolbar {
    display: flex;
    flex-direction: column;
    align-items: center;
    flex-wrap: nowrap;
    gap: var(--annotation-toolbar-gap);
    width: max-content;
    max-width: max-content;
    overflow: visible;
    box-sizing: border-box;
    padding: var(--annotation-toolbar-padding);
    border: 1px solid var(--color-border);
    border-radius: var(--annotation-toolbar-radius);
    background: color-mix(in srgb, var(--color-surface-raised) 92%, transparent);
    box-shadow: var(--shadow-md);
    backdrop-filter: blur(10px);
    pointer-events: auto;
  }

  @media (max-height: 760px) {
    .annotation-toolbar,
    .annotation-toolbar__fab {
      --annotation-toolbar-scale: 0.9;
    }
  }

  @media (max-height: 680px) {
    .annotation-toolbar,
    .annotation-toolbar__fab {
      --annotation-toolbar-scale: 0.82;
    }
  }

  @media (max-height: 600px), (max-width: 520px) {
    .annotation-toolbar,
    .annotation-toolbar__fab {
      --annotation-toolbar-scale: 0.78;
    }

    .annotation-toolbar {
      display: grid;
      grid-auto-flow: column;
      grid-template-rows: repeat(9, max-content);
      align-items: center;
      justify-items: center;
      column-gap: calc(var(--annotation-toolbar-gap) * 1.25);
      row-gap: var(--annotation-toolbar-gap);
    }
  }

  @media (max-height: 480px) {
    .annotation-toolbar,
    .annotation-toolbar__fab {
      --annotation-toolbar-scale: 0.68;
    }
  }

  @media (max-height: 420px) {
    .annotation-toolbar,
    .annotation-toolbar__fab {
      --annotation-toolbar-scale: 0.6;
    }
  }

  .annotation-toolbar__fab {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--annotation-toolbar-control-size);
    height: var(--annotation-toolbar-control-size);
    padding: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--annotation-toolbar-radius);
    background: color-mix(in srgb, var(--color-surface-raised) 90%, transparent);
    box-shadow: var(--shadow-sm);
    backdrop-filter: blur(10px);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--font-size-md);
    line-height: 1;
    pointer-events: auto;
    transition:
      background-color 0.15s ease,
      color 0.15s ease;
  }

  .annotation-toolbar__fab:hover {
    background: var(--color-surface-raised);
    color: var(--color-text-primary);
  }

  .annotation-toolbar__zoom {
    min-width: calc(var(--annotation-toolbar-control-size) + 2px);
    max-width: calc(var(--annotation-toolbar-control-size) + 8px);
    padding-inline: 1px;
    text-align: center;
    font-family: var(--font-mono);
    font-size: calc(0.78rem * var(--annotation-toolbar-scale));
    color: var(--color-text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .annotation-toolbar__button,
  .annotation-toolbar__swatch {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--annotation-toolbar-control-size);
    height: var(--annotation-toolbar-control-size);
    flex: 0 0 var(--annotation-toolbar-control-size);
    padding: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--annotation-toolbar-radius);
    background: var(--color-surface);
    color: var(--color-text-primary);
    cursor: pointer;
    transition:
      background-color 0.15s ease,
      border-color 0.15s ease,
      transform 0.15s ease;
  }

  .annotation-toolbar__button:hover:not(:disabled),
  .annotation-toolbar__swatch:hover:not(:disabled) {
    background: var(--color-surface-raised);
    border-color: var(--color-text-secondary);
  }

  .annotation-toolbar__icon {
    width: var(--annotation-toolbar-icon-size);
    height: var(--annotation-toolbar-icon-size);
    display: block;
  }

  .annotation-toolbar__button :global(svg),
  .annotation-toolbar__fab :global(svg) {
    width: var(--annotation-toolbar-icon-size);
    height: var(--annotation-toolbar-icon-size);
  }

  .annotation-toolbar__button:disabled,
  .annotation-toolbar__swatch:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .annotation-toolbar__button--active,
  .annotation-toolbar__swatch--active {
    border-color: var(--color-accent);
    box-shadow: inset 0 0 0 1px var(--color-accent);
  }

  .annotation-toolbar__button--danger:disabled {
    border-color: var(--color-border);
  }

  .annotation-toolbar__button--danger:not(:disabled) {
    color: var(--color-danger);
  }

  .annotation-toolbar__button--collapse {
    color: var(--color-text-secondary);
    font-size: var(--font-size-lg);
  }

  .annotation-toolbar__button--collapse:hover {
    color: var(--color-text-primary);
  }

  .annotation-toolbar__swatch-fill {
    width: var(--annotation-toolbar-swatch-size);
    height: var(--annotation-toolbar-swatch-size);
    border-radius: var(--radius-full);
    border: 1px solid color-mix(in srgb, var(--color-text-primary) 35%, transparent);
  }

  @media (max-width: 420px), (max-height: 520px) {
    .annotation-toolbar {
      box-shadow: var(--shadow-sm);
    }
  }
</style>
