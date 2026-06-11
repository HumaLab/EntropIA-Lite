<script lang="ts">
  import { getStore } from '$lib/db'
  import { navigation } from '$lib/navigation'
  import { locale, t } from '$lib/i18n'
  import {
    pickFiles,
    classifyFiles,
    importSingleFile,
    isScannedPdf,
    renderPdfPages,
    type ImportedFile,
  } from '$lib/file-import'
  import {
    getAssetUrl,
    generateImageThumbnail,
    deleteAssetFile,
    deleteImageThumbnail,
    deletePdfThumbnail,
  } from '$lib/file-import'
  import { appDataDir, join } from '@tauri-apps/api/path'
  import { stat } from '@tauri-apps/plugin-fs'
  import { exportCollectionById } from '$lib/export'
  import {
    DOCUMENT_EXPLORER_COLLECTION_CHANGED_EVENT,
    type DocumentExplorerCollectionChangedDetail,
  } from '$lib/document-explorer'
  import { ConfirmDialog, ItemCard, SearchBar, Button } from '@entropia/ui'
  import { onMount, onDestroy } from 'svelte'
  import { getCurrentWebview, type DragDropEvent } from '@tauri-apps/api/webview'
  import { listen, type UnlistenFn } from '@tauri-apps/api/event'
  import type { Item, Asset, CollectionItemCardSummary } from '@entropia/store'

  let { collectionId }: { collectionId: string } = $props()

  let items = $state<Item[]>([])
  let searchQuery = $state('')
  let loading = $state(true)
  let error = $state<string | null>(null)
  let importing = $state(false)
  let exporting = $state(false)
  type ImportSummary = {
    imported: number
    skipped: number
    errors: string[]
    rejected: string[]
    lastItemTitle: string | null
  }
  let importSummary = $state<ImportSummary | null>(null)
  let dragActive = $state(false)
  let unlistenDragDrop: (() => void) | null = null
  let unlistenAssetUpdate: (() => void) | null = null
  const currentLocale = locale
  let itemsLoadRequestId = 0
  let itemAssetsLoadRequestId = 0
  let imageThumbnailLoadRequestId = 0
  let activeCollectionId: string | null = null
  const IMAGE_THUMBNAIL_CONCURRENCY = 4

  type ItemAssetMeta = {
    assetCount: number
    thumbnailUrl: string | null
    primaryAssetId: string | null
    primaryAssetPath: string | null
    primaryAssetType: string | null
  }

  let visibleCountLabel = $derived.by(() => {
    $currentLocale
    return items.length === 1
      ? t('collection.visibleCount.one', { count: items.length })
      : t('collection.visibleCount.other', { count: items.length })
  })

  let collectionTitle = $derived.by(() => {
    $currentLocale
    return navigation.current.name === 'collection'
      ? navigation.current.collectionName
      : t('collection.documentsFallback')
  })

  // Cache itemId → { assetCount, thumbnailUrl, primaryAssetId, primaryAssetPath, primaryAssetType }
  let itemAssetMeta = $state<Map<string, ItemAssetMeta>>(new Map())

  // Delete confirmation state
  let showDeleteConfirm = $state(false)
  let pendingDeleteAssetId = $state<string | null>(null)
  let pendingDeleteItemId = $state<string | null>(null)
  let pendingDeleteFilename = $state<string | null>(null)
  let deleting = $state(false)
  let deleteError = $state<string | null>(null)

  function getItemAssetMeta(itemId: string): ItemAssetMeta {
    return (
      itemAssetMeta.get(itemId) ?? {
        assetCount: 0,
        thumbnailUrl: null,
        primaryAssetId: null,
        primaryAssetPath: null,
        primaryAssetType: null,
      }
    )
  }

  function buildMetaFromSummary(summary: CollectionItemCardSummary): ItemAssetMeta {
    return {
      assetCount: summary.assetCount,
      thumbnailUrl: null,
      primaryAssetId: summary.primaryAssetId,
      primaryAssetPath: summary.primaryAssetPath,
      primaryAssetType: summary.primaryAssetType,
    }
  }

  function applySummaries(summaries: CollectionItemCardSummary[]) {
    items = summaries.map(
      ({ assetCount, primaryAssetId, primaryAssetPath, primaryAssetType, ...item }) => item
    )

    const newMeta = new Map<string, ItemAssetMeta>()
    for (const summary of summaries) {
      newMeta.set(summary.id, buildMetaFromSummary(summary))
    }
    itemAssetMeta = newMeta
  }

  async function loadImageThumbnails(summaries: CollectionItemCardSummary[]) {
    const requestId = ++imageThumbnailLoadRequestId
    const imageSummaries = summaries.filter(
      (summary) =>
        summary.primaryAssetType === 'image' &&
        summary.primaryAssetId &&
        summary.primaryAssetPath
    )

    for (let i = 0; i < imageSummaries.length; i += IMAGE_THUMBNAIL_CONCURRENCY) {
      const chunk = imageSummaries.slice(i, i + IMAGE_THUMBNAIL_CONCURRENCY)
      const thumbnailResults = await Promise.all(
        chunk.map(async (summary) => {
          try {
            const thumbnailUrl = await generateImageThumbnail(
              summary.primaryAssetPath!,
              summary.primaryAssetId!
            )
            return { summary, thumbnailUrl }
          } catch (e) {
            console.warn('[CollectionView] Failed to generate image thumbnail for item', summary.id, e)
            return null
          }
        })
      )

      if (requestId !== imageThumbnailLoadRequestId) return

      const newMeta = new Map(itemAssetMeta)
      let changed = false
      for (const result of thumbnailResults) {
        if (!result) continue

        const currentMeta = newMeta.get(result.summary.id)
        if (!currentMeta || currentMeta.primaryAssetPath !== result.summary.primaryAssetPath) continue

        newMeta.set(result.summary.id, { ...currentMeta, thumbnailUrl: result.thumbnailUrl })
        changed = true
      }

      if (changed) itemAssetMeta = newMeta
    }
  }

  async function refreshItemAssetMeta(itemIds: string[]) {
    const requestId = ++itemAssetsLoadRequestId
    if (itemIds.length === 0) return
    const store = getStore()
    const newMeta = new Map(itemAssetMeta)
    for (const itemId of itemIds) {
      try {
        const assets: Asset[] = await store.assets.findByItem(itemId)
        if (requestId !== itemAssetsLoadRequestId) return
        const imageAsset = assets.find((a) => a.type === 'image')
        // For PDFs, keep exploration lightweight: ItemCard shows the PDF icon.
        const pdfAsset = assets.find((a) => a.type === 'pdf')

        let thumbnailUrl: string | null = null
        let primaryAssetType: string | null = null

        if (imageAsset) {
          thumbnailUrl = await generateImageThumbnail(imageAsset.path, imageAsset.id)
          primaryAssetType = imageAsset.type
        } else if (pdfAsset) {
          thumbnailUrl = null
          primaryAssetType = pdfAsset.type
        } else {
          const thumbAsset = assets[0]
          const isAudio = thumbAsset?.type === 'audio'
          thumbnailUrl = !isAudio && thumbAsset ? getAssetUrl(thumbAsset.path) : null
          primaryAssetType = thumbAsset?.type ?? null
        }

        newMeta.set(itemId, {
          assetCount: assets.length,
          thumbnailUrl,
          primaryAssetId: imageAsset?.id ?? pdfAsset?.id ?? assets[0]?.id ?? null,
          primaryAssetPath: imageAsset?.path ?? pdfAsset?.path ?? assets[0]?.path ?? null,
          primaryAssetType,
        })
      } catch (e) {
        console.error('[CollectionView] Failed to load assets for item', itemId, e)
        // Non-fatal: item card shows placeholder
      }
    }
    if (requestId !== itemAssetsLoadRequestId) return
    itemAssetMeta = newMeta
  }

  // Search filtering is delegated to the repo call in loadItems(); there is
  // no client-side filtering of the loaded items.
  async function loadItems() {
    const requestId = ++itemsLoadRequestId
    try {
      loading = true
      error = null
      const store = getStore()
      const loadedSummaries = store.items.findCardSummariesByCollection
        ? await store.items.findCardSummariesByCollection(collectionId, searchQuery)
        : null
      const loadedItems = loadedSummaries
        ? []
        : searchQuery
          ? await store.items.searchByText(collectionId, searchQuery)
          : await store.items.findByCollection(collectionId)
      if (requestId !== itemsLoadRequestId) return
      if (loadedSummaries) {
        applySummaries(loadedSummaries)
        void loadImageThumbnails(loadedSummaries)
      } else {
        items = loadedItems
        await refreshItemAssetMeta(items.map((i) => i.id))
      }
    } catch (e) {
      if (requestId !== itemsLoadRequestId) return
      error = e instanceof Error ? e.message : t('collection.error.load')
    } finally {
      if (requestId === itemsLoadRequestId) {
        loading = false
      }
    }
  }

  async function handleSearch(query: string) {
    searchQuery = query
    await loadItems()
  }

  async function handleClearSearch() {
    searchQuery = ''
    await loadItems()
  }

  function resetCollectionState() {
    itemsLoadRequestId++
    itemAssetsLoadRequestId++
    imageThumbnailLoadRequestId++
    items = []
    itemAssetMeta = new Map()
    searchQuery = ''
    error = null
    importSummary = null
    dragActive = false
    showDeleteConfirm = false
    pendingDeleteAssetId = null
    pendingDeleteItemId = null
    pendingDeleteFilename = null
    deleting = false
    deleteError = null
  }

  function notifyExplorerCollectionChanged(itemId?: string) {
    window.dispatchEvent(
      new CustomEvent<DocumentExplorerCollectionChangedDetail>(
        DOCUMENT_EXPLORER_COLLECTION_CHANGED_EVENT,
        {
          detail: { collectionId, itemId },
        }
      )
    )
  }

  async function finalizeImportedItem(itemId: string, imported: ImportedFile) {
    const store = getStore()

    // For scanned PDFs, convert to per-page image assets instead of a single PDF asset
    if (imported.type === 'pdf') {
      try {
        const isScanned = await isScannedPdf(imported.destPath)
        if (isScanned) {
          const pages = await convertScannedPdfToPages(imported, collectionId, itemId, store)
          if (pages.length > 0) {
            // Delete the original PDF file — we only keep the page images
            try {
              await deleteAssetFile(imported.destPath)
            } catch (e) {
              console.warn('[CollectionView] Failed to delete original scanned PDF:', e)
            }
            return // Pages created, no PDF asset needed
          }
          // If page conversion failed, fall through to create a regular PDF asset
        }
      } catch (e) {
        console.warn('[CollectionView] PDF profile failed, trying image-page conversion:', e)
        const pages = await convertScannedPdfToPages(imported, collectionId, itemId, store)
        if (pages.length > 0) {
          try {
            await deleteAssetFile(imported.destPath)
          } catch (deleteError) {
            console.warn('[CollectionView] Failed to delete original PDF after fallback conversion:', deleteError)
          }
          return
        }
        // If both profiling and rendering fail, keep the imported PDF as the recoverable fallback.
      }
    }

    // Default: create a single asset for the imported file
    await store.assets.create({
      itemId,
      path: imported.destPath,
      type: imported.type,
      size: imported.size,
      sortIndex: 0,
    })
  }

  const IMPORTED_FILE_METADATA_KEY = '__entropia_file_metadata'

  function buildImportedItemMetadata(imported: ImportedFile): string {
    return JSON.stringify({
      [IMPORTED_FILE_METADATA_KEY]: imported.originalMetadata,
    })
  }

  async function readAssetSize(path: string): Promise<number | null> {
    try {
      const metadata = await stat(path)
      const size = Number(metadata.size ?? 0)
      return Number.isFinite(size) ? size : null
    } catch (e) {
      console.warn('[CollectionView] Failed to read rendered page size:', e)
      return null
    }
  }

  /**
   * Convert a scanned PDF to per-page PNG image assets.
   * Returns the list of created asset IDs, or empty array on failure.
   */
  async function convertScannedPdfToPages(
    imported: ImportedFile,
    collId: string,
    itemId: string,
    store: ReturnType<typeof getStore>
  ): Promise<string[]> {
    try {
      const dataDir = await appDataDir()
      const outputDir = await join(dataDir, 'assets', collId, itemId)

      // Render all PDF pages as PNGs using the backend command
      const baseName = imported.originalName.replace(/\.[^.]+$/, '')
      const pages = await renderPdfPages(imported.destPath, outputDir, baseName)

      // Create an image asset for each page, with sort_index for ordering
      const assetIds: string[] = []
      for (const page of pages) {
        const asset = await store.assets.create({
          itemId,
          path: page.png_path,
          type: 'image',
          sortIndex: page.page_number - 1, // 0-indexed
          size: await readAssetSize(page.png_path),
        })
        assetIds.push(asset.id)
      }

      console.log(`[CollectionView] Converted scanned PDF to ${pages.length} page assets`)
      return assetIds
    } catch (e) {
      console.error('[CollectionView] Failed to convert scanned PDF to pages:', e)
      return []
    }
  }

  function getErrorDetails(e: unknown): string {
    return e instanceof Error ? e.message : String(e)
  }

  function formatImportStageError(baseMessage: string, stage: string, e: unknown): string {
    return `${baseMessage} (${stage}): ${getErrorDetails(e)}`
  }

  async function importClassifiedPaths(paths: string[], baseErrorMessage: string) {
    const store = getStore()

    // Classify files before creating items or copying assets.
    const { classified, rejected } = classifyFiles(paths)

    if (classified.length === 0) {
      if (rejected.length > 0) {
        error = `Unsupported format: ${rejected.join(', ')}`
        importSummary = {
          imported: 0,
          skipped: rejected.length,
          errors: [],
          rejected,
          lastItemTitle: null,
        }
      }
      return
    }

    // Create one item per file, copy file, create asset.
    const createdItemIds: string[] = []
    let importError: string | null = null

    for (const file of classified) {
      let itemId: string
      try {
        const item = await store.items.create({
          title: file.name.replace(/\.[^.]+$/, ''),
          collectionId,
          metadata: null,
        })
        itemId = item.id
      } catch (e) {
        importError = formatImportStageError(baseErrorMessage, 'creating item', e)
        break
      }

      try {
        const imported = await importSingleFile(file.sourcePath, collectionId, itemId)
        await store.items.update(itemId, { metadata: buildImportedItemMetadata(imported) })
        await finalizeImportedItem(itemId, imported)
        createdItemIds.push(itemId)
      } catch (e) {
        // Clean up the item if file copy failed
        try {
          await store.items.delete(itemId)
        } catch {
          // ignore cleanup errors
        }
        importError = formatImportStageError(baseErrorMessage, `importing ${file.name}`, e)
        break
      }
    }

    await loadItems()
    notifyExplorerCollectionChanged()

    // Navigate to the last created item
    if (createdItemIds.length > 0) {
      const lastItemId = createdItemIds[createdItemIds.length - 1]!
      const lastFile = classified[classified.length - 1]!
      navigation.navigate({
        name: 'item',
        collectionId,
        collectionName:
          navigation.current.name === 'collection'
            ? (navigation.current as { collectionName: string }).collectionName
            : '',
        itemId: lastItemId,
        itemTitle: lastFile.name.replace(/\.[^.]+$/, ''),
      })
    }

    importSummary = {
      imported: createdItemIds.length,
      skipped: rejected.length,
      errors: importError ? [importError] : [],
      rejected,
      lastItemTitle:
        createdItemIds.length > 0 ? classified[createdItemIds.length - 1]!.name.replace(/\.[^.]+$/, '') : null,
    }

    if (importError && createdItemIds.length === 0) {
      error = importError
    }
  }

  async function handleImport() {
    importing = true
    error = null
    importSummary = null

    // Open file picker — get raw paths BEFORE creating any items.
    let selectedPaths: string[]
    try {
      selectedPaths = await pickFiles()
    } catch (e) {
      error = formatImportStageError('Failed to import files', 'selecting files', e)
      importing = false
      return
    }

    if (selectedPaths.length === 0) {
      importing = false
      return
    }

    await importClassifiedPaths(selectedPaths, 'Failed to import files')
    importing = false
  }

  async function handleImportFromDroppedPaths(paths: string[]) {
    importing = true
    error = null
    importSummary = null

    await importClassifiedPaths(paths, 'Failed to import dropped files')
    importing = false
    dragActive = false
  }

  async function handleExportJson() {
    try {
      exporting = true
      error = null
      const store = getStore()
      await exportCollectionById(store, collectionId)
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to export collection'
    } finally {
      exporting = false
    }
  }

  // ---------------------------------------------------------------------------
  // Asset deletion flow
  // ---------------------------------------------------------------------------

  /**
   * Extract just the filename from a full native path.
   */
  function extractFilename(nativePath: string): string {
    return nativePath.split(/[/\\]/).pop() ?? t('collection.unknownFile')
  }

  /**
   * Open the delete confirmation dialog for the primary asset of an item.
   */
  function handleDeleteClick(itemId: string) {
    const meta = getItemAssetMeta(itemId)
    if (!meta.primaryAssetId || !meta.primaryAssetPath) {
      error = t('collection.error.noAssetToDelete')
      return
    }
    pendingDeleteAssetId = meta.primaryAssetId
    pendingDeleteItemId = itemId
    pendingDeleteFilename = extractFilename(meta.primaryAssetPath)
    showDeleteConfirm = true
    deleteError = null
  }

  /**
   * Cancel the delete confirmation dialog.
   */
  function handleDeleteCancel() {
    showDeleteConfirm = false
    pendingDeleteAssetId = null
    pendingDeleteItemId = null
    pendingDeleteFilename = null
    deleteError = null
  }

  /**
   * Execute the asset deletion: remove file from FS, then cascade delete from DB.
   * If the deleted asset is the item's last one, the entire item is removed
   * (with all associated metadata) and the card disappears from the grid.
   *
   * Resilient: DB errors do NOT block file deletion or UI update.
   * The file is always removed and the UI is always refreshed.
   */
  async function handleDeleteConfirm() {
    if (!pendingDeleteAssetId || !pendingDeleteItemId) return

    deleting = true
    deleteError = null

    const store = getStore()
    const meta = getItemAssetMeta(pendingDeleteItemId)
    const assetPath = meta.primaryAssetPath
    const isLastAsset = meta.assetCount <= 1

    // Step 1: Always delete the file from filesystem (ENOENT is OK)
    // Use the cached path — do NOT depend on a DB lookup
    if (assetPath) {
      try {
        await deleteAssetFile(assetPath)
      } catch (e) {
        // Log but continue — file deletion should not block UI update
        console.warn('[CollectionView] File deletion warning:', e)
      }
    }

    // Step 2: Try DB cleanup — non-blocking, but keep the warning visible if it fails.
    let dbCleanupFailed = false
    try {
      if (isLastAsset) {
        await store.items.deleteWithCascade(pendingDeleteItemId)
      } else {
        await store.assets.deleteWithCascade(pendingDeleteAssetId)
      }
    } catch (e) {
      // Log DB error but do NOT block UI update
      const message = e instanceof Error ? e.message : String(e)
      console.error('[CollectionView] DB cleanup failed:', message)
      deleteError = t('collection.error.fileRemovedDbFailed', { message })
      dbCleanupFailed = true
    }

    // Step 2b: Clean up cached PDF thumbnail if the asset was a PDF
    if (meta.primaryAssetType === 'pdf' && pendingDeleteAssetId) {
      try {
        await deletePdfThumbnail(pendingDeleteAssetId)
      } catch (e) {
        console.warn('[CollectionView] Failed to delete PDF thumbnail:', e)
        // Non-fatal: thumbnail cache cleanup is best-effort
      }
    }

    if (meta.primaryAssetType === 'image' && pendingDeleteAssetId) {
      try {
        await deleteImageThumbnail(pendingDeleteAssetId)
      } catch (e) {
        console.warn('[CollectionView] Failed to delete image thumbnail:', e)
      }
    }

    if (dbCleanupFailed) {
      await loadItems()
      notifyExplorerCollectionChanged(pendingDeleteItemId)
      deleting = false
      return
    }

    // Step 3: Update UI after confirmed DB cleanup
    if (isLastAsset) {
      items = items.filter((i) => i.id !== pendingDeleteItemId)
      const newMeta = new Map(itemAssetMeta)
      newMeta.delete(pendingDeleteItemId)
      itemAssetMeta = newMeta
    } else {
      await refreshItemAssetMeta([pendingDeleteItemId])
    }

    notifyExplorerCollectionChanged(pendingDeleteItemId)

    // Step 4: Close only on full success.
    handleDeleteCancel()
    deleting = false
  }

  $effect(() => {
    if (collectionId === activeCollectionId) return

    activeCollectionId = collectionId
    resetCollectionState()
    void loadItems()
  })

  onMount(() => {

    getCurrentWebview()
      .onDragDropEvent((event: { payload: DragDropEvent }) => {
        if (event.payload.type === 'enter') {
          dragActive = true
          return
        }

        if (event.payload.type === 'over') {
          dragActive = true
          return
        }

        if (event.payload.type === 'leave') {
          dragActive = false
          return
        }

        if (event.payload.type !== 'drop') {
          return
        }

        dragActive = false
        void handleImportFromDroppedPaths(event.payload.paths)
      })
      .then((unlisten: () => void) => {
        unlistenDragDrop = unlisten
      })

    // Listen for asset image updates from ItemView (crop, rotate, erase, undo).
    // When an image is edited, the asset path changes to a new versioned file.
    // We must invalidate the cached thumbnail URL so the card shows the latest
    // version instead of a stale browser-cached image.
    listen<{ itemId: string; assetId: string; path: string }>('asset:image-updated', (event) => {
      const { itemId: updatedItemId } = event.payload
      // Invalidate the cached metadata for this item so the thumbnail
      // is regenerated with the new path (which includes a cache-busting
      // version number since edits create new files).
      void refreshItemAssetMeta([updatedItemId])
    })
      .then((unlisten) => {
        unlistenAssetUpdate = unlisten
      })
      .catch((e: unknown) => {
        console.warn('[CollectionView] Failed to subscribe to asset:image-updated:', e)
      })
  })

  onDestroy(() => {
    unlistenDragDrop?.()
    unlistenAssetUpdate?.()
  })
</script>

<div class="collection-view page-shell" class:drag-active={dragActive}>
  <section class="page-header collection-view__header">
    <div class="page-header__content">
      <span class="page-header__eyebrow">{$currentLocale && t('collection.active')}</span>
      <h1>{collectionTitle}</h1>
      <p>{$currentLocale && t('collection.subtitle')}</p>
      <span class="page-header__meta">{visibleCountLabel}</span>
    </div>

    <div class="page-toolbar collection-toolbar">
      <SearchBar
        placeholder={$currentLocale && t('collection.searchPlaceholder')}
        onsearch={handleSearch}
        onclear={handleClearSearch}
      />
      <Button variant="primary" onclick={handleImport} disabled={importing}>
        {importing
          ? $currentLocale && t('collection.importing')
          : $currentLocale && t('collection.import')}
      </Button>
      <Button variant="secondary" onclick={handleExportJson} disabled={exporting}>
        {exporting
          ? $currentLocale && t('collection.exporting')
          : $currentLocale && t('collection.export')}
      </Button>
    </div>
  </section>

  {#if error}
    <p class="surface-message surface-message--error">{error}</p>
  {/if}

  {#if importing || importSummary}
    <section class="import-summary" aria-live="polite" aria-label={t('collection.importSummary.title')}>
      <div class="import-summary__header">
        <strong>
          {importing ? t('collection.importSummary.importingTitle') : t('collection.importSummary.title')}
        </strong>
        <span>
          {#if importing}
            {t('collection.importSummary.importingDescription')}
          {:else if importSummary?.lastItemTitle}
            {t('collection.importSummary.openedLast', { title: importSummary.lastItemTitle })}
          {:else}
            {t('collection.importSummary.reviewCollection')}
          {/if}
        </span>
      </div>

      {#if importSummary}
        <dl class="import-summary__counts">
          <div>
            <dt>{t('collection.importSummary.imported')}</dt>
            <dd>{importSummary.imported}</dd>
          </div>
          <div>
            <dt>{t('collection.importSummary.skipped')}</dt>
            <dd>{importSummary.skipped}</dd>
          </div>
          <div>
            <dt>{t('collection.importSummary.errors')}</dt>
            <dd>{importSummary.errors.length}</dd>
          </div>
        </dl>

        {#if importSummary.rejected.length > 0}
          <p class="import-summary__detail">
            {t('collection.importSummary.skippedFiles', { files: importSummary.rejected.join(', ') })}
          </p>
        {/if}
        {#if importSummary.errors.length > 0}
          <p class="import-summary__detail import-summary__detail--error">
            {importSummary.errors[0]}
          </p>
        {/if}
      {/if}
    </section>
  {/if}

  {#if dragActive}
    <div class="drop-hint">{t('collection.dropHint')}</div>
  {/if}

  {#if loading}
    <p class="surface-message surface-message--center">{t('collection.loading')}</p>
  {:else if items.length === 0}
    <div class="surface-message surface-message--center empty">
      <p>
        {searchQuery ? t('collection.emptySearch') : t('collection.empty')}
      </p>
    </div>
  {:else}
    <div class="grid">
      {#each items as item (item.id)}
        {@const meta = getItemAssetMeta(item.id)}
        <ItemCard
          id={item.id}
          title={item.title}
          assetCount={meta.assetCount}
          thumbnailPath={meta.thumbnailUrl ?? undefined}
          primaryAssetType={(meta.primaryAssetType as 'image' | 'pdf' | 'audio' | undefined) ??
            undefined}
          onclick={() =>
            navigation.navigate({
              name: 'item',
              collectionId,
              collectionName:
                navigation.current.name === 'collection'
                  ? (navigation.current as { collectionName: string }).collectionName
                  : '',
              itemId: item.id,
              itemTitle: item.title,
            })}
          onDelete={() => handleDeleteClick(item.id)}
        />
      {/each}
    </div>
  {/if}

  <!-- Delete confirmation modal -->
  {#if showDeleteConfirm}
    <ConfirmDialog
      title={t('collection.deleteAssetTitle')}
      titleId="delete-modal-title"
      message={t('collection.deleteAssetMessage', { name: pendingDeleteFilename ?? '' })}
      error={deleteError}
      cancelLabel={t('collections.cancel')}
      confirmIcon="delete"
      confirmAriaLabel={t('collection.deleteAssetAria')}
      confirmTitle={deleting ? t('collection.deletingAssetTitle') : t('collection.deleteAssetAria')}
      variant="destructive"
      confirming={deleting}
      cancelDisabled={deleting}
      oncancel={handleDeleteCancel}
      onconfirm={handleDeleteConfirm}
    />
  {/if}
</div>

<style>
  .collection-view {
    min-height: 100%;
  }

  .collection-view__header {
    align-items: center;
  }

  .collection-toolbar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex: 1;
  }

  .collection-toolbar :global(.search-bar) {
    min-width: min(100%, 340px);
    flex: 1 1 280px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--space-3);
  }

  .empty {
    min-height: 220px;
  }

  .drop-hint {
    padding: var(--space-4);
    border: 1px dashed color-mix(in srgb, var(--color-accent) 44%, transparent);
    border-radius: var(--radius-surface);
    color: var(--color-text-secondary);
    text-align: center;
    background: var(--color-surface-sunken);
  }

  .collection-view.drag-active {
    outline: 1px dashed var(--color-primary);
    outline-offset: 6px;
    border-radius: var(--radius-md);
  }

  @media (max-width: 720px) {
    .collection-toolbar {
      width: 100%;
      justify-content: stretch;
    }

    .collection-toolbar :global(.search-bar),
    .collection-toolbar :global(.btn) {
      width: 100%;
    }
  }

  .import-summary {
    display: grid;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid color-mix(in srgb, var(--color-accent) 24%, transparent);
    border-radius: var(--radius-surface);
    background: color-mix(in srgb, var(--color-surface) 92%, var(--color-accent));
  }

  .import-summary__header {
    display: grid;
    gap: var(--space-1);
    color: var(--color-text-secondary);
  }

  .import-summary__header strong {
    color: var(--color-text-primary);
  }

  .import-summary__counts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: 0;
  }

  .import-summary__counts div {
    min-width: 96px;
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--color-surface-sunken);
  }

  .import-summary__counts dt {
    color: var(--color-text-secondary);
    font-size: var(--font-size-xs);
  }

  .import-summary__counts dd {
    margin: 0;
    color: var(--color-text-primary);
    font-weight: 700;
  }

  .import-summary__detail {
    margin: 0;
    color: var(--color-text-secondary);
  }

  .import-summary__detail--error {
    color: var(--color-danger);
  }
</style>
