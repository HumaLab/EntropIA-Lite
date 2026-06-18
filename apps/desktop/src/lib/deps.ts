/**
 * Lite dependency-status client.
 * Commands are retained for Tauri compatibility; Lite has no local dependency manager.
 */

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type DependencyId = string

export type DependencyStatus =
  | { type: 'unknown' }
  | { type: 'checking' }
  | { type: 'installed'; version?: string }
  | { type: 'missing' }
  | { type: 'installing'; percent: number }
  | { type: 'failed'; message: string }

export interface DepCheckResult {
  id: DependencyId
  status: DependencyStatus
  version: string | null
}

export interface UvStatusResult {
  uv_ready: boolean
  uv_path: string | null
  uv_version: string | null
  uv_source: string | null
  uv_compatible_for_dev: boolean
  venv_exists: boolean
  venv_path: string | null
  uv_warning: string | null
  release_runtime_ready: boolean
  release_runtime_state: string | null
  dev_fallback_available: boolean
  dev_fallback_reason: string | null
}

export interface DepsProgressEvent {
  id: DependencyId
  status: DependencyStatus
  message: string
}

export interface DepsCompleteEvent {
  results: DepCheckResult[]
  all_critical_installed: boolean
}

export interface DepsErrorEvent {
  stage: string
  error: string
  recoverable: boolean
}

function normalizeDepResults(results: DepCheckResult[] | null | undefined): DepCheckResult[] {
  return Array.isArray(results) ? results : []
}

// ---------------------------------------------------------------------------
// Invoke wrappers
// ---------------------------------------------------------------------------

export function checkAllDeps(): Promise<DepCheckResult[]> {
  return invoke<DepCheckResult[]>('deps_check_all').then(normalizeDepResults)
}

export function getCachedDepsStatuses(): Promise<DepCheckResult[]> {
  return invoke<DepCheckResult[]>('deps_get_cached_statuses').then(normalizeDepResults)
}

export function installAllDeps(): Promise<void> {
  return invoke<void>('deps_install_all')
}

export function installOneDep(id: DependencyId): Promise<DepCheckResult> {
  return invoke<DepCheckResult>('deps_install_one', { id })
}

export function getUvStatus(): Promise<UvStatusResult> {
  return invoke<UvStatusResult>('deps_get_uv_status')
}

export function resetDeps(): Promise<void> {
  return invoke<void>('deps_reset')
}

export const CRITICAL_DEPS: DependencyId[] = []

// ---------------------------------------------------------------------------
// Event listener helpers
// ---------------------------------------------------------------------------

export function onDepsProgress(callback: (event: DepsProgressEvent) => void): Promise<UnlistenFn> {
  return listen<DepsProgressEvent>('deps://progress', (e) => callback(e.payload))
}

export function onDepsComplete(callback: (event: DepsCompleteEvent) => void): Promise<UnlistenFn> {
  return listen<DepsCompleteEvent>('deps://complete', (e) => callback(e.payload))
}

export function onDepsError(callback: (event: DepsErrorEvent) => void): Promise<UnlistenFn> {
  return listen<DepsErrorEvent>('deps://error', (e) => callback(e.payload))
}
