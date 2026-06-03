<script lang="ts">
  import {
    DocumentViewer,
    TabButton,
    TabList,
    type AnnotationTool,
    type DocumentViewerProps,
    type EditTool,
    type ViewerAnnotation,
    type ViewerLayoutRegion,
    type ViewerType,
  } from '@entropia/ui'
  import type { I18nKey, I18nParams } from '$lib/i18n'
  import type { Asset } from '@entropia/store'
  import type { AssetOcrState } from '$lib/ocr'
  import type { AssetTranscriptionState } from '$lib/transcription'

  let leftPanelTab = $state<'document' | 'text'>('document')
  let currentAssetId = $state<string | null>(null)

  let {
    selectedAsset,
    viewerSrc,
    viewerType,
    annotations,
    layoutRegions,
    showLayoutOverlay,
    hoveredLayoutRegionId,
    selectedLayoutRegionId,
    layoutReferenceWidth,
    layoutReferenceHeight,
    selectedAnnotationId,
    annotationTool,
    annotationColor,
    editTool,
    canUndo,
    viewerPage,
    annotationSaveError,
    ocrState,
    ocrEditedText,
    transcriptionState,
    transcriptionEditedText,
    documentViewerLabels,
    annotationToolbarLabels,
    translate,
    onAnnotationsChange,
    onSelectedAnnotationIdChange,
    onAnnotationToolChange,
    onAnnotationColorChange,
    onLayoutRegionHoverChange,
    onLayoutRegionSelect,
    onEditSelect,
    onEditToolChange,
    onRotateLeft,
    onRotateRight,
    onUndo,
    onPageChange,
    onDimensionsChange,
  }: {
    selectedAsset: Asset | null
    viewerSrc: string
    viewerType: ViewerType
    annotations: ViewerAnnotation[]
    layoutRegions: ViewerLayoutRegion[]
    showLayoutOverlay: boolean
    hoveredLayoutRegionId: string | null
    selectedLayoutRegionId: string | null
    layoutReferenceWidth: number
    layoutReferenceHeight: number
    selectedAnnotationId: string | null
    annotationTool: AnnotationTool
    annotationColor: string
    editTool: EditTool
    canUndo: boolean
    viewerPage: number
    annotationSaveError: string | null
    ocrState: AssetOcrState | null
    ocrEditedText: string
    transcriptionState: AssetTranscriptionState | null
    transcriptionEditedText: string
    documentViewerLabels: DocumentViewerProps['labels']
    annotationToolbarLabels: DocumentViewerProps['annotationToolbarLabels']
    translate: (key: I18nKey, params?: I18nParams) => string
    onAnnotationsChange: (annotations: ViewerAnnotation[]) => void
    onSelectedAnnotationIdChange: (annotationId: string | null) => void
    onAnnotationToolChange: (tool: AnnotationTool) => void
    onAnnotationColorChange: (color: string) => void
    onLayoutRegionHoverChange: (regionId: string | null) => void
    onLayoutRegionSelect: (regionId: string) => void
    onEditSelect: (region: { x: number; y: number; width: number; height: number }) => void | Promise<void>
    onEditToolChange: (tool: EditTool) => void
    onRotateLeft: () => void | Promise<void>
    onRotateRight: () => void | Promise<void>
    onUndo: () => void | Promise<void>
    onPageChange: (page: number, totalPages: number) => void
    onDimensionsChange: (dimensions: { width: number; height: number }) => void
  } = $props()

  $effect(() => {
    const nextAssetId = selectedAsset?.id ?? null

    if (nextAssetId !== currentAssetId) {
      currentAssetId = nextAssetId
      leftPanelTab = 'document'
    }
  })
</script>

{#if selectedAsset}
  <TabList class="left-panel-tabs" aria-label={translate('item.assetPanel')}>
    <TabButton
      id="left-panel-tab-document"
      active={leftPanelTab === 'document'}
      class="left-panel-tab"
      aria-controls="left-panel-document"
      onclick={() => {
        leftPanelTab = 'document'
      }}
    >
      {translate('item.documentTab')}
    </TabButton>
    <TabButton
      id="left-panel-tab-text"
      active={leftPanelTab === 'text'}
      class="left-panel-tab"
      aria-controls="left-panel-text"
      onclick={() => {
        leftPanelTab = 'text'
      }}
    >
      {translate('item.extractedTextTab')}
    </TabButton>
  </TabList>

  <div class="left-panel-content">
    <div
      id="left-panel-document"
      role="tabpanel"
      aria-labelledby="left-panel-tab-document"
      class="left-panel-pane left-panel-pane--document"
      class:is-hidden={leftPanelTab !== 'document'}
    >
      <DocumentViewer
        path={selectedAsset.path}
        assetUrl={viewerSrc}
        type={viewerType}
        {annotations}
        {layoutRegions}
        {showLayoutOverlay}
        {hoveredLayoutRegionId}
        {selectedLayoutRegionId}
        {layoutReferenceWidth}
        {layoutReferenceHeight}
        {selectedAnnotationId}
        {annotationTool}
        {annotationColor}
        {editTool}
        {canUndo}
        currentPage={viewerPage}
        {onAnnotationsChange}
        {onSelectedAnnotationIdChange}
        {onAnnotationToolChange}
        {onAnnotationColorChange}
        {onLayoutRegionHoverChange}
        {onLayoutRegionSelect}
        {onEditSelect}
        {onEditToolChange}
        {onRotateLeft}
        {onRotateRight}
        {onUndo}
        {onPageChange}
        {onDimensionsChange}
        labels={documentViewerLabels}
        {annotationToolbarLabels}
      />

      {#if annotationSaveError}
        <p class="error">{annotationSaveError}</p>
      {/if}
    </div>

    <div
      id="left-panel-text"
      role="tabpanel"
      aria-labelledby="left-panel-tab-text"
      class="left-panel-pane left-panel-pane--text"
      class:is-hidden={leftPanelTab !== 'text'}
    >
      {#if selectedAsset.type !== 'audio'}
        <section class="left-text-panel-section">
          <div class="left-text-panel-card">
            {#if ocrEditedText.trim()}
              <div class="left-text-panel-meta">
                <span>{translate('item.extractedText')}</span>
                <span class="ocr-meta"
                  >via {ocrState?.method ?? translate('item.ocrMethodUnknown')} · {translate(
                    'item.characters',
                    { count: ocrEditedText.length }
                  )}</span
                >
              </div>
              <div class="left-text-panel-body">
                {ocrEditedText}
              </div>
            {:else}
              <p class="empty-text">{translate('item.noExtractedText')}</p>
            {/if}
          </div>
        </section>
      {:else}
        <section class="left-text-panel-section">
          <div class="left-text-panel-card">
            {#if transcriptionEditedText.trim()}
              <div class="left-text-panel-meta">
                <span>{translate('item.transcription')}</span>
                <span class="ocr-meta">
                  {#if transcriptionState?.language}{transcriptionState.language} &middot;
                  {/if}{translate('item.characters', { count: transcriptionEditedText.length })}
                  {#if transcriptionState?.durationMs}
                    &middot; {translate('item.audioDurationSeconds', {
                      count: Math.round(transcriptionState.durationMs / 1000),
                    })}{/if}
                </span>
              </div>
              <div class="left-text-panel-body">
                {transcriptionEditedText}
              </div>
            {:else}
              <p class="empty-text">{translate('item.noExtractedText')}</p>
            {/if}
          </div>
        </section>
      {/if}
    </div>
  </div>
{:else}
  <div class="empty-viewer">
    <p>{translate('item.noAssets')}</p>
  </div>
{/if}

<style>
  :global(.left-panel-tabs) {
    display: flex;
    width: 100%;
    flex-shrink: 0;
    border-color: var(--border-subtle);
    background: var(--surface-input);
  }

  :global(.left-panel-tab) {
    flex: 1;
    min-width: 0;
  }

  .left-panel-content {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }

  .left-panel-pane {
    min-height: 0;
  }

  .left-panel-pane.is-hidden {
    display: none;
  }

  .left-panel-pane--text {
    flex: 1;
    padding: 0 var(--space-2);
    min-height: 0;
  }

  .left-text-panel-section {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }

  .left-text-panel-card {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-surface);
    background: var(--surface-panel);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025);
  }

  .left-text-panel-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
  }

  .left-text-panel-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--surface-input);
    color: var(--color-text-primary);
    font-size: var(--font-size-sm);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .empty-viewer {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 300px;
    color: var(--color-text-secondary);
    border: 1px dashed var(--color-hairline);
    border-radius: var(--radius-md);
  }

  .empty-text {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }

  .error {
    color: var(--color-danger);
  }

  .ocr-meta {
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
  }
</style>
