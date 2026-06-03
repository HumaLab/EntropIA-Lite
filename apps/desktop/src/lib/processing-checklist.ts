import type { AssetOcrState } from './ocr'
import type { AssetTranscriptionState } from './transcription'
import type { ItemLlmState } from './llm'
import type { ItemNlpState } from './nlp'
import type { Asset } from '@entropia/store'

export type ProcessingChecklistStatus = 'ready' | 'pending' | 'blocked'

export type ProcessingChecklistStepId =
  | 'text'
  | 'ocrCorrection'
  | 'summary'
  | 'entitiesTriples'
  | 'embeddings'

export interface ProcessingChecklistStep {
  id: ProcessingChecklistStepId
  labelKey: string
  status: ProcessingChecklistStatus
  reasonKey?: string
}

export interface BuildProcessingChecklistOptions {
  selectedAsset: Asset | null
  ocrState: AssetOcrState | null
  ocrEditedText: string
  transcriptionState: AssetTranscriptionState | null
  transcriptionEditedText: string
  llmState: ItemLlmState
  nlpState: ItemNlpState
  llmAvailable: boolean
  ocrCorrected: boolean
  currentSummary: string | null
  isSummarizing: boolean
}

function isActive(status: string | undefined): boolean {
  return status === 'pending' || status === 'running'
}

function isTextReady(options: BuildProcessingChecklistOptions): boolean {
  const { selectedAsset, ocrState, ocrEditedText, transcriptionState, transcriptionEditedText } = options
  if (!selectedAsset) return false

  if (selectedAsset.type === 'audio') {
    return transcriptionState?.status === 'done' && transcriptionEditedText.trim().length > 0
  }

  return ocrState?.status === 'done' && ocrEditedText.trim().length > 0
}

function getTextStep(options: BuildProcessingChecklistOptions): ProcessingChecklistStep {
  const { selectedAsset, ocrState, transcriptionState } = options

  if (!selectedAsset) {
    return {
      id: 'text',
      labelKey: 'item.processingStep.text',
      status: 'blocked',
      reasonKey: 'item.processingBlocked.noAsset',
    }
  }

  const state = selectedAsset.type === 'audio' ? transcriptionState : ocrState
  if (state?.status === 'done') {
    return { id: 'text', labelKey: 'item.processingStep.text', status: 'ready' }
  }

  if (isActive(state?.status)) {
    return { id: 'text', labelKey: 'item.processingStep.text', status: 'pending' }
  }

  return { id: 'text', labelKey: 'item.processingStep.text', status: 'pending' }
}

export function buildProcessingChecklist(
  options: BuildProcessingChecklistOptions
): ProcessingChecklistStep[] {
  const { selectedAsset, llmState, nlpState, llmAvailable, ocrCorrected, currentSummary, isSummarizing } =
    options
  const textReady = isTextReady(options)
  const llmBusy = llmState.status === 'running' || llmState.status === 'pending'
  const steps: ProcessingChecklistStep[] = [getTextStep(options)]

  if (selectedAsset && selectedAsset.type !== 'audio') {
    steps.push(
      ocrCorrected
        ? { id: 'ocrCorrection', labelKey: 'item.processingStep.ocrCorrection', status: 'ready' }
        : !textReady
          ? {
              id: 'ocrCorrection',
              labelKey: 'item.processingStep.ocrCorrection',
              status: 'blocked',
              reasonKey: 'item.processingBlocked.needsText',
            }
          : !llmAvailable
            ? {
                id: 'ocrCorrection',
                labelKey: 'item.processingStep.ocrCorrection',
                status: 'blocked',
                reasonKey: 'item.processingBlocked.needsLlm',
              }
            : llmBusy
              ? { id: 'ocrCorrection', labelKey: 'item.processingStep.ocrCorrection', status: 'pending' }
              : { id: 'ocrCorrection', labelKey: 'item.processingStep.ocrCorrection', status: 'pending' }
    )
  }

  steps.push(
    currentSummary
      ? { id: 'summary', labelKey: 'item.processingStep.summary', status: 'ready' }
      : isSummarizing || (llmBusy && llmState.activeJob === 'summarize')
        ? { id: 'summary', labelKey: 'item.processingStep.summary', status: 'pending' }
        : !textReady
          ? {
              id: 'summary',
              labelKey: 'item.processingStep.summary',
              status: 'blocked',
              reasonKey: 'item.processingBlocked.needsText',
            }
          : !llmAvailable
            ? {
                id: 'summary',
                labelKey: 'item.processingStep.summary',
                status: 'blocked',
                reasonKey: 'item.processingBlocked.needsLlm',
              }
            : { id: 'summary', labelKey: 'item.processingStep.summary', status: 'pending' }
  )

  steps.push(
    nlpState.ner === 'done' && nlpState.triples === 'done'
      ? { id: 'entitiesTriples', labelKey: 'item.processingStep.entitiesTriples', status: 'ready' }
      : isActive(nlpState.ner) || isActive(nlpState.triples)
        ? { id: 'entitiesTriples', labelKey: 'item.processingStep.entitiesTriples', status: 'pending' }
        : !textReady
          ? {
              id: 'entitiesTriples',
              labelKey: 'item.processingStep.entitiesTriples',
              status: 'blocked',
              reasonKey: 'item.processingBlocked.needsText',
            }
          : !llmAvailable
            ? {
                id: 'entitiesTriples',
                labelKey: 'item.processingStep.entitiesTriples',
                status: 'blocked',
                reasonKey: 'item.processingBlocked.needsLlm',
              }
            : { id: 'entitiesTriples', labelKey: 'item.processingStep.entitiesTriples', status: 'pending' }
  )

  steps.push(
    nlpState.embed === 'done'
      ? { id: 'embeddings', labelKey: 'item.processingStep.embeddings', status: 'ready' }
      : isActive(nlpState.embed)
        ? { id: 'embeddings', labelKey: 'item.processingStep.embeddings', status: 'pending' }
        : !selectedAsset
          ? {
              id: 'embeddings',
              labelKey: 'item.processingStep.embeddings',
              status: 'blocked',
              reasonKey: 'item.processingBlocked.noAsset',
            }
          : !textReady
            ? {
                id: 'embeddings',
                labelKey: 'item.processingStep.embeddings',
                status: 'blocked',
                reasonKey: 'item.processingBlocked.needsText',
              }
            : !llmAvailable
              ? {
                  id: 'embeddings',
                  labelKey: 'item.processingStep.embeddings',
                  status: 'blocked',
                  reasonKey: 'item.processingBlocked.needsLlm',
                }
              : { id: 'embeddings', labelKey: 'item.processingStep.embeddings', status: 'pending' }
  )

  return steps
}
