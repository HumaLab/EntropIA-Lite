/**
 * Cloud sync frontend client (DESIGN §11, PROTOCOL.md). Typed `invoke` wrappers
 * for every `sync_*` Tauri command plus a `SyncEventManager` for the `sync:status`
 * event stream.
 *
 * Plain TypeScript (not .svelte.ts) for full testability in Vitest. Talks to the
 * Rust backend via Tauri invoke + event listeners. Sync configuration lives ONLY
 * in `sync_meta` + the OS keyring (DESIGN §11): this module NEVER touches
 * `settingsGet`/`settingsSet`/`SETTINGS_KEYS`.
 *
 * The device token never crosses this boundary — it stays in the Rust keyring and
 * is never returned to the renderer (DESIGN §8).
 */

import { invoke } from '@tauri-apps/api/core'
import { t } from './i18n'

// ─────────────────────────────────────────────────────────────────────────────
// Status — the `sync:status` event payload AND the `sync_status` command return.
// Field names mirror the Rust `SyncStatus` serde shape (snake_case on the wire).
// ─────────────────────────────────────────────────────────────────────────────

/**
 * The engine state machine (DESIGN §11). `disabled` renders nothing in the UI
 * (opt-in); the rest map to the status indicator.
 */
export type SyncState = 'disabled' | 'idle' | 'syncing' | 'offline' | 'error'

/** The full `sync:status` payload (DESIGN §11, PROTOCOL flow step 9). */
export interface SyncStatus {
  state: SyncState
  /** Last successful sync, ms since epoch; `null` until the first success. */
  last_sync_at: number | null
  /** Coalesced count of dirty rows awaiting push (distinct `(table, row_id)`). */
  pending: number
  /** Rows awaiting blob download (`COUNT(sync_pending_blobs)`). */
  blobs_pending: number
  /** Estimated bytes of own blobs not yet uploaded (first-sync preflight). */
  pending_blob_bytes: number
  /** Unacknowledged conflicts. */
  conflicts: number
  /** True when |clock offset| exceeds 5 min (non-blocking warning). */
  clock_warning: boolean
  /** Human message for the `error` state (426/clock_skew/507 mapping); optional. */
  message?: string
}

// ─────────────────────────────────────────────────────────────────────────────
// Device / usage / conflict DTOs (mirror the Rust serde shapes — snake_case)
// ─────────────────────────────────────────────────────────────────────────────

/** One device in the account's device list (PROTOCOL `GET /v1/devices`). */
export interface SyncDevice {
  id: string
  name: string
  platform: string
  created_at: number
  last_seen_at: number
  revoked: boolean
  current: boolean
}

/** Account storage usage (PROTOCOL `GET /v1/usage`). */
export interface SyncUsage {
  rows: number
  blobs_count: number
  blobs_bytes: number
  quota_bytes: number
  /** Plan / subscription type name (e.g. `Free`, `5 GB`); null if no plan assigned. */
  plan_name: string | null
}

/** One conflict journal entry surfaced to the UI (DESIGN §6 schema). */
export interface SyncConflict {
  id: string
  table_name: string
  row_id: string
  reason: string
  loser_payload: string | null
  winner_summary: string | null
  created_at: number
  acknowledged: boolean
}

// ─────────────────────────────────────────────────────────────────────────────
// Capture bootstrap (DESIGN §6.1)
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Ensures the local sync schema and the 45 capture triggers exist (DESIGN §6.1).
 * MUST be called right after `initStore()` resolves — that is the signal that
 * the JS migrations have finished, so the Rust side can install/self-heal the
 * triggers against the final schema. Idempotent and safe to call repeatedly.
 */
export async function ensureSyncCapture(): Promise<void> {
  await invoke('sync_ensure_capture')
}

// ─────────────────────────────────────────────────────────────────────────────
// Session — register / login / logout (DESIGN §8, §6.3)
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Registers a new account on the server (PROTOCOL `POST /v1/auth/register`).
 * Returns the new `account_id`. Gated server-side by `SYNC_REGISTRATION_OPEN`;
 * surfaces server errors (e.g. `registration_closed`, `email_taken`).
 */
export function syncRegisterAccount(
  serverUrl: string,
  email: string,
  password: string
): Promise<string> {
  return invoke<string>('sync_register_account', { serverUrl, email, password })
}

/**
 * Logs in (PROTOCOL `POST /v1/auth/login`): creates a fresh device, stores the
 * token in the keyring, persists the session in `sync_meta`, and turns capture on.
 * The token never returns to the renderer (DESIGN §8).
 */
export function syncLogin(serverUrl: string, email: string, password: string): Promise<void> {
  return invoke<void>('sync_login', { serverUrl, email, password })
}

/**
 * Logs out (DESIGN §6.3): best-effort remote revoke then a full local wipe of
 * the sync state and the keyring token. Local app data is untouched.
 */
export function syncLogout(): Promise<void> {
  return invoke<void>('sync_logout')
}

// ─────────────────────────────────────────────────────────────────────────────
// Status + manual sync + auto-sync (DESIGN §11, §3.1)
// ─────────────────────────────────────────────────────────────────────────────

/** Returns the current status snapshot for UI bootstrap (DESIGN §11). */
export function syncStatus(): Promise<SyncStatus> {
  return invoke<SyncStatus>('sync_status')
}

/**
 * Triggers a manual sync run (DESIGN §3.1). Returns the current status snapshot
 * immediately; the engine coalesces concurrent requests into at most one run.
 */
export function syncNow(): Promise<SyncStatus> {
  return invoke<SyncStatus>('sync_now')
}

/**
 * Sets the auto-sync toggle + interval (DESIGN §11). `intervalMin` is clamped to
 * ≥ 1 on the Rust side.
 */
export function syncSetAuto(enabled: boolean, intervalMin: number): Promise<void> {
  return invoke<void>('sync_set_auto', { enabled, intervalMin })
}

// ─────────────────────────────────────────────────────────────────────────────
// Devices (PROTOCOL `GET/DELETE /v1/devices`)
// ─────────────────────────────────────────────────────────────────────────────

/** Lists the account's devices. Always resolves to an array. */
export function syncListDevices(): Promise<SyncDevice[]> {
  return invoke<SyncDevice[]>('sync_list_devices').then((r) => (Array.isArray(r) ? r : []))
}

/** Revokes another device by id. A device cannot revoke itself (use logout). */
export function syncRevokeDevice(deviceId: string): Promise<void> {
  return invoke<void>('sync_revoke_device', { deviceId })
}

// ─────────────────────────────────────────────────────────────────────────────
// Conflicts journal (DESIGN §11)
// ─────────────────────────────────────────────────────────────────────────────

/** Lists conflict journal entries newest-first, paginated. Always resolves to an array. */
export function syncListConflicts(limit?: number, offset?: number): Promise<SyncConflict[]> {
  return invoke<SyncConflict[]>('sync_list_conflicts', { limit, offset }).then((r) =>
    Array.isArray(r) ? r : []
  )
}

/** Acknowledges a conflict by id so it drops out of the unacknowledged count. */
export function syncAckConflict(conflictId: string): Promise<void> {
  return invoke<void>('sync_ack_conflict', { conflictId })
}

// ─────────────────────────────────────────────────────────────────────────────
// Usage + account deletion + blob re-verify
// ─────────────────────────────────────────────────────────────────────────────

/** Returns the account's storage usage (PROTOCOL `GET /v1/usage`). */
export function syncGetUsage(): Promise<SyncUsage> {
  return invoke<SyncUsage>('sync_get_usage')
}

/**
 * Deletes the account's server-side data (PROTOCOL `DELETE /v1/account`, re-auth
 * with password). On success the local sync state is wiped; local app data stays.
 */
export function syncDeleteAccount(password: string): Promise<void> {
  return invoke<void>('sync_delete_account', { password })
}

/**
 * Resets `uploaded=0` on own blobs, forcing a re-HEAD/re-PUT (DESIGN §7). Repopulates
 * a restored server, since HEAD answers from the filesystem, not the table.
 */
export function syncReverifyBlobs(): Promise<void> {
  return invoke<void>('sync_reverify_blobs')
}

// ─────────────────────────────────────────────────────────────────────────────
// Error mapping (DESIGN §11 / PROTOCOL "Errores")
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Maps a thrown sync error to a human i18n message. The Rust commands surface
 * errors as a `String` (`SyncError::Display`), shaped like
 * `"api error {status} ({code}): {message}"` for server errors. We branch on the
 * stable HTTP status / error code (PROTOCOL "Errores"), falling back to the raw
 * message when no specific mapping applies.
 */
export function describeSyncError(error: unknown): string {
  const raw =
    typeof error === 'string'
      ? error
      : error instanceof Error
        ? error.message
        : ''
  const lower = raw.toLowerCase()

  // HTTP status / stable code mapping (DESIGN §11, PROTOCOL "Errores").
  if (lower.includes('426') || lower.includes('schema_upgrade_required')) return t('sync.error.426')
  if (lower.includes('507') || lower.includes('insufficient_storage')) return t('sync.error.507')
  if (lower.includes('clock_skew')) return t('sync.error.clockSkew')
  if (lower.includes('account_suspended')) return t('sync.error.accountSuspended')
  if (lower.includes('subscription_expired')) return t('sync.error.subscriptionExpired')
  if (lower.includes('registration_closed')) return t('sync.error.registrationClosed')
  if (lower.includes('email_taken')) return t('sync.error.emailTaken')
  if (lower.includes('401') || lower.includes('unauthorized')) return t('sync.error.unauthorized')

  // Otherwise surface the backend message when present, else a generic fallback.
  return raw.trim() || t('sync.error.generic')
}

// ─────────────────────────────────────────────────────────────────────────────
// SyncEventManager — generation-counter listener manager (mirrors ocr.ts)
// ─────────────────────────────────────────────────────────────────────────────

/** The Tauri event carrying every status transition + cycle end (DESIGN §11). */
export const SYNC_STATUS_EVENT = 'sync:status'

/**
 * Subscribes to the backend `sync:status` event stream and forwards each payload
 * to the supplied callback. The `listen` function is injected (from
 * `@tauri-apps/api/event`) for testability — mirrors the OcrStore listener idiom
 * exactly, including the generation-counter cleanup that unlistens late
 * registrations instead of leaking them.
 */
export class SyncEventManager {
  private cleanupFns: Array<() => void> = []
  private listenGeneration = 0
  private onStatus: (status: SyncStatus) => void

  constructor(onStatus: (status: SyncStatus) => void) {
    this.onStatus = onStatus
  }

  /** Registers the `sync:status` listener. */
  async startListening(
    listen: (
      event: string,
      callback: (e: { payload: unknown }) => void
    ) => Promise<() => void>
  ): Promise<void> {
    const generation = ++this.listenGeneration

    const unlistenStatus = await listen(SYNC_STATUS_EVENT, (e) => {
      this.onStatus(e.payload as SyncStatus)
    })

    const cleanupFns = [unlistenStatus]

    // stopListening may run while the listen() promise above is still in flight;
    // unlisten late registrations immediately instead of leaking them.
    if (generation !== this.listenGeneration) {
      for (const fn of cleanupFns) {
        fn()
      }
      return
    }

    this.cleanupFns = cleanupFns
  }

  /** Calls all cleanup functions returned by listen(), removing event listeners. */
  stopListening(): void {
    this.listenGeneration++
    for (const fn of this.cleanupFns) {
      fn()
    }
    this.cleanupFns = []
  }
}
