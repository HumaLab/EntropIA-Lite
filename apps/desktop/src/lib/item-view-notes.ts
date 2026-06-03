export type NoteUiState = {
  expandedNoteId: string | null
  editingNoteId: string | null
  pendingDeleteNoteId: string | null
}

export function getNextExpandedNoteId(currentExpandedNoteId: string | null, noteId: string) {
  return currentExpandedNoteId === noteId ? null : noteId
}

export function getNoteStateAfterDelete(state: NoteUiState, deletedNoteId: string): NoteUiState {
  return {
    expandedNoteId: state.expandedNoteId === deletedNoteId ? null : state.expandedNoteId,
    editingNoteId: state.editingNoteId === deletedNoteId ? null : state.editingNoteId,
    pendingDeleteNoteId: null,
  }
}

export function canCancelDelete(deletingNote: boolean) {
  return !deletingNote
}
