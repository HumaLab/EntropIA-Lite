import { initStore, type StoreApi } from '@entropia/store'
import { ensureSyncCapture } from '$lib/sync'

let _store: StoreApi | null = null

export async function initDb(): Promise<void> {
  _store = await initStore()
  // JS migrations have finished — install/self-heal the sync capture triggers
  // against the final schema (DESIGN §6.1). Best-effort: a sync bootstrap
  // failure must not block app startup.
  try {
    await ensureSyncCapture()
  } catch (error) {
    console.error('[sync] ensureSyncCapture failed:', error)
  }
}

export function getStore(): StoreApi {
  if (!_store) throw new Error('Store not initialized. Call initDb() first.')
  return _store
}
