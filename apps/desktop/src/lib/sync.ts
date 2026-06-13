/**
 * Cloud sync frontend client (DESIGN §6.1). For now this only exposes the
 * capture-bootstrap call; the rest of the sync surface (login, status, etc.)
 * lands in later slices.
 *
 * Plain TypeScript (not .svelte.ts) for full testability in Vitest. Talks to the
 * Rust backend via Tauri invoke.
 */

import { invoke } from '@tauri-apps/api/core'

/**
 * Ensures the local sync schema and the 45 capture triggers exist (DESIGN §6.1).
 * MUST be called right after `initStore()` resolves — that is the signal that
 * the JS migrations have finished, so the Rust side can install/self-heal the
 * triggers against the final schema. Idempotent and safe to call repeatedly.
 */
export async function ensureSyncCapture(): Promise<void> {
  await invoke('sync_ensure_capture')
}
