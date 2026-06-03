import { describe, expect, it } from 'vitest'
import { getFtsTerms, splitHighlightedSegments } from './item-view-search'

describe('item view search helpers', () => {
  it('extracts unique lower-case FTS terms while removing operators and punctuation', () => {
    expect(getFtsTerms('Acta AND (OCR) NOT pdf:* acta')).toEqual(['acta', 'ocr', 'pdf'])
  })

  it('returns a single non-match segment for empty queries or empty text', () => {
    expect(splitHighlightedSegments('Acta secreta', '')).toEqual([
      { text: 'Acta secreta', isMatch: false },
    ])
    expect(splitHighlightedSegments('', 'acta')).toEqual([{ text: '', isMatch: false }])
  })

  it('splits text into highlighted and non-highlighted segments case-insensitively', () => {
    expect(splitHighlightedSegments('Acta secreta de archivo', 'secreta archivo')).toEqual([
      { text: 'Acta ', isMatch: false },
      { text: 'secreta', isMatch: true },
      { text: ' de ', isMatch: false },
      { text: 'archivo', isMatch: true },
    ])
  })

  it('prefers longer terms before shorter overlapping terms', () => {
    expect(splitHighlightedSegments('metadata meta', 'meta metadata')).toEqual([
      { text: 'metadata', isMatch: true },
      { text: ' ', isMatch: false },
      { text: 'meta', isMatch: true },
    ])
  })
})
