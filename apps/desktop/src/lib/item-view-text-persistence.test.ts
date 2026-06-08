import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  DebouncedAssetReanalysisScheduler,
  DebouncedAssetTextPersistor,
} from './item-view-text-persistence'

describe('DebouncedAssetTextPersistor', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('persists the latest scheduled text after the debounce delay', async () => {
    const persist = vi.fn().mockResolvedValue(undefined)
    const afterPersist = vi.fn()
    const persistor = new DebouncedAssetTextPersistor({ delayMs: 500, persist, afterPersist })

    persistor.schedule('asset-1', 'old text')
    persistor.schedule('asset-1', 'new text')

    await vi.advanceTimersByTimeAsync(499)
    expect(persist).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)

    expect(persist).toHaveBeenCalledTimes(1)
    expect(persist).toHaveBeenCalledWith('asset-1', 'new text')
    expect(afterPersist).toHaveBeenCalledWith('asset-1', 'new text')
  })

  it('does not call afterPersist when persistence fails', async () => {
    const error = new Error('persist failed')
    const persist = vi.fn().mockRejectedValue(error)
    const afterPersist = vi.fn()
    const onError = vi.fn()
    const persistor = new DebouncedAssetTextPersistor({
      delayMs: 500,
      persist,
      afterPersist,
      onError,
    })

    persistor.schedule('asset-1', 'text')
    await vi.advanceTimersByTimeAsync(500)

    expect(afterPersist).not.toHaveBeenCalled()
    expect(onError).toHaveBeenCalledWith(error)
  })

  it('cancels all pending text persistence timers', async () => {
    const persist = vi.fn().mockResolvedValue(undefined)
    const persistor = new DebouncedAssetTextPersistor({ delayMs: 500, persist })

    persistor.schedule('asset-1', 'text')
    persistor.schedule('asset-2', 'other')
    persistor.cancelAll()
    await vi.advanceTimersByTimeAsync(500)

    expect(persist).not.toHaveBeenCalled()
  })
})

describe('DebouncedAssetReanalysisScheduler', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('runs all jobs after the debounce delay', async () => {
    const ner = vi.fn().mockResolvedValue(undefined)
    const fts = vi.fn().mockResolvedValue(undefined)
    const onStart = vi.fn()
    const scheduler = new DebouncedAssetReanalysisScheduler({
      delayMs: 1500,
      getJobs: (assetId) => [
        ['ner', () => ner(assetId)],
        ['fts', () => fts(assetId)],
      ],
      onStart,
    })

    scheduler.schedule('asset-1')
    await vi.advanceTimersByTimeAsync(1499)
    expect(ner).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)

    expect(onStart).toHaveBeenCalledWith('asset-1')
    expect(ner).toHaveBeenCalledWith('asset-1')
    expect(fts).toHaveBeenCalledWith('asset-1')
  })

  it('debounces repeated reanalysis schedules for the same asset', async () => {
    const run = vi.fn().mockResolvedValue(undefined)
    const scheduler = new DebouncedAssetReanalysisScheduler({
      delayMs: 1500,
      getJobs: () => [['ner', run]],
    })

    scheduler.schedule('asset-1')
    await vi.advanceTimersByTimeAsync(1000)
    scheduler.schedule('asset-1')
    await vi.advanceTimersByTimeAsync(1499)
    expect(run).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(run).toHaveBeenCalledTimes(1)
  })

  it('reports individual job failures without aborting remaining jobs', async () => {
    const failure = new Error('ner failed')
    const onJobError = vi.fn()
    const succeedingJob = vi.fn().mockResolvedValue(undefined)
    const scheduler = new DebouncedAssetReanalysisScheduler({
      delayMs: 1500,
      getJobs: () => [
        ['ner', () => Promise.reject(failure)],
        ['fts', succeedingJob],
      ],
      onJobError,
    })

    scheduler.schedule('asset-1')
    await vi.advanceTimersByTimeAsync(1500)

    expect(succeedingJob).toHaveBeenCalled()
    expect(onJobError).toHaveBeenCalledWith('ner', failure)
  })

  it('cancels all pending reanalysis timers', async () => {
    const run = vi.fn().mockResolvedValue(undefined)
    const scheduler = new DebouncedAssetReanalysisScheduler({
      delayMs: 1500,
      getJobs: () => [['ner', run]],
    })

    scheduler.schedule('asset-1')
    scheduler.schedule('asset-2')
    scheduler.cancelAll()
    await vi.advanceTimersByTimeAsync(1500)

    expect(run).not.toHaveBeenCalled()
  })
})
