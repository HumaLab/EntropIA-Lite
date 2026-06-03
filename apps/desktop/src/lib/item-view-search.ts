export type HighlightSegment = {
  text: string
  isMatch: boolean
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

export function getFtsTerms(rawQuery: string): string[] {
  if (!rawQuery.trim()) return []

  const noOperators = rawQuery.replace(/\b(AND|OR|NOT|NEAR)\b/gi, ' ')
  const terms = noOperators
    .split(/\s+/)
    .map((token) => token.replace(/[()"\-*^:,./\\]/g, '').trim())
    .filter((token) => token.length > 0)

  return Array.from(new Set(terms.map((token) => token.toLocaleLowerCase())))
}

export function splitHighlightedSegments(text: string, rawQuery: string): HighlightSegment[] {
  const terms = getFtsTerms(rawQuery)
  if (terms.length === 0 || !text) return [{ text, isMatch: false }]

  const pattern = terms
    .slice()
    .sort((a, b) => b.length - a.length)
    .map((term) => escapeRegExp(term))
    .join('|')

  if (!pattern) return [{ text, isMatch: false }]

  const regex = new RegExp(pattern, 'gi')
  const segments: HighlightSegment[] = []
  let lastIndex = 0

  for (const match of text.matchAll(regex)) {
    const index = match.index ?? 0
    const value = match[0] ?? ''
    if (index > lastIndex) {
      segments.push({ text: text.slice(lastIndex, index), isMatch: false })
    }
    if (value) {
      segments.push({ text: value, isMatch: true })
    }
    lastIndex = index + value.length
  }

  if (lastIndex < text.length) {
    segments.push({ text: text.slice(lastIndex), isMatch: false })
  }

  return segments.length > 0 ? segments : [{ text, isMatch: false }]
}
