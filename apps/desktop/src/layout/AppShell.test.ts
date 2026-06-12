import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fireEvent, render, screen } from '@testing-library/svelte'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AppShellHost from './__fixtures__/AppShellHost.svelte'
import { locale } from '$lib/i18n'

type EventListenerCallback = (event: { payload: unknown }) => void

const { invokeMock, listenMock, navigationStore, storeRef } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn<(eventName: string, callback: EventListenerCallback) => Promise<() => void>>(
    () => Promise.resolve(vi.fn()),
  ),
  navigationStore: {
    subscribe(run: (value: unknown) => void) {
      run({
        history: [{ name: 'collections' }],
        current: { name: 'collections' },
        canGoBack: false,
        breadcrumb: ['Collections'],
      })
      return () => {}
    },
  },
  storeRef: {
    current: {
      collections: {
        findAll: vi.fn().mockResolvedValue([]),
        countItems: vi.fn().mockResolvedValue(0),
        findById: vi.fn().mockResolvedValue(null),
      },
      assets: { findByItem: vi.fn().mockResolvedValue([]) },
      items: {
        searchGlobal: vi.fn().mockResolvedValue([]),
        findByCollection: vi.fn().mockResolvedValue([]),
      },
    },
  },
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: listenMock,
}))

vi.mock('$lib/navigation', () => ({
  navigation: {
    subscribe: navigationStore.subscribe,
    navigate: vi.fn(),
    back: vi.fn(),
  },
}))

vi.mock('$lib/db', () => ({
  getStore: () => storeRef.current,
}))

describe('AppShell', () => {
  beforeEach(() => {
    locale.set('es')
    invokeMock.mockReset().mockImplementation((command: string) => {
      if (command === 'deps_get_cached_statuses') {
        return Promise.resolve([])
      }

      if (command === 'runtime_get_status') {
        return Promise.resolve({
          state: 'healthy',
          packVersion: null,
          repairNeeded: false,
          repairAvailable: false,
          summary: 'Runtime listo',
          blockedCapabilities: [],
          details: [],
          guidance: [],
          bootstrapEligible: false,
          bootstrapRequired: false,
          activeOperation: null,
        })
      }

      return Promise.resolve(undefined)
    })
    listenMock.mockClear().mockImplementation(() => Promise.resolve(vi.fn()))
    storeRef.current.items.searchGlobal.mockClear()
    storeRef.current.items.findByCollection.mockClear()
    storeRef.current.collections.findAll.mockClear()
    storeRef.current.collections.countItems.mockClear()
    storeRef.current.assets.findByItem.mockClear()
    storeRef.current.collections.findById.mockClear()
  })

  it('renders the app frame, visible footer actions, and projected content', () => {
    render(AppShellHost)

    expect(screen.getByRole('navigation', { name: 'Breadcrumb' })).toBeInTheDocument()
    expect(screen.getByTestId('app-shell-child')).toHaveTextContent('Contenido de prueba')
    expect(screen.getByText('EntropIA Lite β')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'GitHub' })).toBeInTheDocument()
    expect(screen.getByText('Desarrollado por')).toBeInTheDocument()
  })

  it('keeps the entropic constellation visible behind workspace surfaces', () => {
    const source = readFileSync(resolve(import.meta.dirname, 'AppShell.svelte'), 'utf-8')

    expect(source).toContain('<EntropicConstellation />')
    expect(source).toContain('color-mix(in srgb, var(--surface-app) 72%, transparent)')
    expect(source).toContain('color-mix(in srgb, var(--surface-app) 42%, transparent)')
  })

  it('keeps footer typography enlarged for readability', () => {
    const source = readFileSync(resolve(import.meta.dirname, 'AppShell.svelte'), 'utf-8')

    expect(source).toContain('font-size: var(--font-size-sm);')
    expect(source).toContain('font-size: calc(0.58rem + 3px);')
  })

  it('delegates open explorer width and border ownership to DocumentExplorer', () => {
    const source = readFileSync(resolve(import.meta.dirname, 'AppShell.svelte'), 'utf-8')
    const sidebarRule = source.match(/\.sidebar \{(?<body>[\s\S]*?)\n {2}\}/)?.groups?.body ?? ''

    expect(sidebarRule).not.toContain('width: 240px')
    expect(sidebarRule).not.toContain('border-right')
    expect(sidebarRule).not.toContain('overflow: hidden')
    expect(sidebarRule).toContain('flex: 0 0 auto;')
  })

  it('opens external links through the desktop bridge', async () => {
    render(AppShellHost)

    await fireEvent.click(screen.getByRole('link', { name: 'GitHub' }))
    expect(invokeMock).toHaveBeenCalledWith('open_external_url', {
      url: 'https://github.com/hlabrepo/EntropIA-Lite',
    })

    await fireEvent.click(screen.getByRole('link', { name: 'HLab' }))
    expect(invokeMock).toHaveBeenCalledWith('open_external_url', {
      url: 'https://hlab.com.ar/',
    })
  })

  it('toggles the sidebar with Ctrl+B except when typing in editable targets', async () => {
    render(AppShellHost)

    const editable = document.createElement('div')
    editable.setAttribute('contenteditable', 'true')
    document.body.appendChild(editable)

    try {
      // Plain Ctrl+B collapses the sidebar.
      await fireEvent.keyDown(document.body, { key: 'b', ctrlKey: true })
      expect(screen.getByRole('button', { name: 'Expandir panel (Ctrl+B)' })).toBeInTheDocument()

      // Ctrl+B from a contenteditable surface (e.g. the note editor) is ignored.
      await fireEvent.keyDown(editable, { key: 'b', ctrlKey: true })
      expect(screen.getByRole('button', { name: 'Expandir panel (Ctrl+B)' })).toBeInTheDocument()

      // Plain Ctrl+B expands it again.
      await fireEvent.keyDown(document.body, { key: 'b', ctrlKey: true })
      expect(screen.getByRole('button', { name: 'Colapsar panel (Ctrl+B)' })).toBeInTheDocument()

      // Ctrl+B from a text input is ignored too.
      await fireEvent.click(screen.getByRole('button', { name: 'Filtrar colecciones' }))
      const filterInput = screen.getByPlaceholderText('Filtrar colecciones...')
      await fireEvent.keyDown(filterInput, { key: 'b', ctrlKey: true })
      expect(screen.getByRole('button', { name: 'Colapsar panel (Ctrl+B)' })).toBeInTheDocument()
    } finally {
      editable.remove()
    }
  })

  it('reacts to locale changes in footer and sidebar copy', async () => {
    render(AppShellHost)

    locale.set('en')

    expect(await screen.findByText('Archive, OCR, and assisted analysis.')).toBeInTheDocument()
    expect(screen.getByText('Developed by')).toBeInTheDocument()
    expect(screen.getByRole('complementary', { name: 'Sidebar' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Collapse sidebar (Ctrl+B)' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New collection' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Filter collections' })).toBeInTheDocument()
    expect(screen.getByText('Open a collection to view the explorer')).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Filter collections' }))

    expect(screen.getByPlaceholderText('Filter collections...')).toBeInTheDocument()
  })

})
