import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import CollectionView from './CollectionView.svelte'
import { locale } from '$lib/i18n'

const { storeRef, navigationRef, fileImportRef, dragDropRef } = vi.hoisted(() => ({
  storeRef: {
    current: {
      items: {
        findByCollection: vi.fn(),
        searchByText: vi.fn(),
        create: vi.fn(),
        update: vi.fn(),
        delete: vi.fn(),
        deleteWithCascade: vi.fn(),
      },
      assets: {
        create: vi.fn(),
        findByItem: vi.fn(),
        findById: vi.fn(),
        deleteWithCascade: vi.fn(),
      },
    },
  },
  navigationRef: {
    current: { name: 'collection', collectionName: 'Colección' } as const,
    navigate: vi.fn(),
  },
  fileImportRef: {
    pickFiles: vi.fn(),
    classifyFiles: vi.fn(),
    importSingleFile: vi.fn(),
    isScannedPdf: vi.fn(),
    renderPdfPages: vi.fn(),
  },
  dragDropRef: {
    onDragDropEvent: vi.fn(),
    handler: undefined as
      | ((event: { payload: { type: string; paths?: string[] } }) => void)
      | undefined,
  },
}))

type ItemRow = {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  collectionId: string
  metadata: string | null
}

type AssetRow = {
  id: string
  itemId: string
  path: string
  type: string
  size: number | null
  createdAt: number
}

function createStore(items: ItemRow[], assets: AssetRow[] = []) {
  return {
    items: {
      findByCollection: vi.fn().mockResolvedValue(items),
      searchByText: vi.fn().mockResolvedValue(items),
      create: vi.fn(),
      update: vi.fn(),
      delete: vi.fn(),
      deleteWithCascade: vi.fn().mockResolvedValue(undefined),
    },
    assets: {
      create: vi.fn(),
      findByItem: vi.fn().mockResolvedValue(assets),
      findById: vi.fn().mockResolvedValue(assets[0] ?? null),
      deleteWithCascade: vi.fn().mockResolvedValue(undefined),
    },
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

vi.mock('$lib/db', () => ({
  getStore: () => storeRef.current,
}))

vi.mock('$lib/navigation', () => ({
  navigation: navigationRef,
}))

vi.mock('$lib/file-import', () => ({
  pickFiles: fileImportRef.pickFiles,
  classifyFiles: fileImportRef.classifyFiles,
  importSingleFile: fileImportRef.importSingleFile,
  isScannedPdf: fileImportRef.isScannedPdf,
  renderPdfPages: fileImportRef.renderPdfPages,
  pickAndImportFiles: vi.fn().mockResolvedValue([]),
  importFilesFromPaths: vi
    .fn()
    .mockResolvedValue({ imported: [], rejected: [], skippedDuplicatePaths: 0 }),
  getAssetUrl: vi.fn().mockImplementation((p: string) => `asset://localhost${p}`),
  deleteAssetFile: vi.fn().mockResolvedValue(undefined),
  generatePdfThumbnail: vi.fn().mockResolvedValue('asset://localhost/thumbnails/asset-1.png'),
  deletePdfThumbnail: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('$lib/export', () => ({
  exportCollectionById: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: vi.fn(() => ({
    onDragDropEvent: dragDropRef.onDragDropEvent,
  })),
}))

beforeEach(() => {
  fileImportRef.pickFiles.mockReset()
  fileImportRef.classifyFiles.mockReset()
  fileImportRef.importSingleFile.mockReset()
  fileImportRef.isScannedPdf.mockReset()
  fileImportRef.renderPdfPages.mockReset()
  fileImportRef.pickFiles.mockResolvedValue([])
  fileImportRef.classifyFiles.mockReturnValue({ classified: [], rejected: [] })
  fileImportRef.isScannedPdf.mockResolvedValue(false)
  dragDropRef.handler = undefined
  dragDropRef.onDragDropEvent.mockReset()
  dragDropRef.onDragDropEvent.mockImplementation((handler) => {
    dragDropRef.handler = handler
    return Promise.resolve(vi.fn())
  })
})

describe('CollectionView consumer compatibility', () => {
  beforeEach(() => {
    locale.set('es')
    vi.useFakeTimers()
    navigationRef.navigate.mockReset()
    storeRef.current = createStore([
      {
        id: 'item-1',
        title: 'Acta',
        createdAt: Date.now(),
        updatedAt: Date.now(),
        collectionId: 'col-1',
        metadata: null,
      },
    ])
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('uses SearchBar onsearch/onclear contract to call collection queries', async () => {
    render(CollectionView, { collectionId: 'col-1' })

    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(0)

    await waitFor(() => {
      expect(storeRef.current.items.findByCollection).toHaveBeenCalledWith('col-1')
    })

    expect(screen.getByRole('heading', { name: 'Colección' })).toBeInTheDocument()
    expect(
      screen.getByText('Importá, explorá y mantené ordenados los assets de esta colección.')
    ).toBeInTheDocument()
    expect(screen.getByText('1 documento visible')).toBeInTheDocument()

    const searchInput = screen.getByRole('searchbox')
    await fireEvent.input(searchInput, { target: { value: 'acta' } })
    vi.advanceTimersByTime(300)

    await waitFor(() => {
      expect(storeRef.current.items.searchByText).toHaveBeenCalledWith('col-1', 'acta')
    })

    await fireEvent.click(screen.getByRole('button', { name: /clear search/i }))

    await waitFor(() => {
      expect(storeRef.current.items.findByCollection).toHaveBeenCalledTimes(2)
    })
  })

  it('shows the empty-state guidance when there are no items', async () => {
    storeRef.current = createStore([])

    render(CollectionView, { collectionId: 'col-1' })

    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(0)

    await waitFor(() => {
      expect(storeRef.current.items.findByCollection).toHaveBeenCalledWith('col-1')
    })

    expect(screen.getByText('0 documentos visibles')).toBeInTheDocument()
    expect(
      screen.getByText(
        'Todavía no hay documentos en esta colección. Importá archivos para empezar a trabajar.'
      )
    ).toBeInTheDocument()
  })

  it('updates translated collection copy when locale changes', async () => {
    render(CollectionView, { collectionId: 'col-1' })

    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(0)

    expect(await screen.findByRole('heading', { name: 'Colección' })).toBeInTheDocument()

    locale.set('en')

    await waitFor(() => {
      expect(screen.getByText('1 visible document')).toBeInTheDocument()
      expect(
        screen.getByText('Import, browse, and keep this collection assets organized.')
      ).toBeInTheDocument()
    })
  })

  it('ignores stale item loads that resolve after a newer search', async () => {
    const firstLoad = deferred<ItemRow[]>()
    const searchLoad = deferred<ItemRow[]>()
    const oldItem: ItemRow = {
      id: 'item-old',
      title: 'Acta vieja',
      createdAt: Date.now(),
      updatedAt: Date.now(),
      collectionId: 'col-1',
      metadata: null,
    }
    const newItem: ItemRow = {
      id: 'item-new',
      title: 'Acta nueva',
      createdAt: Date.now(),
      updatedAt: Date.now(),
      collectionId: 'col-1',
      metadata: null,
    }

    storeRef.current = {
      items: {
        findByCollection: vi.fn().mockReturnValueOnce(firstLoad.promise),
        searchByText: vi.fn().mockReturnValueOnce(searchLoad.promise),
        create: vi.fn(),
        update: vi.fn(),
        delete: vi.fn(),
        deleteWithCascade: vi.fn().mockResolvedValue(undefined),
      },
      assets: {
        create: vi.fn(),
        findByItem: vi.fn().mockResolvedValue([]),
        findById: vi.fn().mockResolvedValue(null),
        deleteWithCascade: vi.fn().mockResolvedValue(undefined),
      },
    }

    render(CollectionView, { collectionId: 'col-1' })

    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'acta' } })
    await vi.advanceTimersByTimeAsync(300)

    searchLoad.resolve([newItem])

    expect(await screen.findByText('Acta nueva')).toBeInTheDocument()

    firstLoad.resolve([oldItem])

    await waitFor(() => {
      expect(screen.getByText('Acta nueva')).toBeInTheDocument()
      expect(screen.queryByText('Acta vieja')).not.toBeInTheDocument()
    })
  })
})

describe('CollectionView import flow', () => {
  beforeEach(() => {
    locale.set('es')
    vi.useFakeTimers()
    navigationRef.navigate.mockReset()
    navigationRef.current = { name: 'collection', collectionName: 'Colección' }
    storeRef.current = createStore([])
    storeRef.current.items.create = vi.fn().mockResolvedValue({ id: 'item-new' })
    storeRef.current.items.update = vi.fn().mockResolvedValue(undefined)
    storeRef.current.items.delete = vi.fn().mockResolvedValue(undefined)
    storeRef.current.assets.create = vi.fn().mockResolvedValue({ id: 'asset-new' })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  function mockImageImport(sourcePath = 'C:\\tmp\\photo.png') {
    fileImportRef.classifyFiles.mockReturnValue({
      classified: [{ sourcePath, name: 'photo.png', type: 'image' }],
      rejected: [],
    })
    fileImportRef.importSingleFile.mockResolvedValue({
      originalName: 'photo.png',
      originalPath: sourcePath,
      destPath: 'C:\\app-data\\assets\\col-1\\item-new\\photo.png',
      type: 'image',
      size: 123,
      originalMetadata: {
        originalName: 'photo.png',
        originalPath: sourcePath,
        importedAt: '2026-06-02T00:00:00.000Z',
        sizeBytes: 123,
      },
    })
  }

  it('imports picker-selected paths through the shared item/asset workflow', async () => {
    const sourcePath = 'C:\\tmp\\photo.png'
    fileImportRef.pickFiles.mockResolvedValue([sourcePath])
    mockImageImport(sourcePath)

    render(CollectionView, { collectionId: 'col-1' })

    await fireEvent.click(screen.getByRole('button', { name: /Importar documento/ }))

    await waitFor(() => {
      expect(fileImportRef.classifyFiles).toHaveBeenCalledWith([sourcePath])
      expect(storeRef.current.items.create).toHaveBeenCalledWith({
        title: 'photo',
        collectionId: 'col-1',
        metadata: null,
      })
      expect(fileImportRef.importSingleFile).toHaveBeenCalledWith(sourcePath, 'col-1', 'item-new')
      expect(storeRef.current.assets.create).toHaveBeenCalledWith({
        itemId: 'item-new',
        path: 'C:\\app-data\\assets\\col-1\\item-new\\photo.png',
        type: 'image',
        size: 123,
        sortIndex: 0,
      })
      expect(navigationRef.navigate).toHaveBeenCalledWith({
        name: 'item',
        collectionId: 'col-1',
        collectionName: 'Colección',
        itemId: 'item-new',
        itemTitle: 'photo',
      })
    })
  })

  it('imports dropped paths through the same item/asset workflow', async () => {
    const sourcePath = 'C:\\tmp\\photo.png'
    mockImageImport(sourcePath)

    render(CollectionView, { collectionId: 'col-1' })

    await waitFor(() => {
      expect(dragDropRef.handler).toBeDefined()
    })

    dragDropRef.handler?.({ payload: { type: 'drop', paths: [sourcePath] } })

    await waitFor(() => {
      expect(fileImportRef.classifyFiles).toHaveBeenCalledWith([sourcePath])
      expect(storeRef.current.items.create).toHaveBeenCalledWith({
        title: 'photo',
        collectionId: 'col-1',
        metadata: null,
      })
      expect(fileImportRef.importSingleFile).toHaveBeenCalledWith(sourcePath, 'col-1', 'item-new')
      expect(storeRef.current.assets.create).toHaveBeenCalledWith({
        itemId: 'item-new',
        path: 'C:\\app-data\\assets\\col-1\\item-new\\photo.png',
        type: 'image',
        size: 123,
        sortIndex: 0,
      })
      expect(navigationRef.navigate).toHaveBeenCalledWith({
        name: 'item',
        collectionId: 'col-1',
        collectionName: 'Colección',
        itemId: 'item-new',
        itemTitle: 'photo',
      })
    })
  })
})

describe('CollectionView asset deletion', () => {
  const sampleAsset: AssetRow = {
    id: 'asset-1',
    itemId: 'item-1',
    path: '/app-data/assets/col-1/item-1/uuid_acta.pdf',
    type: 'pdf',
    size: 1024,
    createdAt: Date.now(),
  }

  beforeEach(() => {
    locale.set('es')
    vi.useFakeTimers()
    navigationRef.navigate.mockReset()
    storeRef.current = createStore(
      [
        {
          id: 'item-1',
          title: 'Acta',
          createdAt: Date.now(),
          updatedAt: Date.now(),
          collectionId: 'col-1',
          metadata: null,
        },
      ],
      [sampleAsset]
    )
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  async function renderAndWaitForItems() {
    render(CollectionView, { collectionId: 'col-1' })

    // Wait for the async load to complete
    await waitFor(() => {
      expect(storeRef.current.items.findByCollection).toHaveBeenCalled()
    })

    // Advance timers to let the promise resolution propagate to Svelte state
    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(0)
  }

  it('shows delete confirmation modal when delete button is clicked', async () => {
    await renderAndWaitForItems()

    // Find and click the delete button
    const deleteBtn = screen.getByRole('button', { name: 'Delete Acta' })
    await fireEvent.click(deleteBtn)

    // Modal should appear
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(screen.getByText(/¿Seguro que querés eliminar/)).toBeInTheDocument()
    expect(screen.getByText(/uuid_acta\.pdf/)).toBeInTheDocument()
  })

  it('cancels deletion when Cancel is clicked', async () => {
    await renderAndWaitForItems()

    const deleteBtn = screen.getByRole('button', { name: 'Delete Acta' })
    await fireEvent.click(deleteBtn)

    expect(screen.getByRole('dialog')).toBeInTheDocument()

    const cancelBtn = screen.getByRole('button', { name: 'Cancelar' })
    await fireEvent.click(cancelBtn)

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    })
  })

  it('deletes entire item when last asset is removed — card disappears from grid', async () => {
    const { deleteAssetFile } = await import('$lib/file-import')

    await renderAndWaitForItems()

    // Verify the card is visible
    expect(screen.getByText('Acta')).toBeInTheDocument()

    const deleteBtn = screen.getByRole('button', { name: 'Delete Acta' })
    await fireEvent.click(deleteBtn)

    const confirmBtn = screen.getByRole('button', { name: 'Eliminar asset' })
    expect(confirmBtn.querySelector('svg')).toBeInTheDocument()
    await fireEvent.click(confirmBtn)

    await waitFor(() => {
      expect(deleteAssetFile).toHaveBeenCalledWith(sampleAsset.path)
      // Last asset → entire item is deleted, not just the asset
      expect(storeRef.current.items.deleteWithCascade).toHaveBeenCalledWith('item-1')
    })

    // Card should be removed from the grid (no ghost card)
    await waitFor(() => {
      expect(screen.queryByText('Acta')).not.toBeInTheDocument()
    })

    // Modal should close after successful deletion
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    })
  })

  it('keeps the dialog and warning visible when DB cleanup fails', async () => {
    const { deleteAssetFile } = await import('$lib/file-import')
    // Simulate DB failure
    storeRef.current.items.deleteWithCascade = vi.fn().mockRejectedValueOnce(new Error('DB locked'))

    await renderAndWaitForItems()

    expect(screen.getByText('Acta')).toBeInTheDocument()

    const deleteBtn = screen.getByRole('button', { name: 'Delete Acta' })
    await fireEvent.click(deleteBtn)

    const confirmBtn = screen.getByRole('button', { name: 'Eliminar asset' })
    expect(confirmBtn.querySelector('svg')).toBeInTheDocument()
    await fireEvent.click(confirmBtn)

    await waitFor(() => {
      // File was still attempted
      expect(deleteAssetFile).toHaveBeenCalledWith(sampleAsset.path)
      // DB failed but...
    })

    // Card stays visible because DB cleanup is the authoritative state.
    await waitFor(() => {
      expect(screen.getByText('Acta')).toBeInTheDocument()
    })

    // Modal stays open and explains the partial failure instead of pretending success.
    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument()
      expect(screen.getByText(/DB locked/)).toBeInTheDocument()
    })
  })

  it('does NOT call findById — uses cached path for file deletion', async () => {
    const { deleteAssetFile } = await import('$lib/file-import')

    await renderAndWaitForItems()

    const deleteBtn = screen.getByRole('button', { name: 'Delete Acta' })
    await fireEvent.click(deleteBtn)

    const confirmBtn = screen.getByRole('button', { name: 'Eliminar asset' })
    expect(confirmBtn.querySelector('svg')).toBeInTheDocument()
    await fireEvent.click(confirmBtn)

    await waitFor(() => {
      expect(deleteAssetFile).toHaveBeenCalled()
      // findById should NOT be called — path comes from cache
      expect(storeRef.current.assets.findById).not.toHaveBeenCalled()
    })
  })
})

describe('CollectionView PDF thumbnail', () => {
  const pdfAsset: AssetRow = {
    id: 'asset-pdf-1',
    itemId: 'item-1',
    path: '/app-data/assets/col-1/item-1/uuid_doc.pdf',
    type: 'pdf',
    size: 2048,
    createdAt: Date.now(),
  }

  beforeEach(() => {
    locale.set('es')
    vi.useFakeTimers()
    navigationRef.navigate.mockReset()
    storeRef.current = createStore(
      [
        {
          id: 'item-1',
          title: 'PDF Document',
          createdAt: Date.now(),
          updatedAt: Date.now(),
          collectionId: 'col-1',
          metadata: null,
        },
      ],
      [pdfAsset]
    )
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  async function renderAndWaitForItems() {
    render(CollectionView, { collectionId: 'col-1' })

    await waitFor(() => {
      expect(storeRef.current.items.findByCollection).toHaveBeenCalled()
    })

    await vi.advanceTimersByTimeAsync(0)
    await vi.advanceTimersByTimeAsync(0)
  }

  it('does not generate thumbnails for PDF assets during initial exploration', async () => {
    const { generatePdfThumbnail } = await import('$lib/file-import')

    await renderAndWaitForItems()

    expect(generatePdfThumbnail).not.toHaveBeenCalled()
  })

  it('cleans up PDF thumbnail when deleting a PDF asset', async () => {
    const { deletePdfThumbnail } = await import('$lib/file-import')

    await renderAndWaitForItems()

    const deleteBtn = screen.getByRole('button', { name: 'Delete PDF Document' })
    await fireEvent.click(deleteBtn)

    const confirmBtn = screen.getByRole('button', { name: 'Eliminar asset' })
    expect(confirmBtn.querySelector('svg')).toBeInTheDocument()
    await fireEvent.click(confirmBtn)

    await waitFor(() => {
      expect(deletePdfThumbnail).toHaveBeenCalledWith(pdfAsset.id)
    })
  })

  it('renders the confirm delete action as the shared trash icon button', async () => {
    await renderAndWaitForItems()

    await fireEvent.click(screen.getByRole('button', { name: 'Delete PDF Document' }))

    const confirmBtn = screen.getByRole('button', { name: 'Eliminar asset' })
    expect(confirmBtn.querySelector('svg')).toBeInTheDocument()
    expect(confirmBtn).not.toHaveTextContent('Eliminar')
  })
})
