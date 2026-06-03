<script lang="ts">
  import { getStore } from '$lib/db'
  import { getAssetUrl } from '$lib/file-import'
  import {
    buildTechnicalMetadata,
    getAssetPathLabel,
    getAssetTypeLabel,
    mergeReservedMetadata,
    normalizeMetadataKey,
    parseImportedFileMetadata,
    parseMetadataRecord,
    type ImportedFileMetadata,
  } from '$lib/item-metadata'
  import {
    cropAnnotations,
    normalizeAnnotationsForAsset,
    normalizedToPixels,
    rotateAnnotations,
    type NormalizedRegion,
    type RotationDirection,
  } from '$lib/item-view-geometry'
  import ItemSearchPanel from './ItemSearchPanel.svelte'
  import ItemMetadataPanel from './ItemMetadataPanel.svelte'
  import ItemNotesPanel from './ItemNotesPanel.svelte'
  import ItemLayoutPanel from './ItemLayoutPanel.svelte'
  import {
    buildLayoutBlockViews,
    countLayoutBlocksByFilter,
    filterBlocksByPage,
    filterRegionsByPage,
    filterLayoutBlocksByType,
    getBlockCountByPage,
    getLayoutByAsset,
    getPagesFromLayout,
    LAYOUT_BLOCK_FILTERS,
    type LayoutBlockFilterId,
  } from '$lib/layouts'
  import { OcrStore, extractText, type OcrMode } from '$lib/ocr'
  import { TranscriptionStore, transcribeAudio, transcribeDictation } from '$lib/transcription'
  import {
    NlpStore,
    indexFts,
    embedAsset,
    extractEntities,
    extractEntitiesForAsset,
    similarAssets as fetchSimilarAssets,
    type SimilarAsset,
  } from '$lib/nlp'
  import {
    LlmStore,
    llmSummarize,
    llmCorrectOcr,
    llmExtractTriples,
    llmSummarizeAsset,
    llmCorrectOcrAsset,
    llmExtractTriplesAsset,
    llmIsAvailable,
    llmGetResult,
  } from '$lib/llm'
  import { GeoStore } from '$lib/geo'
  import {
    DocumentViewer,
    ActionIcon,
    EntityViewer,
    IconButton,
    MapViewer,
    Panel,
    StatusBadge,
    TabButton,
    TabList,
    isNoteHtmlEffectivelyEmpty,
  } from '@entropia/ui'
  import type { MapMarker, StatusBadgeVariant } from '@entropia/ui'
  import { onMount, onDestroy } from 'svelte'
  import { listen, emit } from '@tauri-apps/api/event'
  import { invoke } from '@tauri-apps/api/core'
  import { navigation } from '$lib/navigation'
  import {
    DOCUMENT_EXPLORER_ASSET_SELECTED_EVENT,
    DOCUMENT_EXPLORER_ASSET_SELECT_REQUEST_EVENT,
    type DocumentExplorerAssetDetail,
  } from '$lib/document-explorer'
  import { locale, t, type I18nKey, type I18nParams } from '$lib/i18n'
  import type {
    Item,
    Asset,
    Collection,
    Note,
    Annotation as StoreAnnotation,
    AnnotationKind as StoreAnnotationKind,
  } from '@entropia/store'
  import type {
    Entity,
    ViewerAnnotation,
    ViewerLayoutRegion,
    AnnotationKind as ViewerAnnotationKind,
    EditTool,
    ImageEditResult,
  } from '@entropia/ui'
  import { TranscriptionRepo } from '@entropia/store'

  const isDev = import.meta.env.DEV

  // ── Sidebar resize ──
  const MIN_SIDEBAR_PCT = 20
  const MAX_SIDEBAR_PCT = 50
  const DEFAULT_SIDEBAR_PCT = 33

  let sidebarWidth = $state(
    (() => {
      try {
        const stored = localStorage.getItem('entropia-sidebar-width')
        if (stored !== null) {
          const parsed = Number(stored)
          if (!isNaN(parsed)) {
            return Math.max(MIN_SIDEBAR_PCT, Math.min(MAX_SIDEBAR_PCT, parsed))
          }
        }
      } catch {}
      return DEFAULT_SIDEBAR_PCT
    })()
  )

  let isDragging = $state(false)
  let itemViewEl: HTMLElement | undefined = $state()
  let dragCleanup: (() => void) | null = null

  function handleExplorerAssetSelectRequest(event: Event) {
    const detail = (event as CustomEvent<DocumentExplorerAssetDetail>).detail
    if (detail.itemId !== itemId || !detail.assetId) return

    const nextIndex = assets.findIndex((asset) => asset.id === detail.assetId)
    if (nextIndex >= 0) {
      selectedAssetIndex = nextIndex
    }
  }

  function onResizeHandlePointerDown(e: PointerEvent) {
    e.preventDefault()
    isDragging = true

    const startX = e.clientX
    const startWidthPct = sidebarWidth
    const containerEl = itemViewEl ?? document.querySelector('.item-view') ?? document.body
    const containerWidth = (containerEl as HTMLElement).clientWidth

    let rafId: number | null = null
    let lastClientX = startX

    function onPointerMove(e: PointerEvent) {
      lastClientX = e.clientX
      if (rafId !== null) return
      rafId = requestAnimationFrame(() => {
        const deltaX = lastClientX - startX
        const deltaPct = (deltaX / containerWidth) * 100
        sidebarWidth = Math.max(
          MIN_SIDEBAR_PCT,
          Math.min(MAX_SIDEBAR_PCT, startWidthPct - deltaPct)
        )
        rafId = null
      })
    }

    function onPointerUp() {
      isDragging = false
      try {
        localStorage.setItem('entropia-sidebar-width', String(Math.round(sidebarWidth)))
      } catch {}
      window.removeEventListener('pointermove', onPointerMove)
      window.removeEventListener('pointerup', onPointerUp)
      document.body.classList.remove('no-select')
      dragCleanup = null
    }

    document.body.classList.add('no-select')
    window.addEventListener('pointermove', onPointerMove)
    window.addEventListener('pointerup', onPointerUp)
    dragCleanup = onPointerUp
  }

  let { itemId, collectionId }: { itemId: string; collectionId: string } = $props()

  let item = $state<Item | null>(null)
  let assets = $state<Asset[]>([])
  let collection = $state<Collection | null>(null)
  let notes = $state<Note[]>([])
  let loading = $state(true)
  let error = $state<string | null>(null)
  const currentLocale = locale
  const translate = $derived.by(() => {
    $currentLocale
    return (key: I18nKey, params?: I18nParams) => t(key, params)
  })
  let selectedAssetIndex = $state(0)
  let savingMetadata = $state(false)
  let annotations = $state<ViewerAnnotation[]>([])
  let selectedAnnotationId = $state<string | null>(null)
  let annotationTool = $state<'select' | 'rectangle' | 'underline'>('select')
  let annotationColor = $state('var(--color-accent)')
  let annotationSaveError = $state<string | null>(null)
  let annotationSaveTimer: ReturnType<typeof setTimeout> | null = null
  let pendingAnnotationSave: {
    assetId: string
    annotations: ViewerAnnotation[]
  } | null = null

  let assetLayout = $state<Awaited<ReturnType<typeof getLayoutByAsset>>>(null)
  let layoutLoading = $state(false)
  let layoutError = $state<string | null>(null)
  let showLayout = $state(false)
  let layoutTypeFilter = $state<LayoutBlockFilterId>('all')
  let layoutHoveredBlockId = $state<string | null>(null)
  let layoutSelectedBlockId = $state<string | null>(null)
  let layoutHoveredRegionId = $state<string | null>(null)
  let layoutSelectedRegionId = $state<string | null>(null)
  let layoutLoadToken = 0
  let notesLoadToken = 0
  let selectedAssetStateLoadToken = 0
  let entitiesLoadToken = 0
  let geoMarkersLoadToken = 0
  let triplesLoadToken = 0
  let similarAssetsLoadToken = 0
  let llmSummaryLoadToken = 0
  let viewerPage = $state(1)
  let viewerTotalPages = $state(1)
  let leftPanelTab = $state<'document' | 'text'>('document')

  // Image edit state
  let editTool = $state<EditTool>('none')
  let imageVersion = $state(0)

  // Undo history: stack of { path, width, height, annotations } snapshots
  // Each entry represents the state BEFORE an edit operation. Popping restores
  // the asset to that state (the file is still on disk because edits create
  // versioned files rather than overwriting in-place).
  interface UndoEntry {
    path: string
    width: number
    height: number
    annotations: ViewerAnnotation[]
  }
  let undoStack = $state<UndoEntry[]>([])
  let canUndo = $derived(undoStack.length > 0)
  let lastSelectedAssetId = $state<string | null>(null)

  // OCR state — plain TS class, updated via Tauri events
  const ocrStore = new OcrStore({
    onComplete: (assetId) => {
      // After OCR extraction completes on a specific asset, auto-trigger
      // asset-level refreshes and entity extraction.
      if (selectedAsset && selectedAsset.id === assetId) {
        void reloadSelectedAssetPersistedState({ layout: true })
        void extractEntitiesForAsset(itemId, assetId).catch(() => {})
      }
    },
  })
  // Reactive tick counter: incremented on every OCR event to force Svelte re-evaluation
  let ocrTick = $state(0)
  // Edited text per asset — tracks user corrections to OCR output
  let ocrEditedText = $state(new Map<string, string>())
  // Debounce timers per asset for persisting edits to DB
  let ocrPersistTimers = $state(new Map<string, ReturnType<typeof setTimeout>>())
  // Debounce timers per asset for downstream NLP reprocessing after user inactivity
  let assetReanalysisTimers = $state(new Map<string, ReturnType<typeof setTimeout>>())

  // Transcription state — mirrors OcrStore pattern for audio assets
  const transcriptionStore = new TranscriptionStore({
    onComplete: (assetId) => {
      // After transcription completes, auto-trigger entity extraction only.
      if (selectedAsset && selectedAsset.id === assetId) {
        void extractEntitiesForAsset(itemId, assetId).catch(() => {})
      }
    },
  })
  let transcriptionTick = $state(0)

  let transEditedText = $state(new Map<string, string>())
  let transPersistTimers = $state(new Map<string, ReturnType<typeof setTimeout>>())

  const PERSIST_IDLE_MS = 500
  const REANALYSIS_IDLE_MS = 1500

  function scheduleAssetReanalysis(assetId: string) {
    const existing = assetReanalysisTimers.get(assetId)
    if (existing) clearTimeout(existing)

    const timer = setTimeout(async () => {
      const jobs: Array<[string, () => Promise<unknown>]> = [
        ['ner', () => extractEntitiesForAsset(itemId, assetId)],
        ['fts', () => indexFts(itemId)],
        ['embed', () => embedAsset(itemId, assetId)],
      ]

      try {
        console.info('[ItemView] Re-running post-edit analysis', { itemId, assetId })

        const results = await Promise.allSettled(jobs.map(([, run]) => run()))
        results.forEach((result, index) => {
          const jobName = jobs[index]?.[0] ?? 'unknown'
          if (result.status === 'rejected') {
            console.error(`[ItemView] Post-edit ${jobName} failed`, result.reason)
          }
        })
      } finally {
        assetReanalysisTimers.delete(assetId)
      }
    }, REANALYSIS_IDLE_MS)

    assetReanalysisTimers.set(assetId, timer)
  }

  /** Save quickly, but only re-run expensive analysis after longer inactivity. */
  function schedulePersist(assetId: string, text: string) {
    // Cancel any pending timer for this asset
    const existing = ocrPersistTimers.get(assetId)
    if (existing) clearTimeout(existing)

    // Schedule new persist
    const timer = setTimeout(async () => {
      try {
        await invoke('update_extraction_text_cmd', { assetId, textContent: text })
        scheduleAssetReanalysis(assetId)
      } catch (e) {
        console.error('[ItemView] Failed to persist OCR correction:', e)
      }
      ocrPersistTimers.delete(assetId)
    }, PERSIST_IDLE_MS)

    ocrPersistTimers.set(assetId, timer)
  }

  /** Schedule a debounced persist of edited transcription text to the DB. */
  function scheduleTranscriptionPersist(assetId: string, text: string) {
    const existing = transPersistTimers.get(assetId)
    if (existing) clearTimeout(existing)

    const timer = setTimeout(async () => {
      try {
        await invoke('update_transcription_text_cmd', { assetId, textContent: text })
        scheduleAssetReanalysis(assetId)
      } catch (e) {
        console.error('[ItemView] Failed to persist transcription correction:', e)
      }
      transPersistTimers.delete(assetId)
    }, PERSIST_IDLE_MS)

    transPersistTimers.set(assetId, timer)
  }

  // NLP state — mirrors OcrStore pattern
  const nlpStore = new NlpStore()
  let nlpTick = $state(0)
  let entities = $state<Entity[]>([])
  type EditableEntityType = 'person' | 'organization' | 'place' | 'misc' | 'date'
  const EDITABLE_ENTITY_TYPES: EditableEntityType[] = [
    'person',
    'organization',
    'place',
    'misc',
    'date',
  ]
  let newEntityValue = $state('')
  let newEntityType = $state<EditableEntityType>('organization')
  let editingEntityId = $state<string | null>(null)
  let editingEntityValue = $state('')
  let entityActionError = $state<string | null>(null)
  let similarAssets = $state<SimilarAsset[]>([])
  let ftsQuery = $state('')
  let ftsResults = $state<
    Array<{ itemId: string; title: string; rank: number; collectionId: string }>
  >([])
  let ftsSearching = $state(false)
  let ftsSearchError = $state<string | null>(null)
  let ftsSearchTimer: ReturnType<typeof setTimeout> | null = null
  let ftsIndexedRows = $state<number | null>(null)
  let ftsDebug = $state<{
    rawQuery: string
    sanitizedQuery: string
    strategy: 'empty' | 'strict' | 'relaxed'
    matchCount: number
    hydratedCount: number
    resultIds: string[]
  } | null>(null)
  let triples = $state<Array<{ subject: string; predicate: string; object: string }>>([])
  let rightPanelTab = $state<'notes' | 'text' | 'analysis' | 'search' | 'layout' | 'metadata'>(
    'notes'
  )
  let rightPanelOpen = $state(true)

  function getJobStatusBadgeVariant(status: string): StatusBadgeVariant {
    if (status === 'done') return 'success'
    if (status === 'running') return 'warning'
    if (status === 'pending') return 'info'
    if (status === 'error') return 'danger'
    return 'neutral'
  }

  const metadataEditorLabels = {
    keyPlaceholder: 'Campo',
    valuePlaceholder: 'Valor',
    removeFieldAria: 'Eliminar campo',
    addField: '+ Agregar campo',
    fieldLabel: 'Campo',
    valueLabel: 'Valor',
    emptyText: 'No hay metadatos cargados para este documento.',
  }

  const documentViewerLabels = $derived.by(() => {
    $currentLocale
    return {
      imageAlt: translate('item.viewerImageAlt'),
      imageOverlayAriaLabel: translate('item.viewerImageOverlay'),
      audioSkipBack: translate('item.audioSkipBack'),
      audioPlay: translate('item.audioPlay'),
      audioPause: translate('item.audioPause'),
      audioSkipForward: translate('item.audioSkipForward'),
      audioSeek: translate('item.audioSeek'),
      audioVolume: translate('item.audioVolume'),
      pdfLoading: translate('item.viewerPdfLoading'),
      pdfLoadError: translate('item.viewerPdfLoadError'),
      pdfRenderError: translate('item.viewerPdfRenderError'),
      pdfPreviousPage: translate('item.previousPage'),
      pdfNextPage: translate('item.nextPage'),
      pdfZoomOut: translate('item.toolbar.zoomOut'),
      pdfZoomIn: translate('item.toolbar.zoomIn'),
      layoutOverlayAriaLabel: translate('item.viewerLayoutOverlay'),
      layoutRegionAriaLabel: (label: string) => translate('item.viewerLayoutRegion', { label }),
      annotationAriaLabel: (id: string) => translate('item.viewerAnnotation', { id }),
      cropRegionAriaLabel: translate('item.viewerCropRegion'),
      eraseRegionAriaLabel: translate('item.viewerEraseRegion'),
    }
  })

  const annotationToolbarLabels = $derived.by(() => {
    $currentLocale
    return {
      expandToolbar: translate('item.toolbar.expand'),
      expandToolbarTitle: translate('item.toolbar.expandTitle'),
      collapseToolbar: translate('item.toolbar.collapse'),
      collapseToolbarTitle: translate('item.toolbar.collapseTitle'),
      toolbarAriaLabel: translate('item.toolbar.imageTools'),
      undo: translate('item.toolbar.undo'),
      undoTitle: translate('item.toolbar.undoTitle'),
      rectangleTool: translate('item.toolbar.rectangle'),
      underlineTool: translate('item.toolbar.underline'),
      cropTool: translate('item.toolbar.crop'),
      eraseTool: translate('item.toolbar.erase'),
      rotateLeft: translate('item.toolbar.rotateLeft'),
      rotateRight: translate('item.toolbar.rotateRight'),
      zoomOut: translate('item.toolbar.zoomOut'),
      zoomIn: translate('item.toolbar.zoomIn'),
      deleteSelected: translate('item.toolbar.deleteAnnotation'),
      colorAriaLabel: (label: string) => translate('item.toolbar.colorAria', { label }),
    }
  })

  const noteEditorLabels = $derived.by(() => {
    $currentLocale
    return {
      toolbarAriaLabel: translate('item.noteEditor.toolbar'),
      textStyleGroup: translate('item.noteEditor.group.textStyle'),
      structureGroup: translate('item.noteEditor.group.structure'),
      insertGroup: translate('item.noteEditor.group.insert'),
      dictationGroup: translate('item.noteEditor.group.dictation'),
      bold: translate('item.noteEditor.bold'),
      italic: translate('item.noteEditor.italic'),
      underline: translate('item.noteEditor.underline'),
      inlineCode: translate('item.noteEditor.inlineCode'),
      heading1: translate('item.noteEditor.heading1'),
      heading2: translate('item.noteEditor.heading2'),
      heading3: translate('item.noteEditor.heading3'),
      bulletList: translate('item.noteEditor.bulletList'),
      bulletListShort: translate('item.noteEditor.bulletListShort'),
      orderedList: translate('item.noteEditor.orderedList'),
      orderedListShort: translate('item.noteEditor.orderedListShort'),
      quote: translate('item.noteEditor.quote'),
      quoteShort: translate('item.noteEditor.quoteShort'),
      addLink: translate('item.noteEditor.addLink'),
      addLinkShort: translate('item.noteEditor.addLinkShort'),
      removeLink: translate('item.noteEditor.removeLink'),
      removeLinkShort: translate('item.noteEditor.removeLinkShort'),
      dictationStart: translate('item.noteEditor.dictationStart'),
      dictationStop: translate('item.noteEditor.dictationStop'),
      dictationProcessing: translate('item.noteEditor.dictationProcessing'),
      dictationIdle: translate('item.noteEditor.dictationIdle'),
      helperText: translate('item.noteEditor.helper'),
      dictationNoMicrophone: translate('item.noteEditor.noMicrophone'),
      dictationNoAudio: translate('item.noteEditor.noAudio'),
      dictationAutoStopProcessing: translate('item.noteEditor.autoStopProcessing', {
        duration: '{duration}',
      }),
      dictationTranscribing: translate('item.noteEditor.transcribing'),
      dictationAutoStopInserted: translate('item.noteEditor.autoStopInserted', {
        duration: '{duration}',
      }),
      dictationInserted: translate('item.noteEditor.inserted'),
      dictationNoText: translate('item.noteEditor.noText'),
      dictationTranscriptionFailed: translate('item.noteEditor.transcriptionFailed'),
      linkInvalidUrl: translate('item.noteEditor.linkInvalidUrl'),
      linkInvalidHttp: translate('item.noteEditor.linkInvalidHttp'),
      linkInvalidExample: translate('item.noteEditor.linkInvalidExample'),
      linkModalTitle: translate('item.noteEditor.linkTitle'),
      linkModalDescription: translate('item.noteEditor.linkDescription'),
      linkUrlLabel: translate('item.noteEditor.linkUrlLabel'),
      linkPlaceholder: translate('item.noteEditor.linkPlaceholder'),
      linkCancel: translate('item.noteEditor.linkCancel'),
      linkSubmit: translate('item.noteEditor.linkSubmit'),
    }
  })

  const layoutFilterLabels = $derived.by(() => {
    $currentLocale
    return Object.fromEntries(
      LAYOUT_BLOCK_FILTERS.map((filter) => [filter.id, translate(`item.layoutFilter.${filter.id}`)])
    ) as Record<LayoutBlockFilterId, string>
  })

  // LLM state (Gemma 4)
  const llmStore = new LlmStore({
    onComplete: (id, job, result) => {
      llmTick++
      // Track summary results in the dedicated map
      if (job === 'summarize') {
        summaryTexts.set(id, result)
        summaryTick++
      }
      // When LLM triples complete, reload triples from DB (they're now in the triples table)
      if (job === 'extract_triples') {
        loadTriples()
        nlpStore._setJobStatus(itemId, 'triples', 'done')
        nlpTick++
      }
      if (job === 'correct_ocr') {
        ocrCorrectedAssets.add(id)
        ocrTick++ // Force Svelte reactivity for the textarea
        const assetId = selectedAsset?.id === id ? id : null
        if (assetId) {
          ocrEditedText.set(assetId, result)
          ocrStore.setTextContent(assetId, result)
          schedulePersist(assetId, result)
        } else {
          // Item-level (legacy): update the first asset's text or whichever asset has OCR text
          const asset = assets.find((a: Asset) => ocrStore.getTextContent(a.id))
          if (asset) {
            ocrEditedText.set(asset.id, result)
            ocrStore.setTextContent(asset.id, result)
            schedulePersist(asset.id, result)
          }
        }
      }
    },
    onCorrectOcr: (id, _result) => {
      // Track that OCRC already ran for this asset (from persisted results or live)
      ocrCorrectedAssets.add(id)
    },
    onError: (id, job, error) => {
      // When LLM triples extraction fails, set NLP triples status to error
      if (job === 'extract_triples') {
        nlpStore._setJobStatus(itemId, 'triples', 'error', error)
        nlpTick++
      }
    },
  })
  let llmTick = $state(0)

  // OCRC tracking: once OCRC is done for an asset, hide the button and show
  // only Embedding + Triple buttons in the LLM section.
  let ocrCorrectedAssets = $state(new Set<string>()) // asset IDs that have been OCRC'd

  let llmAvailable = $state(false)
  let summaryTexts = $state(new Map<string, string>()) // assetId → summary text
  let summaryTick = $state(0) // reactivity trigger for summary display

  /**
   * Get the LLM state for the currently active context.
   * When a specific asset/page is selected (multipage), use the asset ID
   * so LLM state is scoped per-page. Otherwise fall back to item ID.
   */
  function getLlmState() {
    void llmTick
    const targetId = selectedAsset ? selectedAsset.id : itemId
    return llmStore.getState(targetId)
  }

  async function handleLlmSummarize() {
    error = null
    try {
      if (selectedAsset) {
        await llmSummarizeAsset(selectedAsset.id)
      } else {
        await llmSummarize(itemId)
      }
    } catch (e) {
      console.error('[LLM] summarize failed:', e)
      error = translate('item.error.summarize')
    }
  }

  async function handleLlmCorrectOcr() {
    error = null
    try {
      if (selectedAsset) {
        await llmCorrectOcrAsset(selectedAsset.id)
      } else {
        await llmCorrectOcr(itemId)
      }
    } catch (e) {
      console.error('[LLM] correct OCR failed:', e)
      error = translate('item.error.correctOcr')
    }
  }

  async function handleLlmExtractTriples() {
    nlpStore._setJobStatus(itemId, 'triples', 'pending')
    nlpTick++
    try {
      if (selectedAsset) {
        await llmExtractTriplesAsset(selectedAsset.id)
      } else {
        await llmExtractTriples(itemId)
      }
    } catch (e) {
      console.error('[LLM] extract triples failed:', e)
      nlpStore._setJobStatus(itemId, 'triples', 'error', e instanceof Error ? e.message : 'Failed')
      nlpTick++
    }
  }

  // Geo state (OpenStreetMap)
  const geoStore = new GeoStore({
    onEntityComplete: () => {
      reloadEntitiesAndGeoMarkers()
    },
    onItemComplete: () => {
      reloadEntitiesAndGeoMarkers()
    },
  })
  let geoMarkers = $state<MapMarker[]>([])

  async function loadGeoMarkers(currentEntities = entities, asset: Asset | null = selectedAsset) {
    const requestToken = ++geoMarkersLoadToken
    try {
      const placeEntitiesById = new Map(
        currentEntities
          .filter((entity) => entity.entityType === 'place')
          .map((entity) => [entity.id, entity])
      )

      if (placeEntitiesById.size === 0) {
        if (geoMarkersLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
          return
        }
        geoMarkers = []
        return
      }

      const rows = await invoke<
        Array<{ id: string; value: string; latitude: number; longitude: number }>
      >('db_select', {
        sql: `SELECT id, value, latitude, longitude FROM entities
              WHERE item_id = ? AND entity_type = 'place' AND geo_status = 'resolved'
              AND latitude IS NOT NULL AND longitude IS NOT NULL
              AND (source IS NULL OR source != 'manual_deleted')`,
        params: [itemId],
      })
      if (geoMarkersLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
        return
      }
      geoMarkers = rows.flatMap((r) => {
        const entity = placeEntitiesById.get(r.id)
        if (!entity) return []

        return [
          {
            entityId: r.id,
            label: entity.value,
            latitude: r.latitude,
            longitude: r.longitude,
          },
        ]
      })
    } catch (e) {
      console.error('[geo] Failed to load markers:', e)
    }
  }

  let metadataValue = $derived<Record<string, string>>(
    item?.metadata ? parseMetadataRecord(item.metadata) : {}
  )
  let originalFileMetadata = $derived<ImportedFileMetadata | null>(
    item?.metadata ? parseImportedFileMetadata(item.metadata) : null
  )
  let customMetadataNormalizedKeys = $derived(
    new Set(Object.keys(metadataValue).map((key) => normalizeMetadataKey(key)))
  )

  // Topic state
  let itemTopics = $state<string[]>([])
  let topicSuggestions = $state<string[]>([])

  async function loadTopics() {
    try {
      const topics = await getStore().topics.findByItemId(itemId)
      itemTopics = topics.map((t) => t.name)
    } catch (e) {
      console.error('[topics] Failed to load topics:', e)
    }
  }

  async function loadTopicSuggestions() {
    try {
      topicSuggestions = await getStore().topics.allNames()
    } catch (e) {
      console.error('[topics] Failed to load suggestions:', e)
    }
  }

  async function handleTopicsChange(newTopics: string[]) {
    try {
      const store = getStore()
      // Find topics to add (in new but not in current)
      const currentSet = new Set(itemTopics)
      const newSet = new Set(newTopics)
      // Add new topics
      for (const name of newTopics) {
        if (!currentSet.has(name)) {
          await store.topics.addTopicToItem(itemId, name)
        }
      }
      // Remove topics no longer present
      for (const name of itemTopics) {
        if (!newSet.has(name)) {
          const topic = await store.topics.findByName(name)
          if (topic) {
            await store.topics.removeTopicFromItem(itemId, topic.id)
          }
        }
      }
      itemTopics = newTopics.map((t) => t.toUpperCase())
      // Refresh suggestions to include any newly created topics
      void loadTopicSuggestions()
    } catch (e) {
      console.error('[topics] Failed to save topics:', e)
    }
  }

  let selectedAsset = $derived(assets[selectedAssetIndex] ?? null)
  let fileMetadataEntries = $derived(
    buildTechnicalMetadata({
      item,
      selectedAsset,
      collection,
      originalFileMetadata,
      customMetadataKeys: customMetadataNormalizedKeys,
    })
  )

  let viewerSrc = $derived(
    selectedAsset
      ? getAssetUrl(selectedAsset.path) + (imageVersion > 0 ? `?_t=${imageVersion}` : '')
      : ''
  )

  let viewerType = $derived<'image' | 'pdf' | 'audio'>(
    selectedAsset?.type === 'pdf' ? 'pdf' : selectedAsset?.type === 'audio' ? 'audio' : 'image'
  )

  let layoutBlocks = $derived(assetLayout ? buildLayoutBlockViews(assetLayout) : [])
  let layoutPages = $derived(getPagesFromLayout(assetLayout))
  let layoutPageOptions = $derived(
    viewerType === 'pdf' && assetLayout
      ? Array.from(
          { length: Math.max(viewerTotalPages, layoutPages[layoutPages.length - 1] ?? 0) },
          (_, index) => index + 1
        )
      : []
  )
  let layoutActivePage = $derived(viewerType === 'pdf' ? viewerPage : (layoutPages[0] ?? 1))
  let layoutBlockCountsByPage = $derived(getBlockCountByPage(layoutBlocks))
  let layoutPageRegions = $derived(
    assetLayout
      ? viewerType === 'pdf'
        ? filterRegionsByPage(assetLayout.regions, layoutActivePage)
        : assetLayout.regions
      : []
  )
  let layoutPageBlocks = $derived(
    viewerType === 'pdf' ? filterBlocksByPage(layoutBlocks, layoutActivePage) : layoutBlocks
  )
  let layoutFilterCounts = $derived(countLayoutBlocksByFilter(layoutPageBlocks))
  let visibleLayoutBlocks = $derived(filterLayoutBlocksByType(layoutPageBlocks, layoutTypeFilter))
  let selectedLayoutBlock = $derived(findVisibleLayoutBlockById(layoutSelectedBlockId))
  let layoutRegions = $derived<ViewerLayoutRegion[]>(
    visibleLayoutBlocks.map((block) => ({
      id: block.regionId,
      blockId: block.id,
      label: block.label,
      x: block.overlayBbox.x,
      y: block.overlayBbox.y,
      width: block.overlayBbox.width,
      height: block.overlayBbox.height,
      matchSource: block.overlaySource,
    }))
  )
  let layoutReferenceWidth = $derived(
    layoutPageBlocks[0]?.imageWidth ??
      layoutPageRegions[0]?.imageWidth ??
      assetLayout?.imageWidth ??
      0
  )
  let layoutReferenceHeight = $derived(
    layoutPageBlocks[0]?.imageHeight ??
      layoutPageRegions[0]?.imageHeight ??
      assetLayout?.imageHeight ??
      0
  )
  let hasLayoutData = $derived(Boolean(assetLayout && layoutBlocks.length > 0))

  function findVisibleLayoutBlockById(blockId: string | null) {
    if (!blockId) return null
    return visibleLayoutBlocks.find((block) => block.id === blockId) ?? null
  }

  function findVisibleLayoutBlockByRegionId(regionId: string | null) {
    if (!regionId) return null
    return visibleLayoutBlocks.find((block) => block.regionId === regionId) ?? null
  }

  function syncLayoutHoverFromBlock(blockId: string | null) {
    const block = findVisibleLayoutBlockById(blockId)
    layoutHoveredBlockId = block?.id ?? null
    layoutHoveredRegionId = block?.regionId ?? null
  }

  function syncLayoutHoverFromRegion(regionId: string | null) {
    const block = findVisibleLayoutBlockByRegionId(regionId)
    layoutHoveredBlockId = block?.id ?? null
    layoutHoveredRegionId = block?.regionId ?? null
  }

  function setSelectedLayoutBlock(blockId: string | null) {
    const block = findVisibleLayoutBlockById(blockId)
    layoutSelectedBlockId = block?.id ?? null
    layoutSelectedRegionId = block?.regionId ?? null
    if (block) {
      showLayout = true
    }
  }

  function setSelectedLayoutRegion(regionId: string | null) {
    const block = findVisibleLayoutBlockByRegionId(regionId)
    layoutSelectedBlockId = block?.id ?? null
    layoutSelectedRegionId = block?.regionId ?? null
    if (block) {
      showLayout = true
    }
  }

  async function persistAnnotations(assetId: string, nextAnnotations: ViewerAnnotation[]) {
    try {
      const inputs = nextAnnotations.map((a) => ({
        kind: a.kind as StoreAnnotationKind,
        color: a.color,
        x: a.x,
        y: a.y,
        width: a.width,
        height: a.height,
      }))
      await getStore().annotations.replaceForAssetPage(assetId, 1, inputs)
      annotationSaveError = null
    } catch {
      annotationSaveError = 'Failed to save annotations. Changes remain local until retry.'
    }
  }

  function clearAnnotationSaveTimer() {
    if (annotationSaveTimer) {
      clearTimeout(annotationSaveTimer)
      annotationSaveTimer = null
    }
  }

  async function flushPendingAnnotationSave() {
    clearAnnotationSaveTimer()

    if (!pendingAnnotationSave) {
      return
    }

    const saveJob = pendingAnnotationSave
    pendingAnnotationSave = null
    await persistAnnotations(saveJob.assetId, saveJob.annotations)
  }

  function scheduleAnnotationPersist(assetId: string, nextAnnotations: ViewerAnnotation[]) {
    clearAnnotationSaveTimer()
    pendingAnnotationSave = {
      assetId,
      annotations: nextAnnotations,
    }

    annotationSaveTimer = setTimeout(async () => {
      const saveJob = pendingAnnotationSave
      pendingAnnotationSave = null
      annotationSaveTimer = null

      if (!saveJob) {
        return
      }

      await persistAnnotations(saveJob.assetId, saveJob.annotations)
    }, 500)
  }

  function handleAnnotationsChange(nextAnnotations: ViewerAnnotation[]) {
    if (!selectedAsset || selectedAsset.type !== 'image') {
      return
    }

    annotations = normalizeAnnotationsForAsset({
      annotations: nextAnnotations,
      assetId: selectedAsset.id,
      now: Date.now(),
      createId: () => crypto.randomUUID(),
    })
    annotationSaveError = null
    scheduleAnnotationPersist(selectedAsset.id, annotations)
  }

  function handleSelectedAnnotationIdChange(annotationId: string | null) {
    selectedAnnotationId = annotationId
  }

  function handleAnnotationToolChange(tool: 'select' | 'rectangle' | 'underline') {
    annotationTool = tool
  }

  function handleAnnotationColorChange(color: string) {
    annotationColor = color
  }

  // ── Image editing handlers ────────────────────────────────────────────

  /** Adjust annotations after a rotation. Converted = new image dimensions. */
  function adjustAnnotationsAfterRotation(rotation: RotationDirection) {
    annotations = rotateAnnotations(annotations, rotation)
  }

  /** Adjust annotations after a crop. Region is the crop area in normalized coords. */
  function adjustAnnotationsAfterCrop(region: NormalizedRegion) {
    annotations = cropAnnotations(annotations, region)
  }

  async function handleEditSelect(region: { x: number; y: number; width: number; height: number }) {
    if (!selectedAsset || selectedAsset.type !== 'image') return
    if (imageNaturalW === 0 || imageNaturalH === 0) return

    await flushPendingAnnotationSave()

    const asset = selectedAsset
    const pixelRegion = normalizedToPixels(region, imageNaturalW, imageNaturalH)

    // Push current state onto undo stack before performing the edit
    undoStack = [
      ...undoStack,
      {
        path: asset.path,
        width: imageNaturalW,
        height: imageNaturalH,
        annotations: JSON.parse(JSON.stringify(annotations)),
      },
    ]

    try {
      if (editTool === 'crop') {
        const result: ImageEditResult = await invoke('crop_image', {
          path: asset.path,
          x: pixelRegion.x,
          y: pixelRegion.y,
          width: pixelRegion.width,
          height: pixelRegion.height,
        })
        adjustAnnotationsAfterCrop(region)
        await handleImageEditResult(result, asset.id)
      } else if (editTool === 'erase') {
        const result: ImageEditResult = await invoke('erase_region', {
          path: asset.path,
          x: pixelRegion.x,
          y: pixelRegion.y,
          width: pixelRegion.width,
          height: pixelRegion.height,
          fill: 'white',
        })
        await handleImageEditResult(result, asset.id)
      }
    } catch (e) {
      // On failure, pop the undo entry since the edit didn't succeed
      undoStack = undoStack.slice(0, -1)
      console.error('[ItemView] Image edit failed:', e)
    } finally {
      // Reset edit tool after operation
      editTool = 'none'
    }
  }

  async function handleRotateLeft() {
    if (!selectedAsset || selectedAsset.type !== 'image') return
    await flushPendingAnnotationSave()
    const asset = selectedAsset

    // Push current state onto undo stack before rotating
    undoStack = [
      ...undoStack,
      {
        path: asset.path,
        width: imageNaturalW,
        height: imageNaturalH,
        annotations: JSON.parse(JSON.stringify(annotations)),
      },
    ]

    try {
      const result: ImageEditResult = await invoke('rotate_image', {
        path: asset.path,
        direction: 'left',
      })
      adjustAnnotationsAfterRotation('left')
      await handleImageEditResult(result, asset.id)
    } catch (e) {
      undoStack = undoStack.slice(0, -1)
      console.error('[ItemView] Rotate left failed:', e)
    }
  }

  async function handleRotateRight() {
    if (!selectedAsset || selectedAsset.type !== 'image') return
    await flushPendingAnnotationSave()
    const asset = selectedAsset

    // Push current state onto undo stack before rotating
    undoStack = [
      ...undoStack,
      {
        path: asset.path,
        width: imageNaturalW,
        height: imageNaturalH,
        annotations: JSON.parse(JSON.stringify(annotations)),
      },
    ]

    try {
      const result: ImageEditResult = await invoke('rotate_image', {
        path: asset.path,
        direction: 'right',
      })
      adjustAnnotationsAfterRotation('right')
      await handleImageEditResult(result, asset.id)
    } catch (e) {
      undoStack = undoStack.slice(0, -1)
      console.error('[ItemView] Rotate right failed:', e)
    }
  }

  /** Undo the last image edit: restore the asset path, dimensions,
   *  and annotations to the previous state. */
  async function handleUndo() {
    if (!selectedAsset || selectedAsset.type !== 'image') return
    if (undoStack.length === 0) return

    await flushPendingAnnotationSave()

    const entry = undoStack[undoStack.length - 1]!
    const assetId = selectedAsset.id

    // Restore state from undo entry
    const store = getStore()
    await store.assets.updatePath(assetId, entry.path)
    assets = assets.map((a) => (a.id === assetId ? { ...a, path: entry.path } : a))
    annotations = entry.annotations
    selectedAnnotationId = null
    // Force image refresh
    imageVersion++

    // Persist the restored annotations
    await persistAnnotations(assetId, annotations)

    // Pop the undo stack
    undoStack = undoStack.slice(0, -1)

    // Notify other views
    try {
      await emit('asset:image-updated', {
        itemId,
        assetId,
        path: entry.path,
      })
    } catch (e) {
      console.warn('[ItemView] Failed to emit asset:image-updated event on undo:', e)
    }
  }

  /** Post-edit: always update asset path in DB (even if format didn't change),
   *  refresh image, persist annotations, push undo entry, and notify other views. */
  async function handleImageEditResult(result: ImageEditResult, assetId: string) {
    // Always update the asset path in DB — versioned paths change on every edit,
    // and the DB must reflect the current file on disk.
    const store = getStore()
    await store.assets.updatePath(assetId, result.path)
    // Update the local assets array with the new path
    assets = assets.map((a) => (a.id === assetId ? { ...a, path: result.path } : a))

    // Force image refresh: bump version counter so the browser fetches the
    // new file (versioned paths already make the URL unique, but this helps
    // if something caches at the protocol level).
    imageVersion++

    // Persist adjusted annotations
    if (selectedAsset && selectedAsset.id === assetId) {
      await persistAnnotations(assetId, annotations)
    }

    // Notify CollectionView (and any other listeners) that the asset path
    // has changed, so they can invalidate their cached thumbnail URLs.
    try {
      await emit('asset:image-updated', {
        itemId,
        assetId,
        path: result.path,
      })
    } catch (e) {
      console.warn('[ItemView] Failed to emit asset:image-updated event:', e)
    }
  }

  // Track natural image dimensions for pixel coordinate conversion
  let imageNaturalW = $state(0)
  let imageNaturalH = $state(0)

  let metadataSaveTimer: ReturnType<typeof setTimeout> | null = null

  async function handleExtractText(asset: Asset, mode: OcrMode = 'light') {
    ocrStore._updateState(asset.id, { status: 'pending', progress: 0 })
    ocrTick++
    try {
      await extractText(asset.id, asset.path, asset.type, mode)
    } catch (e) {
      ocrStore._updateState(asset.id, {
        status: 'error',
        error: e instanceof Error ? e.message : 'Extraction failed',
      })
      ocrTick++
    }
  }

  async function handleTranscribeAudio(asset: Asset) {
    transcriptionStore._updateState(asset.id, { status: 'pending', progress: 0 })
    transcriptionTick++
    try {
      await transcribeAudio(asset.id, asset.path)
    } catch (e) {
      transcriptionStore._updateState(asset.id, {
        status: 'error',
        error: e instanceof Error ? e.message : 'Transcription failed',
      })
      transcriptionTick++
    }
  }

  async function handleTranscribeDictation(audio: Blob): Promise<string> {
    return transcribeDictation(audio)
  }

  function getExtractionPrimaryActionLabel(assetType: Asset['type']) {
    if (assetType === 'pdf') {
      return translate('item.pdfTextAction')
    }

    return translate('item.ocrHighAction')
  }

  function getCorrectionActionLabel(assetType: Asset['type']) {
    return assetType === 'pdf'
      ? translate('item.pdfCorrectAction')
      : translate('item.ocrCorrectAction')
  }

  function getSummaryActionLabel(assetType: Asset['type']) {
    if (assetType === 'pdf') {
      return translate('item.summaryPdfAction')
    }

    if (assetType === 'audio') {
      return translate('item.summaryAudioAction')
    }

    return translate('item.summaryAction')
  }

  function getTranscriptionActionLabel(busy: boolean) {
    return busy ? translate('item.transcribeBusyAction') : translate('item.transcribeShortAction')
  }

  function getTranscriptionStageLabel(stage?: string) {
    if (!stage) return ''

    switch (stage) {
      case 'uploading':
        return translate('item.transcriptionStage.uploading')
      case 'submitting_remote':
        return translate('item.transcriptionStage.submitting_remote')
      case 'polling_remote':
        return translate('item.transcriptionStage.polling_remote')
      default:
        return ''
    }
  }

  function getOcrStageLabel(stage?: string) {
    if (!stage || stage === 'done' || stage === 'error') return ''

    switch (stage) {
      case 'reading':
        return translate('item.ocrStage.reading')
      case 'extracting_native':
        return translate('item.ocrStage.extracting_native')
      case 'ocr_inference':
        return translate('item.ocrStage.ocr_inference')
      case 'paddlevl_detection':
        return translate('item.ocrStage.paddlevl_detection')
      case 'submitting_glm_ocr':
        return translate('item.ocrStage.submitting_glm_ocr')
      case 'waiting_glm_ocr':
        return translate('item.ocrStage.waiting_glm_ocr')
      case 'parsing_glm_ocr':
        return translate('item.ocrStage.parsing_glm_ocr')
      default:
        return ''
    }
  }

  function getOcrState(assetId: string) {
    // Depend on ocrTick to trigger Svelte reactivity when events arrive
    void ocrTick
    return ocrStore.getState(assetId)
  }

  function getTranscriptionState(assetId: string) {
    void transcriptionTick
    return transcriptionStore.getState(assetId)
  }

  function getNlpState() {
    void nlpTick
    return nlpStore.getState(itemId)
  }

  async function handleIndexFts() {
    nlpStore._setJobStatus(itemId, 'fts', 'pending')
    nlpTick++
    try {
      await indexFts(itemId)
    } catch (e) {
      nlpStore._setJobStatus(itemId, 'fts', 'error', e instanceof Error ? e.message : 'Failed')
      nlpTick++
    }
  }

  let activeAssetSummary = $derived(
    selectedAsset
      ? `${getAssetTypeLabel(selectedAsset.type)} · ${getAssetPathLabel(selectedAsset.path)}`
      : 'Sin asset seleccionado'
  )

  function isCurrentSelectedAsset(asset: Asset | null) {
    return (selectedAsset?.id ?? null) === (asset?.id ?? null)
  }

  async function handleEmbedAsset() {
    if (!selectedAsset) {
      nlpStore._setJobStatus(
        itemId,
        'embed',
        'error',
        'Select an asset before generating embeddings.'
      )
      nlpTick++
      return
    }

    nlpStore._setJobStatus(itemId, 'embed', 'pending')
    nlpTick++
    try {
      await embedAsset(itemId, selectedAsset.id)
    } catch (e) {
      nlpStore._setJobStatus(itemId, 'embed', 'error', e instanceof Error ? e.message : 'Failed')
      nlpTick++
    }
  }

  async function handleExtractEntities() {
    nlpStore._setJobStatus(itemId, 'ner', 'pending')
    nlpTick++
    try {
      if (selectedAsset) {
        await extractEntitiesForAsset(itemId, selectedAsset.id)
      } else {
        await extractEntities(itemId)
      }
    } catch (e) {
      nlpStore._setJobStatus(itemId, 'ner', 'error', e instanceof Error ? e.message : 'Failed')
      nlpTick++
    }
  }

  async function loadEntities(asset: Asset | null = selectedAsset) {
    const requestToken = ++entitiesLoadToken
    try {
      const store = getStore()
      let nextEntities: Entity[]
      if (asset) {
        nextEntities = ((await store.entities.findByAssetId(itemId, asset.id)) as Entity[]).filter(
          (entity) => entity.confidence == null || entity.confidence > 0.89
        )
      } else {
        nextEntities = ((await store.entities.findByItemId(itemId)) as Entity[]).filter(
          (entity) => entity.confidence == null || entity.confidence > 0.89
        )
      }
      if (entitiesLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
        return null
      }
      entities = nextEntities
      return nextEntities
    } catch {
      if (entitiesLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
        return null
      }
      // Non-fatal: entities panel shows empty state
      entities = []
      return []
    }
  }

  async function reloadEntitiesAndGeoMarkers(asset: Asset | null = selectedAsset) {
    const nextEntities = await loadEntities(asset)
    if (!nextEntities) return
    await loadGeoMarkers(nextEntities, asset)
  }

  function normalizeManualEntityValue(value: string) {
    return value
      .trim()
      .replace(/^["'“”‘’«»\-–—\s]+|["'“”‘’«»\-–—\s]+$/g, '')
      .trim()
  }

  function toEditableEntityType(entityType: Entity['entityType']): EditableEntityType {
    if (
      entityType === 'person' ||
      entityType === 'organization' ||
      entityType === 'place' ||
      entityType === 'misc' ||
      entityType === 'date'
    ) {
      return entityType
    }
    return 'organization'
  }

  async function handleCreateEntity() {
    const value = normalizeManualEntityValue(newEntityValue)
    if (!value) return
    try {
      await getStore().entities.create({
        itemId,
        assetId: selectedAsset?.id ?? null,
        entityType: newEntityType,
        value,
        startOffset: 0,
        endOffset: 0,
        confidence: 1.0,
        source: 'manual',
        modelName: null,
        createdAt: Date.now(),
      })
      newEntityValue = ''
      newEntityType = 'organization'
      entityActionError = null
      await reloadEntitiesAndGeoMarkers()
    } catch (e) {
      entityActionError = e instanceof Error ? e.message : 'Failed to add entity'
    }
  }

  function startEditingEntity(entity: Entity) {
    editingEntityId = entity.id
    editingEntityValue = entity.value
    entityActionError = null
  }

  function cancelEditingEntity() {
    editingEntityId = null
    editingEntityValue = ''
  }

  function handleEditingEntityValueChange(value: string) {
    editingEntityValue = value
  }

  async function handleSaveEntity(entityId: string, nextValue = editingEntityValue) {
    const value = normalizeManualEntityValue(nextValue)
    if (!value) return
    const entity = entities.find((candidate) => candidate.id === entityId)
    if (!entity) return
    try {
      await getStore().entities.update(entityId, {
        entityType: toEditableEntityType(entity.entityType),
        value,
        confidence: 1.0,
        source: 'manual',
      })
      cancelEditingEntity()
      entityActionError = null
      await reloadEntitiesAndGeoMarkers()
    } catch (e) {
      entityActionError = e instanceof Error ? e.message : 'Failed to save entity'
    }
  }

  async function handleDeleteEntity(entityId: string) {
    try {
      await getStore().entities.delete(entityId)
      if (editingEntityId === entityId) {
        cancelEditingEntity()
      }
      entityActionError = null
      await reloadEntitiesAndGeoMarkers()
    } catch (e) {
      entityActionError = e instanceof Error ? e.message : 'Failed to delete entity'
    }
  }

  async function loadSimilarAssets(asset: Asset | null = selectedAsset) {
    const requestToken = ++similarAssetsLoadToken
    if (!asset) {
      similarAssets = []
      return
    }

    try {
      const nextSimilarAssets = await fetchSimilarAssets(asset.id, 5)
      if (similarAssetsLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
        return
      }
      similarAssets = nextSimilarAssets
    } catch {
      if (similarAssetsLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
        return
      }
      similarAssets = []
    }
  }

  function navigateToSimilarItem(item: { itemId: string; title: string; collectionId: string }) {
    navigation.replace({
      name: 'item',
      itemId: item.itemId,
      collectionId: item.collectionId,
      collectionName: '',
      itemTitle: item.title || item.itemId,
    })
  }

  function clearFtsSearchTimer() {
    if (ftsSearchTimer) {
      clearTimeout(ftsSearchTimer)
      ftsSearchTimer = null
    }
  }

  async function runFtsSearch(rawQuery: string) {
    const query = rawQuery.trim()
    if (!query) {
      ftsResults = []
      ftsSearchError = null
      ftsSearching = false
      ftsDebug = null
      return
    }

    ftsSearching = true
    ftsSearchError = null

    try {
      const store = getStore()
      if (isDev) {
        const stats = await store.fts.stats()
        ftsIndexedRows = stats.totalRows
      }

      const response = await store.fts.searchWithDebug(query, 10)
      const rows = response.results

      const hydrated = await Promise.all(
        rows.map(async (row) => {
          const found = await store.items.findById(row.itemId)
          if (!found) return null

          return {
            itemId: found.id,
            title: found.title,
            rank: row.rank,
            collectionId: found.collectionId,
          }
        })
      )

      ftsResults = hydrated.filter(
        (row): row is { itemId: string; title: string; rank: number; collectionId: string } => !!row
      )

      if (isDev) {
        ftsDebug = {
          ...response.debug,
          hydratedCount: ftsResults.length,
        }
      }
    } catch {
      ftsResults = []
      ftsSearchError = 'No se pudo ejecutar la búsqueda full-text.'
      if (isDev) {
        ftsDebug = null
      }
    } finally {
      ftsSearching = false
    }
  }

  async function loadFtsStats() {
    if (!isDev) return

    try {
      const store = getStore()
      const stats = await store.fts.stats()
      ftsIndexedRows = stats.totalRows
    } catch {
      ftsIndexedRows = null
    }
  }

  function handleFtsInput(event: Event) {
    const value = (event.currentTarget as HTMLInputElement).value
    ftsQuery = value

    clearFtsSearchTimer()
    if (!value.trim()) {
      ftsResults = []
      ftsSearchError = null
      ftsSearching = false
      ftsDebug = null
      return
    }

    ftsSearchTimer = setTimeout(() => {
      void runFtsSearch(value)
    }, 250)
  }

  function handleFtsKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault()
      clearFtsSearchTimer()
      void runFtsSearch(ftsQuery)
      return
    }

    if (event.key === 'Escape') {
      event.preventDefault()
      clearFtsSearchTimer()
      ftsQuery = ''
      ftsResults = []
      ftsSearchError = null
      ftsSearching = false
      ftsDebug = null
    }
  }

  async function loadTriples(asset: Asset | null = selectedAsset) {
    const requestToken = ++triplesLoadToken
    try {
      const store = getStore()
      const nextTriples = asset
        ? await store.triples.findByAssetId(itemId, asset.id)
        : await store.triples.findByItemId(itemId)
      if (triplesLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
        return
      }
      triples = nextTriples
    } catch {
      if (triplesLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
        return
      }
      triples = []
    }
  }

  async function refreshNotesForAsset(asset: Asset | null = selectedAsset) {
    const requestToken = ++notesLoadToken
    const loadedNotes = await loadNotesForAsset(asset)
    if (notesLoadToken !== requestToken || !isCurrentSelectedAsset(asset)) {
      return false
    }
    notes = loadedNotes
    return true
  }

  async function reloadSelectedAssetPersistedState(options: {
    layout?: boolean
    entities?: boolean
    triples?: boolean
    similarAssets?: boolean
  }) {
    const asset = selectedAsset
    if (!asset) return

    const reloads: Promise<unknown>[] = []

    if (options.layout && asset.type !== 'audio') {
      reloads.push(reloadLayoutForAsset(asset))
    }
    if (options.entities) {
      reloads.push(reloadEntitiesAndGeoMarkers(asset))
    }
    if (options.triples) {
      reloads.push(loadTriples(asset))
    }
    if (options.similarAssets) {
      reloads.push(loadSimilarAssets(asset))
    }

    await Promise.allSettled(reloads)
  }

  function handleMetadataChange(metadata: Record<string, string>) {
    if (metadataSaveTimer) clearTimeout(metadataSaveTimer)
    metadataSaveTimer = setTimeout(async () => {
      if (!item) return
      try {
        savingMetadata = true
        const store = getStore()
        await store.items.update(item.id, {
          metadata: JSON.stringify(mergeReservedMetadata(metadata, item.metadata)),
        })
      } catch (e) {
        error = e instanceof Error ? e.message : 'Failed to save metadata'
      } finally {
        savingMetadata = false
      }
    }, 1000)
  }

  async function handleSaveNote(content: string) {
    const asset = selectedAsset
    try {
      error = null
      const store = getStore()
      await store.notes.create({ itemId, assetId: asset?.id ?? null, content })
      await refreshNotesForAsset(asset)
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to save note'
    }
  }

  let pendingDeleteNoteId = $state<string | null>(null)
  let deletingNote = $state(false)

  async function handleDeleteNote(noteId: string) {
    const asset = selectedAsset
    try {
      error = null
      deletingNote = true
      const store = getStore()
      await store.notes.delete(noteId)
      await refreshNotesForAsset(asset)
      if (expandedNoteId === noteId) {
        expandedNoteId = null
      }
      if (editingNoteId === noteId) {
        editingNoteId = null
      }
      pendingDeleteNoteId = null
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to delete note'
    } finally {
      deletingNote = false
    }
  }

  // Note editing state
  let editingNoteId = $state<string | null>(null)
  let expandedNoteId = $state<string | null>(null)

  function openDeleteNoteConfirm(noteId: string) {
    pendingDeleteNoteId = noteId
  }

  function handleDeleteNoteCancel() {
    if (deletingNote) return
    pendingDeleteNoteId = null
  }

  async function handleDeleteNoteConfirm() {
    if (!pendingDeleteNoteId || deletingNote) return
    await handleDeleteNote(pendingDeleteNoteId)
  }

  function handleEditNote(note: Note) {
    editingNoteId = note.id
  }

  function toggleNoteExpanded(noteId: string) {
    expandedNoteId = expandedNoteId === noteId ? null : noteId
  }

  async function handleSaveEdit(noteId: string, content: string) {
    if (isNoteHtmlEffectivelyEmpty(content)) return
    const asset = selectedAsset
    try {
      error = null
      const store = getStore()
      await store.notes.update(noteId, content)
      await refreshNotesForAsset(asset)
      editingNoteId = null
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to update note'
    }
  }

  function handleCancelEdit() {
    editingNoteId = null
  }

  /** Load notes scoped to the current asset (plus item-level notes). */
  async function loadNotesForAsset(asset: Asset | null = selectedAsset): Promise<Note[]> {
    if (!asset) {
      const store = getStore()
      return store.notes.findByItem(itemId)
    }
    const store = getStore()
    return store.notes.findByAsset(itemId, asset.id)
  }

  async function loadData() {
    try {
      loading = true
      error = null
      selectedAssetIndex = 0 // Reset page selection on item change
      const store = getStore()
      const [loadedItem, loadedAssets, loadedCollection] = await Promise.all([
        store.items.findById(itemId),
        store.assets.findByItem(itemId),
        store.collections.findById(collectionId),
      ])
      item = loadedItem
      assets = loadedAssets
      collection = loadedCollection
      // Asset-scoped data (notes, entities, triples, similar assets) will be loaded by the selectedAsset effect
      void loadTopics()
      void loadTopicSuggestions()
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load item'
    } finally {
      loading = false
    }
  }

  async function reloadLayoutForAsset(asset: Asset | null) {
    const requestToken = ++layoutLoadToken

    if (!asset || asset.type === 'audio') {
      assetLayout = null
      layoutLoading = false
      layoutError = null
      return
    }

    layoutLoading = true
    layoutError = null

    try {
      const layout = await getLayoutByAsset(asset.id)
      if (layoutLoadToken !== requestToken || selectedAsset?.id !== asset.id) {
        return
      }

      assetLayout = layout
      if (!layout || layout.blocks.length === 0) {
        showLayout = false
      }
    } catch (e) {
      if (layoutLoadToken !== requestToken || selectedAsset?.id !== asset.id) {
        return
      }

      assetLayout = null
      layoutError = e instanceof Error ? e.message : 'Failed to load layout'
      showLayout = false
    } finally {
      if (layoutLoadToken === requestToken && selectedAsset?.id === asset.id) {
        layoutLoading = false
      }
    }
  }

  $effect(() => {
    const asset = selectedAsset
    const currentAssetId = asset?.id ?? null
    const switchedAsset = currentAssetId !== lastSelectedAssetId

    lastSelectedAssetId = currentAssetId

    if (switchedAsset) {
      selectedAnnotationId = null
      annotationTool = 'select'
      editTool = 'none'
      viewerPage = 1
      viewerTotalPages = 1
      showLayout = false
      layoutTypeFilter = 'all'
      layoutHoveredBlockId = null
      layoutSelectedBlockId = null
      layoutHoveredRegionId = null
      layoutSelectedRegionId = null
      // Reset undo stack only when switching to a DIFFERENT asset by id.
      // Editing the same asset creates a new versioned path, which should NOT
      // clear undo history.
      undoStack = []
    }

    if (pendingAnnotationSave && pendingAnnotationSave.assetId !== currentAssetId) {
      void flushPendingAnnotationSave()
    }

    if (!asset || asset.type !== 'image') {
      annotations = []
      annotationSaveError = null
      return
    }

    let cancelled = false

    void (async () => {
      try {
        annotationSaveError = null
        const loadedAnnotations = await getStore().annotations.findByAsset(asset.id, 1)
        if (!cancelled && selectedAsset?.id === asset.id) {
          annotations = loadedAnnotations.map((a) => ({
            ...a,
            kind: a.kind as ViewerAnnotationKind,
          }))
        }
      } catch {
        if (!cancelled) {
          annotations = []
          annotationSaveError = 'Failed to load annotations for this asset.'
        }
      }
    })()

    return () => {
      cancelled = true
    }
  })

  $effect(() => {
    window.dispatchEvent(
      new CustomEvent<DocumentExplorerAssetDetail>(DOCUMENT_EXPLORER_ASSET_SELECTED_EVENT, {
        detail: {
          itemId,
          assetId: selectedAsset?.id ?? null,
        },
      })
    )
  })

  $effect(() => {
    void reloadLayoutForAsset(selectedAsset)
  })

  $effect(() => {
    if (!visibleLayoutBlocks.some((block) => block.id === layoutSelectedBlockId)) {
      layoutSelectedBlockId = null
      layoutSelectedRegionId = null
    }

    if (!visibleLayoutBlocks.some((block) => block.id === layoutHoveredBlockId)) {
      layoutHoveredBlockId = null
      layoutHoveredRegionId = null
    }
  })

  // Reload asset-scoped data when the selected asset changes
  $effect(() => {
    const asset = selectedAsset
    if (!asset) return
    const requestToken = ++selectedAssetStateLoadToken

    leftPanelTab = 'document'
    rightPanelTab = 'notes'

    // Reload notes for this asset (plus item-level notes)
    void refreshNotesForAsset(asset)

    // Load existing extraction text for this asset
    const store = getStore()
    void store.extractions.findByAsset(asset.id).then((extraction) => {
      if (selectedAssetStateLoadToken === requestToken && isCurrentSelectedAsset(asset) && extraction) {
        ocrStore._updateState(asset.id, {
          status: 'done',
          progress: 100,
          textLength: extraction.textContent.length,
          method: extraction.method,
          textContent: extraction.textContent,
        })
        ocrTick++
      }
    })

    // Load existing transcription for audio assets
    if (asset.type === 'audio') {
      void store.transcriptions.findByAsset(asset.id).then((transcription) => {
        if (
          selectedAssetStateLoadToken === requestToken &&
          isCurrentSelectedAsset(asset) &&
          transcription
        ) {
          transcriptionStore._updateState(asset.id, {
            status: 'done',
            progress: 100,
            text: transcription.textContent,
            language: transcription.language ?? undefined,
            durationMs: transcription.durationMs ?? undefined,
            segmentsCount: transcription.segments
              ? TranscriptionRepo.parseSegments(transcription.segments).length
              : 0,
          })
          transcriptionTick++
        }
      })
    }
  })

  // Reload analysis data when the selected asset changes
  $effect(() => {
    const asset = selectedAsset
    if (!asset) return
    void reloadEntitiesAndGeoMarkers(asset)
    void loadTriples(asset)
    void loadSimilarAssets(asset)
    // Load persisted LLM results for this asset so previous
    // asset-level results (summarize, correct_ocr, etc.) are visible.
    llmStore.loadPersistedResults(asset.id, 'asset')
    const requestToken = ++llmSummaryLoadToken
    llmGetResult(asset.id, 'summarize', 'asset')
      .then((result) => {
        if (llmSummaryLoadToken === requestToken && isCurrentSelectedAsset(asset) && result) {
          summaryTexts.set(asset.id, result.result)
          summaryTick++
        }
      })
      .catch(() => {
        // Silently degrade — persisted summaries are optional
      })
  })

  $effect(() => {
    // Reload all data when navigating to a different item.
    // Reading itemId here ensures the effect re-runs when the prop changes.
    const _id = itemId
    void loadData()
  })

  onMount(() => {
    window.addEventListener(
      DOCUMENT_EXPLORER_ASSET_SELECT_REQUEST_EVENT,
      handleExplorerAssetSelectRequest
    )

    ocrStore
      .startListening((eventName, callback) =>
        listen(eventName, callback).then((unlisten) => {
          // Wrap unlisten to also trigger reactivity tick
          return () => {
            unlisten()
          }
        })
      )
      .then(() => {
        // Patch each event to also bump ocrTick for Svelte reactivity
        const origUpdate = ocrStore._updateState.bind(ocrStore)
        ocrStore._updateState = (assetId, partial) => {
          origUpdate(assetId, partial)
          ocrTick++
        }
      })

    nlpStore
      .startListening((eventName, callback) =>
        listen(eventName, callback).then((unlisten) => () => unlisten())
      )
      .then(() => {
        const origSet = nlpStore._setJobStatus.bind(nlpStore)
        nlpStore._setJobStatus = (id, job, status, err) => {
          origSet(id, job, status, err)
          nlpTick++
          // After NER completes, reload entities for the current context
          if (job === 'ner' && status === 'done' && id === itemId) {
            void reloadEntitiesAndGeoMarkers()
          }
          if (job === 'embed' && status === 'done' && id === itemId) {
            void reloadSelectedAssetPersistedState({ similarAssets: true })
          }
          if (job === 'triples' && status === 'done' && id === itemId) {
            void reloadSelectedAssetPersistedState({ triples: true })
          }
        }
      })

    transcriptionStore
      .startListening((eventName, callback) =>
        listen(eventName, callback).then((unlisten) => () => unlisten())
      )
      .then(() => {
        const origUpdate = transcriptionStore._updateState.bind(transcriptionStore)
        transcriptionStore._updateState = (assetId, partial) => {
          origUpdate(assetId, partial)
          transcriptionTick++
        }
      })

    llmStore.startListening().then(() => {
      llmStore.onChange(() => {
        llmTick++
      })
      // Load persisted LLM results for the item (legacy item-level results).
      // Asset-level results are loaded in the selectedAsset effect below.
      llmStore.loadPersistedResults(itemId, 'item')
    })

    llmIsAvailable()
      .then((available) => {
        llmAvailable = available
      })
      .catch(() => {
        llmAvailable = false
      })

    geoStore.startListening()
    return () => {
      if (metadataSaveTimer) clearTimeout(metadataSaveTimer)
    }
  })

  onDestroy(() => {
    layoutLoadToken++
    notesLoadToken++
    selectedAssetStateLoadToken++
    entitiesLoadToken++
    geoMarkersLoadToken++
    triplesLoadToken++
    similarAssetsLoadToken++
    llmSummaryLoadToken++
    window.removeEventListener(
      DOCUMENT_EXPLORER_ASSET_SELECT_REQUEST_EVENT,
      handleExplorerAssetSelectRequest
    )
    ocrStore.stopListening()
    nlpStore.stopListening()
    transcriptionStore.stopListening()
    llmStore.stopListening()
    geoStore.stopListening()
    // Clear any pending debounce timers to avoid stale persist after unmount
    for (const timer of ocrPersistTimers.values()) {
      clearTimeout(timer)
    }
    ocrPersistTimers.clear()
    for (const timer of transPersistTimers.values()) {
      clearTimeout(timer)
    }
    transPersistTimers.clear()
    for (const timer of assetReanalysisTimers.values()) {
      clearTimeout(timer)
    }
    assetReanalysisTimers.clear()
    clearAnnotationSaveTimer()
    clearFtsSearchTimer()
    if (dragCleanup) dragCleanup()
  })
</script>

{#if loading}
  <p class="status">{translate('item.loading')}</p>
{:else if error && !item}
  <p class="error">{error}</p>
{:else if item}
  <div
    class="item-view"
    bind:this={itemViewEl}
    style="grid-template-columns: 1fr auto {rightPanelOpen ? `6px ${sidebarWidth}%` : ''}"
  >
    <Panel variant="glass" padding="none" class="left-panel">
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
              showLayoutOverlay={showLayout && layoutRegions.length > 0}
              hoveredLayoutRegionId={layoutHoveredRegionId}
              selectedLayoutRegionId={layoutSelectedRegionId}
              {layoutReferenceWidth}
              {layoutReferenceHeight}
              {selectedAnnotationId}
              {annotationTool}
              {annotationColor}
              {editTool}
              {canUndo}
              currentPage={viewerPage}
              onAnnotationsChange={handleAnnotationsChange}
              onSelectedAnnotationIdChange={handleSelectedAnnotationIdChange}
              onAnnotationToolChange={handleAnnotationToolChange}
              onAnnotationColorChange={handleAnnotationColorChange}
              onLayoutRegionHoverChange={(regionId: string | null) => {
                syncLayoutHoverFromRegion(regionId)
              }}
              onLayoutRegionSelect={(regionId: string) => {
                setSelectedLayoutRegion(regionId)
              }}
              onEditSelect={handleEditSelect}
              onEditToolChange={(tool: EditTool) => {
                editTool = tool
                if (tool !== 'none') annotationTool = 'select'
              }}
              onRotateLeft={handleRotateLeft}
              onRotateRight={handleRotateRight}
              onUndo={handleUndo}
              onPageChange={(page: number, totalPages: number) => {
                viewerPage = page
                viewerTotalPages = totalPages
              }}
              onDimensionsChange={(dims: { width: number; height: number }) => {
                imageNaturalW = dims.width
                imageNaturalH = dims.height
              }}
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
              {@const ocr = getOcrState(selectedAsset.id)}
              <section class="left-text-panel-section">
                <div class="left-text-panel-card">
                  {#if (ocrEditedText.get(selectedAsset.id) ?? ocr.textContent ?? '').trim()}
                    <div class="left-text-panel-meta">
                      <span>{translate('item.extractedText')}</span>
                      <span class="ocr-meta"
                        >via {ocr.method ?? translate('item.ocrMethodUnknown')} · {translate(
                          'item.characters',
                          {
                            count: (ocrEditedText.get(selectedAsset.id) ?? ocr.textContent ?? '')
                              .length,
                          }
                        )}</span
                      >
                    </div>
                    <div class="left-text-panel-body">
                      {ocrEditedText.get(selectedAsset.id) ?? ocr.textContent ?? ''}
                    </div>
                  {:else}
                    <p class="empty-text">{translate('item.noExtractedText')}</p>
                  {/if}
                </div>
              </section>
            {:else}
              {@const ts = getTranscriptionState(selectedAsset.id)}
              <section class="left-text-panel-section">
                <div class="left-text-panel-card">
                  {#if (transEditedText.get(selectedAsset.id) ?? ts.text ?? '').trim()}
                    <div class="left-text-panel-meta">
                      <span>{translate('item.transcription')}</span>
                      <span class="ocr-meta">
                        {#if ts.language}{ts.language} &middot;
                        {/if}{translate('item.characters', {
                          count: (transEditedText.get(selectedAsset.id) ?? ts.text ?? '').length,
                        })}
                        {#if ts.durationMs}
                          &middot; {translate('item.audioDurationSeconds', {
                            count: Math.round(ts.durationMs / 1000),
                          })}{/if}
                      </span>
                    </div>
                    <div class="left-text-panel-body">
                      {transEditedText.get(selectedAsset.id) ?? ts.text ?? ''}
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

      {#if assets.length > 1}
        <div class="asset-pagination">
          <button
            class="pagination-btn"
            disabled={selectedAssetIndex <= 0}
            onclick={() => (selectedAssetIndex = Math.max(0, selectedAssetIndex - 1))}
            aria-label={translate('item.previousPage')}
          >
            <ActionIcon name="chevron-left" size={18} />
          </button>
          <span class="pagination-info">
            {selectedAssetIndex + 1} / {assets.length}
          </span>
          <button
            class="pagination-btn"
            disabled={selectedAssetIndex >= assets.length - 1}
            onclick={() =>
              (selectedAssetIndex = Math.min(assets.length - 1, selectedAssetIndex + 1))}
            aria-label={translate('item.nextPage')}
          >
            <ActionIcon name="chevron-right" size={18} />
          </button>
        </div>
      {/if}
    </Panel>

    <!-- Right panel toggle -->
    <IconButton
      class="right-panel-toggle"
      variant="ghost"
      size="sm"
      label={rightPanelOpen ? 'Ocultar panel derecho' : 'Mostrar panel derecho'}
      onclick={() => { rightPanelOpen = !rightPanelOpen }}
      title={rightPanelOpen ? 'Ocultar panel' : 'Mostrar panel'}
    >
      <ActionIcon name={rightPanelOpen ? 'chevron-right' : 'chevron-left'} size={14} />
    </IconButton>

    {#if rightPanelOpen}
    <div
      class="resize-handle"
      role="separator"
      aria-orientation="vertical"
      onpointerdown={onResizeHandlePointerDown}
    ></div>

    <Panel variant="default" padding="none" class="right-panel">
      <header class="item-header">
        <span class="item-header__eyebrow">{translate('item.activeDocument')}</span>
        <h2 class="item-title">{item.title}</h2>
        <p class="item-header__meta">{activeAssetSummary}</p>
      </header>

      {#if error}
        <p class="error">{error}</p>
      {/if}

      <TabList class="right-panel-tabs" aria-label={translate('item.rightPanel')}>
        <TabButton
          active={rightPanelTab === 'notes'}
          class="right-panel-tab"
          onclick={() => {
            rightPanelTab = 'notes'
          }}
        >
          {translate('item.notesTab')}
        </TabButton>
        <TabButton
          active={rightPanelTab === 'text'}
          class="right-panel-tab"
          onclick={() => {
            rightPanelTab = 'text'
          }}
        >
          {translate('item.textTab')}
        </TabButton>
        <TabButton
          active={rightPanelTab === 'analysis'}
          class="right-panel-tab"
          onclick={() => {
            rightPanelTab = 'analysis'
            reloadEntitiesAndGeoMarkers()
            loadTriples()
          }}
        >
          {translate('item.analysisTab')}
        </TabButton>
        <TabButton
          active={rightPanelTab === 'search'}
          class="right-panel-tab"
          onclick={() => {
            rightPanelTab = 'search'
            loadSimilarAssets()
            loadFtsStats()
          }}
        >
          {translate('item.searchTab')}
        </TabButton>
        <TabButton
          active={rightPanelTab === 'layout'}
          class="right-panel-tab"
          onclick={() => {
            rightPanelTab = 'layout'
          }}
        >
          {translate('item.layoutTab')}
        </TabButton>
        <TabButton
          active={rightPanelTab === 'metadata'}
          class="right-panel-tab"
          onclick={() => {
            rightPanelTab = 'metadata'
          }}
        >
          {translate('item.metadataTab')}
        </TabButton>
      </TabList>

      <div class="right-panel-content">
        <div class="right-panel-pane" class:is-hidden={rightPanelTab !== 'notes'}>
          <ItemNotesPanel
            {itemTopics}
            {topicSuggestions}
            assetsCount={assets.length}
            {selectedAssetIndex}
            {notes}
            {editingNoteId}
            {expandedNoteId}
            {pendingDeleteNoteId}
            {deletingNote}
            {noteEditorLabels}
            {translate}
            onTopicsChange={handleTopicsChange}
            onSaveNote={handleSaveNote}
            onTranscribeDictation={handleTranscribeDictation}
            onSaveEdit={handleSaveEdit}
            onCancelEdit={handleCancelEdit}
            onEditNote={handleEditNote}
            onOpenDeleteNoteConfirm={openDeleteNoteConfirm}
            onDeleteNoteCancel={handleDeleteNoteCancel}
            onDeleteNoteConfirm={handleDeleteNoteConfirm}
            onToggleNoteExpanded={toggleNoteExpanded}
          />
        </div>

        <div class="right-panel-pane" class:is-hidden={rightPanelTab !== 'metadata'}>
          <ItemMetadataPanel
            {savingMetadata}
            {fileMetadataEntries}
            {metadataValue}
            {metadataEditorLabels}
            {translate}
            onMetadataChange={handleMetadataChange}
          />
        </div>

        <div class="right-panel-pane" class:is-hidden={rightPanelTab !== 'layout'}>
          <ItemLayoutPanel
            selectedAssetType={selectedAsset?.type ?? null}
            {viewerType}
            {assetLayout}
            {layoutLoading}
            {layoutError}
            {showLayout}
            {layoutActivePage}
            {layoutBlockCountsByPage}
            {layoutBlocks}
            layoutPageRegionCount={layoutPageRegions.length}
            layoutRegionCount={assetLayout?.regions.length ?? 0}
            {layoutPageOptions}
            {layoutTypeFilter}
            {layoutFilterLabels}
            {layoutFilterCounts}
            {layoutPageBlocks}
            {visibleLayoutBlocks}
            {layoutHoveredBlockId}
            {layoutSelectedBlockId}
            {selectedLayoutBlock}
            {hasLayoutData}
            {translate}
            onToggleLayout={(nextShowLayout) => {
              showLayout = nextShowLayout
            }}
            onPageChange={(page) => {
              viewerPage = page
            }}
            onFilterChange={(filter) => {
              layoutTypeFilter = filter
            }}
            onHoverBlock={syncLayoutHoverFromBlock}
            onSelectBlock={setSelectedLayoutBlock}
          />
        </div>

        <div class="right-panel-pane" class:is-hidden={rightPanelTab !== 'text'}>
          {#if selectedAsset && selectedAsset.type !== 'audio'}
            {@const ocr = getOcrState(selectedAsset.id)}
            {@const busy = ocr.status === 'pending' || ocr.status === 'running'}
            {@const isPdfAsset = selectedAsset.type === 'pdf'}
            <section class="section">
              <h3>
                {translate('item.textExtraction')}{#if assets.length > 1}
                  {translate('item.pageInline', { page: selectedAssetIndex + 1 })}{/if}
              </h3>
              <div class="ocr-item">
                <div class="ocr-item-header">
                  <span class="ocr-filename">
                    {assets.length > 1 && assets.every((a) => a.type === 'image')
                      ? translate('item.assetPageLabel', { page: selectedAssetIndex + 1 })
                      : (selectedAsset.path.split(/[/\\]/).pop() ??
                        translate('item.assetNoSelection'))}
                  </span>
                  <StatusBadge
                    variant={getJobStatusBadgeVariant(ocr.status)}
                    size="sm"
                    class="ocr-status-badge"
                  >
                    {ocr.status}
                  </StatusBadge>
                  <div class="ocr-btn-group">
                    {#if isPdfAsset}
                      <button
                        class="ocr-btn ocr-btn--light"
                        disabled={busy}
                        onclick={() => handleExtractText(selectedAsset, 'light')}
                        title={busy
                          ? translate('item.pdfTextBusyTitle')
                          : translate('item.pdfTextTitle')}
                      >
                        {getExtractionPrimaryActionLabel(selectedAsset.type)}
                      </button>
                    {:else}
                      <button
                        class="ocr-btn ocr-btn--high"
                        disabled={busy}
                        onclick={() => handleExtractText(selectedAsset, 'high')}
                        title={busy
                          ? translate('item.ocrHighBusyTitle')
                          : translate('item.ocrHighTitle')}
                      >
                        {translate('item.ocrHighAction')}
                      </button>
                    {/if}
                    {#if llmAvailable && !ocrCorrectedAssets.has(selectedAsset.id)}
                      <button
                        class="ocr-btn ocr-btn--correct"
                        disabled={getLlmState().status === 'running' || ocr.status !== 'done'}
                        onclick={handleLlmCorrectOcr}
                        title={!llmAvailable
                          ? translate('item.ocrCorrectUnavailable')
                          : ocr.status !== 'done'
                            ? translate('item.ocrCorrectNeedsText')
                            : isPdfAsset
                              ? translate('item.pdfCorrectTitle')
                              : translate('item.ocrCorrectTitle')}
                      >
                        {getCorrectionActionLabel(selectedAsset.type)}
                      </button>
                    {/if}
                    {#if llmAvailable}
                      <button
                        class="ocr-btn ocr-btn--summarize"
                        disabled={getLlmState().status === 'running' || ocr.status !== 'done'}
                        onclick={handleLlmSummarize}
                        title={!llmAvailable
                          ? translate('item.summaryUnavailable')
                          : ocr.status !== 'done'
                            ? translate('item.summaryNeedsText')
                            : translate('item.summaryTitle')}
                      >
                        {getSummaryActionLabel(selectedAsset.type)}
                      </button>
                    {/if}
                  </div>
                  {#if !llmAvailable}
                    <p class="ocr-llm-hint">{translate('item.llmUnavailableHint')}</p>
                  {/if}
                </div>

                {#if ocr.status === 'running'}
                  {@const ocrStageLabel = getOcrStageLabel(ocr.stage)}
                  <progress class="ocr-progress" value={ocr.progress} max="100">
                    {ocr.progress}%
                  </progress>
                  <p class="ocr-status-text">
                    {ocrStageLabel
                      ? translate('item.extractionRunningStage', {
                          progress: ocr.progress,
                          stage: ocrStageLabel,
                        })
                      : translate('item.extractionRunning', { progress: ocr.progress })}
                  </p>
                {:else if ocr.status === 'pending'}
                  <p class="ocr-status-text">{translate('item.extractionStarting')}</p>
                {:else if ocr.status === 'error'}
                  <p class="ocr-error">
                    {translate('item.extractionFailed', { error: ocr.error ?? '' })}
                  </p>
                {:else if ocr.status === 'done'}
                  {@const editedText = (() => {
                    void ocrTick
                    return ocrEditedText.get(selectedAsset.id) ?? ocr.textContent ?? ''
                  })()}
                  {@const displayLength = editedText.length}
                  <details class="ocr-result">
                    <summary>
                      {translate('item.extractedText')}
                      <span class="ocr-meta">
                        via {ocr.method ?? translate('item.ocrMethodUnknown')} · {translate(
                          'item.characters',
                          { count: displayLength }
                        )}
                      </span>
                    </summary>
                    <textarea
                      class="ocr-result-body ocr-textarea"
                      rows="8"
                      oninput={(e) => {
                        const val = e.currentTarget.value
                        ocrEditedText.set(selectedAsset.id, val)
                        ocrStore.setTextContent(selectedAsset.id, val)
                        schedulePersist(selectedAsset.id, val)
                        ocrTick++
                      }}>{editedText}</textarea
                    >
                  </details>
                {/if}
              </div>
            </section>
          {/if}

          {#if selectedAsset && selectedAsset.type === 'audio'}
            {@const ts = getTranscriptionState(selectedAsset.id)}
            {@const busy = ts.status === 'pending' || ts.status === 'running'}
            <section class="section">
              <h3>
                {translate('item.audioTranscription')}{#if assets.length > 1}
                  {translate('item.pageInline', { page: selectedAssetIndex + 1 })}{/if}
              </h3>
              <div class="ocr-item">
                <div class="ocr-item-header">
                  <span class="ocr-filename"
                    ><ActionIcon name="volume" size={16} />
                    <span class="ocr-filename__text">
                      {selectedAsset.path.split(/[/\\]/).pop() ?? translate('item.audioLabel')}
                    </span></span
                  >
                  <StatusBadge
                    variant={getJobStatusBadgeVariant(ts.status)}
                    size="sm"
                    class="ocr-status-badge"
                  >
                    {ts.status}
                  </StatusBadge>
                  <div class="ocr-btn-group">
                    <button
                      class="ocr-btn"
                      disabled={busy}
                      onclick={() => handleTranscribeAudio(selectedAsset)}
                      title={busy
                        ? translate('item.transcribeBusyTitle')
                        : translate('item.transcribeTitle')}
                    >
                      {getTranscriptionActionLabel(busy)}
                    </button>
                    {#if llmAvailable}
                      <button
                        class="ocr-btn ocr-btn--summarize"
                        disabled={getLlmState().status === 'running' || ts.status !== 'done'}
                        onclick={handleLlmSummarize}
                        title={!llmAvailable
                          ? translate('item.summaryUnavailable')
                          : ts.status !== 'done'
                            ? translate('item.summaryNeedsText')
                            : translate('item.summaryTitle')}
                      >
                        {getSummaryActionLabel(selectedAsset.type)}
                      </button>
                    {/if}
                  </div>
                </div>

                {#if ts.status === 'running'}
                  <progress class="ocr-progress" value={ts.progress} max="100">
                    {ts.progress}%
                  </progress>
                  <p class="ocr-status-text">
                    {translate('item.transcriptionRunning', { progress: ts.progress })}
                    {#if getTranscriptionStageLabel(ts.stage)}
                      · {getTranscriptionStageLabel(ts.stage)}
                    {/if}
                  </p>
                {:else if ts.status === 'pending'}
                  <p class="ocr-status-text">{translate('item.transcriptionStarting')}</p>
                {:else if ts.status === 'error'}
                  <p class="ocr-error">
                    {translate('item.transcriptionFailed', { error: ts.error ?? '' })}
                  </p>
                {:else if ts.status === 'done'}
                  {@const editedText = transEditedText.get(selectedAsset.id) ?? ts.text ?? ''}
                  {@const displayLength = editedText.length}
                  <details class="ocr-result">
                    <summary>
                      {translate('item.transcription')}
                      <span class="ocr-meta">
                        {#if ts.language}{ts.language} &middot;
                        {/if}{translate('item.characters', { count: displayLength })}
                        {#if ts.durationMs}
                          &middot; {translate('item.audioDurationSeconds', {
                            count: Math.round(ts.durationMs / 1000),
                          })}{/if}
                      </span>
                    </summary>
                    <textarea
                      class="ocr-result-body ocr-textarea"
                      rows="8"
                      oninput={(e) => {
                        const val = e.currentTarget.value
                        transEditedText.set(selectedAsset.id, val)
                        transcriptionStore.setTextContent(selectedAsset.id, val)
                        scheduleTranscriptionPersist(selectedAsset.id, val)
                        transcriptionTick++
                      }}>{editedText}</textarea
                    >
                  </details>
                {/if}
              </div>
            </section>
          {/if}

          {#if selectedAsset}
            {@const currentSummary = (() => {
              void summaryTick
              return summaryTexts.get(selectedAsset.id) ?? null
            })()}
            {@const isSummarizing =
              getLlmState().status === 'running' && getLlmState().activeJob === 'summarize'}
            {#if currentSummary || isSummarizing}
              <section class="section">
                <h3>
                  {translate('item.summary')}{#if assets.length > 1}
                    {translate('item.pageInline', { page: selectedAssetIndex + 1 })}{/if}
                </h3>
                {#if isSummarizing}
                  <p class="summary-status">{translate('item.generatingSummary')}</p>
                {:else if currentSummary}
                  <div class="summary-result">
                    <pre class="summary-text">{currentSummary}</pre>
                  </div>
                {/if}
              </section>
            {/if}
          {/if}
        </div>

        <div class="right-panel-pane" class:is-hidden={rightPanelTab !== 'analysis'}>
          {#if assets.length > 0}
            {@const nlp = getNlpState()}
            <section class="section">
              <div class="analysis-panel analysis-panel--tabbed">
                <div class="nlp-actions">
                  <button
                    class="nlp-btn"
                    disabled={nlp.fts === 'pending' || nlp.fts === 'running'}
                    onclick={handleIndexFts}
                  >
                    {translate('item.indexAction')}
                    <StatusBadge variant={getJobStatusBadgeVariant(nlp.fts)} size="sm" class="nlp-badge">{nlp.fts}</StatusBadge>
                  </button>

                  <button
                    class="nlp-btn"
                    disabled={!selectedAsset || nlp.embed === 'pending' || nlp.embed === 'running'}
                    onclick={handleEmbedAsset}
                  >
                    {translate('item.embedAction')}
                    <StatusBadge variant={getJobStatusBadgeVariant(nlp.embed)} size="sm" class="nlp-badge">{nlp.embed}</StatusBadge>
                  </button>

                  <button
                    class="nlp-btn"
                    disabled={nlp.ner === 'pending' || nlp.ner === 'running'}
                    onclick={handleExtractEntities}
                  >
                    {translate('item.nerAction')}
                    <StatusBadge variant={getJobStatusBadgeVariant(nlp.ner)} size="sm" class="nlp-badge">{nlp.ner}</StatusBadge>
                  </button>

                  <button
                    class="nlp-btn"
                    disabled={!llmAvailable ||
                      nlp.triples === 'pending' ||
                      nlp.triples === 'running'}
                    onclick={handleLlmExtractTriples}
                  >
                    {translate('item.triplesAction')}
                    <StatusBadge variant={getJobStatusBadgeVariant(nlp.triples)} size="sm" class="nlp-badge">{nlp.triples}</StatusBadge>
                  </button>
                </div>

                {#if nlp.errors?.embed}
                  <p class="ocr-error">
                    {translate('item.embeddingError', { error: nlp.errors.embed })}
                  </p>
                {/if}

                {#if !selectedAsset}
                  <p class="empty-text">
                    {translate('item.analysisNeedAsset')}
                  </p>
                {/if}

                <div class="geo-section">
                  <MapViewer
                    markers={geoMarkers}
                    height="280px"
                    visible={rightPanelTab === 'analysis'}
                  />
                </div>

                <div class="entities-section">
                  <h4>{translate('item.entities')}</h4>
                  <EntityViewer
                    {entities}
                    {editingEntityId}
                    editingValue={editingEntityValue}
                    labels={{
                      editValueAria: translate('item.entityEditValueAria'),
                      deleteEntityAria: (value: string) =>
                        translate('item.entityDeleteAria', { value }),
                    }}
                    onentityclick={startEditingEntity}
                    oneditvaluechange={handleEditingEntityValueChange}
                    onsaveentity={handleSaveEntity}
                    oncancelentityedit={cancelEditingEntity}
                    ondeleteentity={handleDeleteEntity}
                  />

                  <div class="entity-editor">
                    <h5>{translate('item.manualEntities')}</h5>
                    <p class="entity-editor__hint">
                      {translate('item.entityHint')}
                    </p>

                    <div class="entity-editor__create">
                      <select
                        value={newEntityType}
                        aria-label={translate('item.newEntityType')}
                        onchange={(event) => {
                          newEntityType = event.currentTarget.value as EditableEntityType
                        }}
                      >
                        {#each EDITABLE_ENTITY_TYPES as type}
                          <option value={type}>{type.toUpperCase()}</option>
                        {/each}
                      </select>
                      <input
                        bind:value={newEntityValue}
                        type="text"
                        placeholder={translate('item.newEntityValue')}
                        aria-label={translate('item.newEntityValue')}
                        onkeydown={(event) => event.key === 'Enter' && void handleCreateEntity()}
                      />
                      <button type="button" class="nlp-btn" onclick={handleCreateEntity}
                        >{translate('item.addEntity')}</button
                      >
                    </div>

                    {#if entityActionError}
                      <p class="error">{entityActionError}</p>
                    {/if}
                  </div>
                </div>

                <div class="triples-section">
                  <h4>
                    {translate('item.semanticTriples')}{#if assets.length > 1}
                      {translate('item.pageInline', { page: selectedAssetIndex + 1 })}{/if}
                  </h4>
                  {#if triples.length === 0}
                    <p class="empty-text">
                      {translate('item.noTriples', {
                        suffix: assets.length > 1 ? translate('item.noTriplesPageSuffix') : '',
                      })}
                    </p>
                  {:else}
                    <ul class="triples-list">
                      {#each triples as triple, i (`${triple.subject}-${triple.predicate}-${triple.object}-${i}`)}
                        <li class="triple-item">
                          <span class="triple-cell">{triple.subject}</span>
                          <span class="triple-cell">{triple.predicate}</span>
                          <span class="triple-cell">{triple.object}</span>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              </div>
            </section>
          {/if}
        </div>

        <div class="right-panel-pane" class:is-hidden={rightPanelTab !== 'search'}>
          <ItemSearchPanel
            assetsCount={assets.length}
            selectedAsset={Boolean(selectedAsset)}
            {selectedAssetIndex}
            {ftsQuery}
            {ftsResults}
            {ftsSearching}
            {ftsSearchError}
            {ftsIndexedRows}
            {ftsDebug}
            {similarAssets}
            {isDev}
            {translate}
            onFtsInput={handleFtsInput}
            onFtsKeydown={handleFtsKeydown}
            onNavigateToSimilarItem={navigateToSimilarItem}
          />
        </div>
      </div>
    </Panel>
    {/if}
  </div>
{/if}

<style>
  .item-view {
    display: grid;
    /* grid-template-columns set via inline style */
    gap: var(--space-3);
    height: 100%;
    min-height: 0;
    padding: var(--space-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-dialog);
    background: var(--surface-app);
  }
  :global(.left-panel) {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    overflow-y: auto;
    padding: var(--space-2);
    min-height: 0;
  }
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
  :global(.right-panel) {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    overflow: hidden;
    padding: 0;
    min-height: 0;
  }
  :global(.right-panel-tabs) {
    display: flex;
    flex-wrap: wrap;
    align-self: stretch;
    margin: 0 var(--space-3);
    background: var(--surface-input);
    border-color: var(--border-subtle);
  }
  :global(.right-panel-tab) {
    flex: 1 1 auto;
    min-width: fit-content;
  }
  .right-panel-content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    margin: 0 var(--space-3) var(--space-3);
    background: var(--surface-input);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-surface);
  }
  .right-panel-pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: 100%;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-2);
  }
  .right-panel-pane.is-hidden {
    display: none;
  }
  .analysis-panel--tabbed {
    border-top: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }
  .item-header {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .item-header__eyebrow {
    font-family: var(--font-mono);
    font-size: 0.6rem;
    font-weight: var(--font-weight-normal);
    letter-spacing: 0.15em;
    text-transform: uppercase;
    color: var(--color-text-muted);
  }
  :global(.icon-button.right-panel-toggle) {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: auto;
    flex-shrink: 0;
    border-radius: var(--radius-dialog);
    background: var(--surface-input);
    border: 1px solid var(--border-subtle);
    color: var(--color-text-muted);
    cursor: pointer;
  }
  :global(.icon-button.right-panel-toggle:hover) {
    color: var(--color-accent);
    background: var(--color-accent-soft);
  }
  .resize-handle {
    width: 6px;
    position: relative;
    cursor: col-resize;
    z-index: 1;
  }
  .resize-handle::before {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    transform: translateX(-50%);
    width: 1px;
    background-color: var(--color-border);
    transition:
      background-color 0.15s ease,
      width 0.15s ease;
  }
  .resize-handle:hover::before {
    background-color: var(--color-text-muted, var(--color-border));
    width: 2px;
  }
  :global(body.no-select),
  :global(body.no-select *) {
    cursor: col-resize !important;
    user-select: none !important;
    -webkit-user-select: none !important;
  }
  .item-title {
    font-family: var(--font-display);
    font-size: var(--font-size-md);
    font-weight: var(--font-weight-bold);
    color: var(--color-text-primary);
  }
  .item-header__meta {
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
  }
  .section h3 {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    color: var(--color-text-secondary);
    margin-bottom: var(--space-1);
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2);
    border: 1px solid var(--color-hairline);
    border-radius: var(--radius-surface);
    background: var(--color-surface);
    box-shadow: var(--shadow-surface);
  }
  .asset-pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    padding: var(--space-2) 0;
  }
  .pagination-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--color-hairline);
    border-radius: var(--radius-sm);
    background: var(--color-surface-sunken);
    color: var(--color-text-primary);
    font-size: var(--font-size-md);
    cursor: pointer;
    transition:
      background var(--transition-smooth),
      border-color var(--transition-smooth);
  }
  .pagination-btn:hover:not(:disabled) {
    border-color: var(--color-accent);
    background: var(--color-primary-subtle);
  }
  .pagination-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .pagination-info {
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    min-width: 60px;
    text-align: center;
    font-variant-numeric: tabular-nums;
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

  /* ── Summary (auto-generated by Gemma 4) ── */
  .summary-result {
    margin-top: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--color-hairline);
    border-radius: var(--radius-sm);
    background: var(--color-surface-sunken);
  }

  .summary-status {
    font-size: var(--font-size-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .summary-text {
    margin: 0;
    font-size: var(--font-size-sm);
    font-family: var(--font-sans);
    white-space: pre-wrap;
    word-wrap: break-word;
    max-height: 300px;
    overflow-y: auto;
    line-height: 1.6;
    color: var(--color-text-secondary);
  }
  .empty-text {
    color: var(--color-text-secondary);
    font-size: var(--font-size-sm);
  }
  .status {
    color: var(--color-text-secondary);
    text-align: center;
  }
  .error {
    color: var(--color-danger);
  }

  /* ── OCR UI ── */
  .ocr-item {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-surface);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    background: var(--surface-card);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025);
  }
  .ocr-item-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }
  .ocr-filename {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }
  .ocr-filename :global(svg) {
    flex-shrink: 0;
  }
  .ocr-filename__text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  :global(.ocr-status-badge) {
    flex-shrink: 0;
    text-transform: uppercase;
  }
  .ocr-btn {
    padding: var(--space-1) var(--space-3);
    font-size: var(--font-size-xs);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-control);
    background: var(--surface-card);
    color: var(--color-text-primary);
    cursor: pointer;
    white-space: nowrap;
    flex-shrink: 0;
    font-family: var(--font-sans);
    transition:
      background-color var(--transition-base),
      border-color var(--transition-base),
      box-shadow var(--transition-base),
      color var(--transition-base);
  }
  .ocr-btn:hover:not(:disabled) {
    border-color: var(--border-panel);
    background: var(--color-accent-faint);
  }
  .ocr-btn:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }
  .ocr-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
    border-color: var(--border-subtle);
    background: var(--surface-input);
    color: var(--color-text-muted);
  }
  .ocr-btn-group {
    display: flex;
    gap: var(--space-1);
    flex-shrink: 0;
  }
  .ocr-btn--light {
    border-color: var(--color-success);
    background: var(--color-success-soft);
    color: var(--color-success);
  }
  .ocr-btn--light:disabled {
    border-color: var(--border-subtle);
    background: var(--surface-input);
    color: var(--color-text-muted);
  }
  .ocr-btn--high {
    border-color: var(--color-info);
    background: var(--color-info-soft);
    color: var(--color-info);
  }
  .ocr-btn--high:disabled {
    border-color: var(--border-subtle);
    background: var(--surface-input);
    color: var(--color-text-muted);
  }
  .ocr-btn--correct {
    border-color: var(--color-accent);
    background: var(--color-accent-faint);
    color: var(--color-accent);
  }
  .ocr-btn--correct:disabled {
    border-color: var(--border-subtle);
    background: var(--surface-input);
    color: var(--color-text-muted);
  }
  .ocr-btn--summarize {
    border-color: var(--color-warning);
    background: var(--color-warning-soft);
    color: var(--color-warning);
  }
  .ocr-btn--summarize:disabled {
    border-color: var(--border-subtle);
    background: var(--surface-input);
    color: var(--color-text-muted);
  }
  .ocr-llm-hint {
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
    margin: var(--space-1) 0 0;
    font-style: italic;
  }
  .ocr-progress {
    width: 100%;
    height: 6px;
    border-radius: var(--radius-sm);
    appearance: none;
  }
  .ocr-status-text {
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
  }
  .ocr-error {
    font-size: var(--font-size-xs);
    color: var(--color-danger);
  }
  .ocr-meta {
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
  }
  .ocr-result {
    font-size: var(--font-size-sm);
  }
  .ocr-result summary {
    cursor: pointer;
    color: var(--color-text-secondary);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) 0;
  }
  .ocr-result-body {
    margin-top: var(--space-1);
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .ocr-textarea {
    width: 100%;
    min-height: 7rem;
    padding: var(--space-1) var(--space-2);
    font-family: var(--font-mono, ui-monospace, SFMono-Regular, monospace);
    font-size: var(--font-size-sm);
    line-height: 1.5;
    color: var(--color-text-secondary);
    background: var(--surface-input);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    resize: vertical;
    white-space: pre-wrap;
    word-break: break-word;
    outline: none;
    transition:
      border-color var(--transition-base),
      box-shadow var(--transition-base);
  }
  .ocr-textarea:focus {
    border-color: var(--border-focus);
    box-shadow: var(--focus-ring);
  }
  .ocr-textarea:hover:not(:focus) {
    border-color: var(--border-panel);
  }

  /* ── Analysis Panel ── */
  .analysis-panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-2);
    border: 1px solid var(--border-subtle);
    border-top: none;
    border-radius: 0 0 var(--radius-md) var(--radius-md);
    overflow: hidden;
    background: var(--surface-card);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025);
  }

  .nlp-actions {
    display: flex;
    flex-direction: row;
    gap: var(--space-1);
  }

  .nlp-btn {
    display: inline-flex;
    flex-direction: row;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    flex: 1 1 25%;
    min-width: 0;
    padding: 6px var(--space-1);
    font-size: var(--font-size-xs);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--surface-card);
    cursor: pointer;
    color: var(--color-text-primary);
    font-family: var(--font-sans);
    text-align: center;
    white-space: nowrap;
  }

  .nlp-btn:hover:not(:disabled) {
    border-color: var(--border-panel);
    background: var(--color-accent-faint);
  }

  .nlp-btn:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .nlp-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(.nlp-badge) {
    font-size: 10px;
    text-transform: uppercase;
  }

  .entities-section,
  .triples-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .entities-section h4 {
    font-size: var(--font-size-sm);
    font-weight: var(--font-weight-medium);
    color: var(--color-text-secondary);
  }

  .entity-editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-top: var(--space-3);
    min-width: 0;
  }

  .entity-editor h5 {
    margin: 0;
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .entity-editor__hint {
    margin: 0;
    font-size: var(--font-size-xs);
    color: var(--color-text-muted);
  }

  .entity-editor__create {
    display: grid;
    grid-template-columns: 35fr 50fr 15fr;
    gap: var(--space-2);
    align-items: center;
    padding-bottom: var(--space-2);
    min-width: 0;
  }

  .entity-editor__create select {
    min-width: 0;
    padding: var(--space-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--surface-input);
    color: var(--color-text-primary);
    font-size: var(--font-size-xs);
  }

  .entity-editor__create input {
    min-width: 0;
    padding: var(--space-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--surface-input);
    color: var(--color-text-primary);
    font-size: var(--font-size-sm);
  }

  .entity-editor__create select:focus-visible,
  .entity-editor__create input:focus-visible {
    outline: none;
    border-color: var(--border-focus);
    box-shadow: var(--focus-ring);
  }

  .entity-editor__create .nlp-btn {
    width: 100%;
    flex-direction: row;
    justify-content: center;
    font-size: var(--font-size-sm);
    padding: var(--space-2) var(--space-3);
  }

  .triples-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .triple-item {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--color-hairline);
    border-radius: var(--radius-sm);
    background: var(--color-surface-raised);
  }

  .triple-cell {
    font-size: var(--font-size-xs);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Geo Section */
  .geo-section {
    margin-top: var(--space-4);
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-hairline);
  }

</style>
