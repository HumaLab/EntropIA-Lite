import { describe, expect, it } from 'vitest'
import { buildProcessingChecklist, type BuildProcessingChecklistOptions } from './processing-checklist'

const pdfAsset = {
  id: 'asset-1',
  itemId: 'item-1',
  path: 'docs/acta.pdf',
  type: 'pdf' as const,
  createdAt: 1,
  sortIndex: 0,
  size: 1024,
}

const audioAsset = {
  ...pdfAsset,
  id: 'asset-audio',
  path: 'audio/test.mp3',
  type: 'audio' as const,
}

function baseOptions(overrides: Partial<BuildProcessingChecklistOptions> = {}): BuildProcessingChecklistOptions {
  return {
    selectedAsset: pdfAsset,
    ocrState: { status: 'idle', progress: 0 },
    ocrEditedText: '',
    transcriptionState: null,
    transcriptionEditedText: '',
    llmState: { status: 'idle', activeJob: null, result: null, error: null },
    nlpState: { fts: 'idle', embed: 'idle', ner: 'idle', triples: 'idle' },
    llmAvailable: true,
    ocrCorrected: false,
    currentSummary: null,
    isSummarizing: false,
    ...overrides,
  }
}

describe('buildProcessingChecklist', () => {
  it('blocks downstream PDF steps until extracted text exists', () => {
    const steps = buildProcessingChecklist(baseOptions())

    expect(steps.map((step) => [step.id, step.status, step.reasonKey])).toEqual([
      ['text', 'pending', undefined],
      ['ocrCorrection', 'blocked', 'item.processingBlocked.needsText'],
      ['summary', 'blocked', 'item.processingBlocked.needsText'],
      ['entitiesTriples', 'blocked', 'item.processingBlocked.needsText'],
      ['embeddings', 'blocked', 'item.processingBlocked.needsText'],
    ])
  })

  it('marks ready and pending PDF steps from existing asset state', () => {
    const steps = buildProcessingChecklist(
      baseOptions({
        ocrState: { status: 'done', progress: 100, textContent: 'texto' },
        ocrEditedText: 'texto',
        ocrCorrected: true,
        currentSummary: 'Resumen',
        nlpState: { fts: 'done', embed: 'done', ner: 'done', triples: 'idle' },
      })
    )

    expect(steps.map((step) => [step.id, step.status])).toEqual([
      ['text', 'ready'],
      ['ocrCorrection', 'ready'],
      ['summary', 'ready'],
      ['entitiesTriples', 'pending'],
      ['embeddings', 'ready'],
    ])
  })

  it('omits OCR correction for audio and uses transcription as the text gate', () => {
    const steps = buildProcessingChecklist(
      baseOptions({
        selectedAsset: audioAsset,
        ocrState: null,
        transcriptionState: { status: 'done', progress: 100, text: 'audio text' },
        transcriptionEditedText: 'audio text',
      })
    )

    expect(steps.map((step) => step.id)).toEqual(['text', 'summary', 'entitiesTriples', 'embeddings'])
    expect(steps.find((step) => step.id === 'summary')?.status).toBe('pending')
  })

  it('blocks LLM-dependent steps when OpenRouter is unavailable', () => {
    const steps = buildProcessingChecklist(
      baseOptions({
        ocrState: { status: 'done', progress: 100, textContent: 'texto' },
        ocrEditedText: 'texto',
        llmAvailable: false,
      })
    )

    expect(steps.find((step) => step.id === 'ocrCorrection')).toMatchObject({
      status: 'blocked',
      reasonKey: 'item.processingBlocked.needsLlm',
    })
    expect(steps.find((step) => step.id === 'summary')).toMatchObject({
      status: 'blocked',
      reasonKey: 'item.processingBlocked.needsLlm',
    })
    expect(steps.find((step) => step.id === 'entitiesTriples')).toMatchObject({
      status: 'blocked',
      reasonKey: 'item.processingBlocked.needsLlm',
    })
    expect(steps.find((step) => step.id === 'embeddings')).toMatchObject({
      status: 'blocked',
      reasonKey: 'item.processingBlocked.needsLlm',
    })
  })
})
