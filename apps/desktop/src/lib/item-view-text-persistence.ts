export type AssetAnalysisJob = [name: string, run: () => Promise<unknown>]

type Timer = ReturnType<typeof setTimeout>

export class DebouncedAssetReanalysisScheduler {
  private timers = new Map<string, Timer>()

  constructor(
    private readonly options: {
      delayMs: number
      getJobs: (assetId: string) => AssetAnalysisJob[]
      onStart?: (assetId: string) => void
      onJobError?: (jobName: string, reason: unknown) => void
    }
  ) {}

  schedule(assetId: string) {
    this.cancel(assetId)

    const timer = setTimeout(async () => {
      const jobs = this.options.getJobs(assetId)

      try {
        this.options.onStart?.(assetId)
        const results = await Promise.allSettled(jobs.map(([, run]) => run()))
        results.forEach((result, index) => {
          if (result.status === 'rejected') {
            this.options.onJobError?.(jobs[index]?.[0] ?? 'unknown', result.reason)
          }
        })
      } finally {
        this.timers.delete(assetId)
      }
    }, this.options.delayMs)

    this.timers.set(assetId, timer)
  }

  cancel(assetId: string) {
    const existing = this.timers.get(assetId)
    if (existing) {
      clearTimeout(existing)
      this.timers.delete(assetId)
    }
  }

  cancelAll() {
    for (const timer of this.timers.values()) {
      clearTimeout(timer)
    }
    this.timers.clear()
  }
}

export class DebouncedAssetTextPersistor {
  private timers = new Map<string, Timer>()

  constructor(
    private readonly options: {
      delayMs: number
      persist: (assetId: string, text: string) => Promise<unknown>
      afterPersist?: (assetId: string) => void
      onError?: (error: unknown) => void
    }
  ) {}

  schedule(assetId: string, text: string) {
    this.cancel(assetId)

    const timer = setTimeout(async () => {
      try {
        await this.options.persist(assetId, text)
        this.options.afterPersist?.(assetId)
      } catch (error) {
        this.options.onError?.(error)
      } finally {
        this.timers.delete(assetId)
      }
    }, this.options.delayMs)

    this.timers.set(assetId, timer)
  }

  cancel(assetId: string) {
    const existing = this.timers.get(assetId)
    if (existing) {
      clearTimeout(existing)
      this.timers.delete(assetId)
    }
  }

  cancelAll() {
    for (const timer of this.timers.values()) {
      clearTimeout(timer)
    }
    this.timers.clear()
  }
}
