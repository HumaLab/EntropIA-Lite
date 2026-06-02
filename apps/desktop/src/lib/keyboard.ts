import { navigation } from './navigation'

/**
 * Global keyboard handler for the desktop app.
 * - Escape → navigate back
 * Returns a cleanup function that removes the listener.
 */
export function setupKeyboardShortcuts(): () => void {
  const handler = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && !shouldIgnoreGlobalEscape(e)) {
      navigation.back()
    }
  }
  window.addEventListener('keydown', handler)
  return () => window.removeEventListener('keydown', handler)
}

function shouldIgnoreGlobalEscape(e: KeyboardEvent): boolean {
  if (e.defaultPrevented) return true

  if (document.querySelector('[role="dialog"], [aria-modal="true"]')) {
    return true
  }

  const target = e.target instanceof Element ? e.target : null
  if (!target) return false

  const tagName = target.tagName.toLowerCase()
  return (
    tagName === 'input' ||
    tagName === 'textarea' ||
    tagName === 'select' ||
    target.closest('[contenteditable="true"]') !== null
  )
}
