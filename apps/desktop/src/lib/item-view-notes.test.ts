import { describe, expect, it } from 'vitest'
import { canCancelDelete, getNextExpandedNoteId, getNoteStateAfterDelete } from './item-view-notes'

describe('getNextExpandedNoteId', () => {
  it('expands the selected note when another note is expanded', () => {
    expect(getNextExpandedNoteId('note-1', 'note-2')).toBe('note-2')
  })

  it('collapses the selected note when it is already expanded', () => {
    expect(getNextExpandedNoteId('note-1', 'note-1')).toBeNull()
  })
})

describe('getNoteStateAfterDelete', () => {
  it('clears expanded, editing, and pending delete state for the deleted note', () => {
    expect(
      getNoteStateAfterDelete(
        {
          expandedNoteId: 'note-1',
          editingNoteId: 'note-1',
          pendingDeleteNoteId: 'note-1',
        },
        'note-1'
      )
    ).toEqual({
      expandedNoteId: null,
      editingNoteId: null,
      pendingDeleteNoteId: null,
    })
  })

  it('keeps unrelated expanded and editing state while clearing pending delete state', () => {
    expect(
      getNoteStateAfterDelete(
        {
          expandedNoteId: 'note-2',
          editingNoteId: 'note-3',
          pendingDeleteNoteId: 'note-1',
        },
        'note-1'
      )
    ).toEqual({
      expandedNoteId: 'note-2',
      editingNoteId: 'note-3',
      pendingDeleteNoteId: null,
    })
  })
})

describe('canCancelDelete', () => {
  it('allows cancelling while deletion is idle', () => {
    expect(canCancelDelete(false)).toBe(true)
  })

  it('blocks cancelling while deletion is in progress', () => {
    expect(canCancelDelete(true)).toBe(false)
  })
})
