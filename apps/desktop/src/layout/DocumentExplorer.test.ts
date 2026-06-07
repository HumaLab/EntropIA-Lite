import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { locale } from '$lib/i18n'
import DocumentExplorer from './DocumentExplorer.svelte'

const state = vi.hoisted(() => {
  const subscribers = new Set<(value: unknown) => void>()

  const snapshot = {
    history: [
      { name: 'collections' as const },
      { name: 'collection' as const, id: 'col-1', collectionName: 'Colección 1' },
      {
        name: 'item' as const,
        collectionId: 'col-1',
        collectionName: 'Colección 1',
        itemId: 'item-1',
        itemTitle: 'Acta 1',
      },
    ],
    current: {
      name: 'item' as const,
      collectionId: 'col-1',
      collectionName: 'Colección 1',
      itemId: 'item-1',
      itemTitle: 'Acta 1',
    },
    canGoBack: true,
    breadcrumb: ['Colecciones', 'Colección 1', 'Acta 1'],
  }

  const store = {
    collections: {
      findAll: vi.fn().mockResolvedValue([
        { id: 'col-1', name: 'Colección 1', description: null, createdAt: 1, updatedAt: 1 },
        { id: 'col-2', name: 'Colección 2', description: null, createdAt: 1, updatedAt: 1 },
      ]),
      countItems: vi.fn().mockImplementation(async (id: string) => (id === 'col-1' ? 2 : 1)),
    },
    items: {
      findByCollection: vi.fn().mockImplementation(async (collectionId: string) => {
        if (collectionId === 'col-2') {
          return [
            {
              id: 'item-3',
              title: 'Acta 3',
              collectionId: 'col-2',
              metadata: null,
              createdAt: 1,
              updatedAt: 3,
            },
          ]
        }

        return [
          {
            id: 'item-1',
            title: 'Acta 1',
            collectionId: 'col-1',
            metadata: null,
            createdAt: 1,
            updatedAt: 2,
          },
          {
            id: 'item-2',
            title: 'Acta 2',
            collectionId: 'col-1',
            metadata: null,
            createdAt: 1,
            updatedAt: 1,
          },
        ]
      }),
    },
    assets: {
      findByItem: vi.fn().mockImplementation(async (itemId: string) => {
        if (itemId === 'item-2') {
          return [
            {
              id: 'asset-3',
              itemId: 'item-2',
              path: 'docs/foto-acta-2.png',
              type: 'image',
              size: 12,
              sortIndex: 0,
              createdAt: 1,
            },
          ]
        }

        if (itemId === 'item-3') {
          return [
            {
              id: 'asset-4',
              itemId: 'item-3',
              path: 'docs/acta-3.pdf',
              type: 'pdf',
              size: 14,
              sortIndex: 0,
              createdAt: 1,
            },
          ]
        }

        return [
          {
            id: 'asset-1',
            itemId: 'item-1',
            path: 'docs/acta-1.pdf',
            type: 'pdf',
            size: 10,
            sortIndex: 0,
            createdAt: 1,
          },
          {
            id: 'asset-2',
            itemId: 'item-1',
            path: 'docs/acta-1-audio.mp3',
            type: 'audio',
            size: 10,
            sortIndex: 1,
            createdAt: 1,
          },
        ]
      }),
    },
  }

  function emit() {
    const payload = {
      history: [...snapshot.history],
      current: { ...snapshot.current },
      canGoBack: snapshot.canGoBack,
      breadcrumb: [...snapshot.breadcrumb],
    }
    subscribers.forEach((run) => run(payload))
  }

  return {
    subscribers,
    snapshot,
    store,
    navigate: vi.fn(),
    replace: vi.fn(),
    resetToPath: vi.fn(),
    emit,
  }
})

vi.mock('$lib/navigation', () => ({
  navigation: {
    subscribe(run: (value: unknown) => void) {
      state.subscribers.add(run)
      state.emit()
      return () => state.subscribers.delete(run)
    },
    navigate: state.navigate,
    replace: state.replace,
    resetToPath: state.resetToPath,
  },
}))

vi.mock('$lib/db', () => ({
  getStore: () => state.store,
}))

function persistOpenTree(collections: string[] = [], items: string[] = []) {
  localStorage.setItem(
    'entropia-document-explorer-tree',
    JSON.stringify({
      collections,
      items,
    })
  )
}

describe('DocumentExplorer', () => {
  beforeEach(() => {
    locale.set('es')
    localStorage.clear()
    state.snapshot.history = [
      { name: 'collections' as const },
      { name: 'collection' as const, id: 'col-1', collectionName: 'Colección 1' },
      {
        name: 'item' as const,
        collectionId: 'col-1',
        collectionName: 'Colección 1',
        itemId: 'item-1',
        itemTitle: 'Acta 1',
      },
    ]
    state.snapshot.current = {
      name: 'item' as const,
      collectionId: 'col-1',
      collectionName: 'Colección 1',
      itemId: 'item-1',
      itemTitle: 'Acta 1',
    }
    state.snapshot.canGoBack = true
    state.snapshot.breadcrumb = ['Colecciones', 'Colección 1', 'Acta 1']
    state.navigate.mockReset()
    state.replace.mockReset()
    state.resetToPath.mockReset()
    state.store.collections.findAll.mockClear()
    state.store.collections.countItems.mockClear()
    state.store.items.findByCollection.mockClear()
    state.store.assets.findByItem.mockClear()
  })

  it('expands collection nodes without navigating and lazy-loads documents', async () => {
    render(DocumentExplorer)

    const expandCollection = await screen.findByRole('button', {
      name: 'Expandir colección Colección 2',
    })

    await fireEvent.click(expandCollection)

    expect(state.navigate).not.toHaveBeenCalled()
    expect(state.replace).not.toHaveBeenCalled()

    await waitFor(() => {
      expect(state.store.items.findByCollection).toHaveBeenCalledWith('col-2')
    })

    expect(await screen.findByRole('treeitem', { name: 'Acta 3' })).toBeInTheDocument()
  })

  it('renders active hierarchy and replaces sibling item navigation', async () => {
    persistOpenTree(['col-1'], ['item-1'])
    const assetSelectRequests: CustomEvent[] = []
    const handleAssetSelectRequest = (event: Event) => {
      assetSelectRequests.push(event as CustomEvent)
    }

    window.addEventListener(
      'entropia:document-explorer-asset-select-request',
      handleAssetSelectRequest
    )

    render(DocumentExplorer)

    await screen.findByText('Colección 1')
    await screen.findByText('Acta 2')
    await screen.findByText('acta-1.pdf')

    await fireEvent.click(screen.getByRole('button', { name: 'Acta 2' }))

    expect(state.replace).toHaveBeenCalledWith({
      name: 'item',
      collectionId: 'col-1',
      collectionName: 'Colección 1',
      itemId: 'item-2',
      itemTitle: 'Acta 2',
    })
    expect(assetSelectRequests.at(-1)?.detail).toEqual({ itemId: 'item-2', assetId: 'asset-3' })
    expect(state.resetToPath).not.toHaveBeenCalled()

    window.removeEventListener(
      'entropia:document-explorer-asset-select-request',
      handleAssetSelectRequest
    )
  })

  it('rebuilds canonical path when clicking a collection from another collection', async () => {
    render(DocumentExplorer)

    const collectionButton = (await screen.findByText('Colección 2')).closest('button')

    if (!collectionButton) {
      throw new Error('Expected collection button to be rendered')
    }

    await fireEvent.click(collectionButton)

    expect(state.resetToPath).toHaveBeenCalledWith([
      { name: 'collections' },
      { name: 'collection', id: 'col-2', collectionName: 'Colección 2' },
    ])
    expect(state.replace).not.toHaveBeenCalled()
    expect(state.navigate).not.toHaveBeenCalled()
  })

  it('rebuilds canonical path when clicking an item from another collection', async () => {
    render(DocumentExplorer)

    await fireEvent.click(
      await screen.findByRole('button', {
        name: 'Expandir colección Colección 2',
      })
    )

    const targetItem = await screen.findByRole('button', { name: /Acta 3/ })
    await fireEvent.click(targetItem)

    expect(state.resetToPath).toHaveBeenCalledWith([
      { name: 'collections' },
      { name: 'collection', id: 'col-2', collectionName: 'Colección 2' },
      {
        name: 'item',
        collectionId: 'col-2',
        collectionName: 'Colección 2',
        itemId: 'item-3',
        itemTitle: 'Acta 3',
      },
    ])
    expect(state.replace).not.toHaveBeenCalled()
    expect(state.navigate).not.toHaveBeenCalled()
  })

  it('keeps multi-asset document nodes expandable and nested', async () => {
    persistOpenTree(['col-1'])

    render(DocumentExplorer)

    const expandItem = await screen.findByRole('button', {
      name: 'Expandir documento Acta 1',
    })

    await fireEvent.click(expandItem)

    expect(state.navigate).not.toHaveBeenCalled()
    expect(state.replace).not.toHaveBeenCalled()

    expect(screen.getByRole('treeitem', { name: 'Acta 1' })).toHaveAttribute('aria-expanded', 'true')
    expect(await screen.findByRole('treeitem', { name: 'acta-1.pdf' })).toBeInTheDocument()
    expect(await screen.findByRole('treeitem', { name: 'acta-1-audio.mp3' })).toBeInTheDocument()
  })

  it('flattens single-asset items without rendering a nested duplicate asset row', async () => {
    persistOpenTree(['col-1'])

    render(DocumentExplorer)

    await screen.findByText('Acta 2')

    await waitFor(() => {
      expect(state.store.assets.findByItem).toHaveBeenCalledWith('item-2')
    })

    expect(screen.getByRole('treeitem', { name: 'Acta 2' })).not.toHaveAttribute('aria-expanded')
    expect(screen.queryByRole('button', { name: 'Expandir documento Acta 2' })).not.toBeInTheDocument()
    expect(screen.queryByRole('treeitem', { name: 'foto-acta-2.png' })).not.toBeInTheDocument()
    expect(screen.getByText('image')).toBeInTheDocument()
  })

  it('keeps the document explorer open and removes the internal collapse control', async () => {
    localStorage.setItem('entropia-document-explorer-open', 'false')

    render(DocumentExplorer)

    expect(await screen.findByRole('tree', { name: 'Explorador de documentos' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Cerrar explorador de documentos' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Abrir explorador de documentos' })).not.toBeInTheDocument()
  })

  it('allows manually collapsing the active collection', async () => {
    persistOpenTree(['col-1'], ['item-1'])

    render(DocumentExplorer)

    expect(await screen.findByRole('treeitem', { name: 'Acta 1' })).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Colapsar colección Colección 1' }))

    expect(screen.getByRole('treeitem', { name: 'Colección 1' })).toHaveAttribute(
      'aria-expanded',
      'false'
    )
    expect(screen.getByRole('treeitem', { name: 'Colección 1' })).toHaveAttribute(
      'aria-selected',
      'true'
    )
    expect(screen.queryByRole('treeitem', { name: 'Acta 1' })).not.toBeInTheDocument()
  })

  it('allows manually collapsing the active item while keeping the selected asset', async () => {
    persistOpenTree(['col-1'], ['item-1'])

    render(DocumentExplorer)

    const assetSelectRequests: CustomEvent[] = []
    const handleAssetSelectRequest = (event: Event) => {
      assetSelectRequests.push(event as CustomEvent)
    }

    window.addEventListener(
      'entropia:document-explorer-asset-select-request',
      handleAssetSelectRequest
    )

    const assetButton = (await screen.findByText('acta-1.pdf')).closest('button')

    if (!assetButton) {
      throw new Error('Expected asset button to be rendered')
    }

    await fireEvent.click(assetButton)
    await fireEvent.click(screen.getByRole('button', { name: 'Colapsar documento Acta 1' }))

    expect(assetSelectRequests.at(-1)?.detail).toEqual({ itemId: 'item-1', assetId: 'asset-1' })
    expect(screen.getByRole('treeitem', { name: 'Acta 1' })).toHaveAttribute(
      'aria-selected',
      'true'
    )
    expect(screen.queryByRole('treeitem', { name: 'acta-1.pdf' })).not.toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Expandir documento Acta 1' }))

    expect(await screen.findByRole('treeitem', { name: 'acta-1.pdf' })).toHaveAttribute(
      'aria-current',
      'true'
    )

    window.removeEventListener(
      'entropia:document-explorer-asset-select-request',
      handleAssetSelectRequest
    )
  })

  it('does not auto-expand a closed parent when navigation selects an item', async () => {
    render(DocumentExplorer)

    expect(await screen.findByRole('treeitem', { name: 'Colección 1' })).toHaveAttribute(
      'aria-expanded',
      'false'
    )
    expect(screen.queryByRole('treeitem', { name: 'Acta 1' })).not.toBeInTheDocument()

    state.snapshot.current = {
      name: 'item' as const,
      collectionId: 'col-1',
      collectionName: 'Colección 1',
      itemId: 'item-2',
      itemTitle: 'Acta 2',
    }
    state.snapshot.breadcrumb = ['Colecciones', 'Colección 1', 'Acta 2']
    state.emit()

    await waitFor(() => {
      expect(state.store.items.findByCollection).toHaveBeenCalledWith('col-1')
    })
    expect(screen.getByRole('treeitem', { name: 'Colección 1' })).toHaveAttribute(
      'aria-expanded',
      'false'
    )
    expect(screen.queryByRole('treeitem', { name: 'Acta 2' })).not.toBeInTheDocument()
  })

  it('does not auto-expand a closed item when an asset becomes selected', async () => {
    persistOpenTree(['col-1'])

    render(DocumentExplorer)

    expect(await screen.findByRole('treeitem', { name: 'Acta 1' })).toHaveAttribute(
      'aria-expanded',
      'false'
    )

    window.dispatchEvent(
      new CustomEvent('entropia:document-explorer-asset-selected', {
        detail: { itemId: 'item-1', assetId: 'asset-1' },
      })
    )

    expect(screen.getByRole('treeitem', { name: 'Acta 1' })).toHaveAttribute(
      'aria-expanded',
      'false'
    )
    expect(screen.queryByRole('treeitem', { name: 'acta-1.pdf' })).not.toBeInTheDocument()
  })

  it('persists expanded nodes and restores them without auto-expanding the active path', async () => {
    persistOpenTree(['col-2'], ['item-3'])

    render(DocumentExplorer)

    expect(await screen.findByRole('treeitem', { name: 'Acta 3' })).toBeInTheDocument()
    expect(screen.queryByRole('treeitem', { name: 'Acta 1' })).not.toBeInTheDocument()
    expect(screen.queryByRole('treeitem', { name: 'acta-1.pdf' })).not.toBeInTheDocument()

    await waitFor(() => {
      expect(state.store.items.findByCollection).toHaveBeenCalledWith('col-2')
      expect(state.store.assets.findByItem).toHaveBeenCalledWith('item-3')
    })
  })

  it('renders centralized svg icons for explorer controls and nodes', async () => {
    persistOpenTree(['col-1'], ['item-1'])

    const { container } = render(DocumentExplorer)

    await screen.findByText('Colección 1')
    await screen.findByText('Acta 1')
    await screen.findByText('acta-1.pdf')
    await screen.findByText('acta-1-audio.mp3')

    const collectionButton = (await screen.findByText('Colección 1')).closest('button')
    const itemButton = (await screen.findByText('Acta 1')).closest('button')
    const pdfAssetButton = (await screen.findByText('acta-1.pdf')).closest('button')
    const audioAssetButton = (await screen.findByText('acta-1-audio.mp3')).closest('button')

    if (!collectionButton || !itemButton || !pdfAssetButton || !audioAssetButton) {
      throw new Error('Expected explorer node buttons to be rendered')
    }

    expect(collectionButton.querySelector('svg')).not.toBeNull()
    expect(itemButton.querySelector('svg')).not.toBeNull()
    expect(pdfAssetButton.querySelector('svg')).not.toBeNull()
    expect(audioAssetButton.querySelector('svg')).not.toBeNull()
    expect(container.querySelectorAll('svg').length).toBeGreaterThanOrEqual(7)
  })

  it('renders centralized svg icons for flattened image assets', async () => {
    persistOpenTree(['col-1'])

    render(DocumentExplorer)

    await screen.findByText('Acta 2')

    const imageAssetButton = (await screen.findByText('Acta 2')).closest('button')

    if (!imageAssetButton) {
      throw new Error('Expected image asset button to be rendered')
    }

    expect(imageAssetButton.querySelector('svg')).not.toBeNull()
    expect(screen.getByText('image')).toBeInTheDocument()
  })

  it('refreshes cached collection items and counts when the collection changes', async () => {
    persistOpenTree(['col-1'])

    render(DocumentExplorer)

    await screen.findByText('Acta 2')
    expect(screen.getByText('2')).toBeInTheDocument()

    state.store.items.findByCollection.mockImplementation(async (collectionId: string) => {
      if (collectionId === 'col-1') {
        return [
          {
            id: 'item-4',
            title: 'Acta 4',
            collectionId: 'col-1',
            metadata: null,
            createdAt: 1,
            updatedAt: 4,
          },
        ]
      }

      return [
        {
          id: 'item-3',
          title: 'Acta 3',
          collectionId: 'col-2',
          metadata: null,
          createdAt: 1,
          updatedAt: 3,
        },
      ]
    })
    state.store.collections.countItems.mockImplementation(async (id: string) =>
      id === 'col-1' ? 1 : 1
    )

    window.dispatchEvent(
      new CustomEvent('entropia:document-explorer-collection-changed', {
        detail: { collectionId: 'col-1', itemId: 'item-1' },
      })
    )

    expect(await screen.findByText('Acta 4')).toBeInTheDocument()
    expect(screen.queryByText('Acta 1')).not.toBeInTheDocument()
    expect(screen.queryByText('Acta 2')).not.toBeInTheDocument()
    expect(screen.queryByText('2')).not.toBeInTheDocument()
  })
})
