import { fireEvent, render, screen } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'
import ConfirmDialog from '../ConfirmDialog.svelte'

describe('ConfirmDialog', () => {
  const baseProps = {
    title: 'Delete item',
    message: 'This cannot be undone.',
    cancelLabel: 'Cancel',
    confirmLabel: 'Delete',
    oncancel: vi.fn(),
    onconfirm: vi.fn(),
  }

  it('renders an accessible modal dialog labelled by the title', () => {
    render(ConfirmDialog, { props: baseProps })

    const dialog = screen.getByRole('dialog', { name: 'Delete item' })
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(screen.getByText('This cannot be undone.')).toBeInTheDocument()
  })

  it('cancels when the overlay is clicked', async () => {
    const oncancel = vi.fn()
    const { container } = render(ConfirmDialog, { props: { ...baseProps, oncancel } })

    await fireEvent.click(container.querySelector('.confirm-dialog__overlay') as Element)

    expect(oncancel).toHaveBeenCalledOnce()
  })

  it('cancels Escape and stops propagation', async () => {
    const oncancel = vi.fn()
    const propagated = vi.fn()
    const dialogKeydown = new KeyboardEvent('keydown', {
      key: 'Escape',
      bubbles: true,
      cancelable: true,
    })
    const dialog = render(ConfirmDialog, { props: { ...baseProps, oncancel } }).getByRole('dialog')
    document.body.addEventListener('keydown', propagated)

    dialog.dispatchEvent(dialogKeydown)

    expect(oncancel).toHaveBeenCalledOnce()
    expect(propagated).not.toHaveBeenCalled()
    document.body.removeEventListener('keydown', propagated)
  })

  it('supports destructive icon-only confirmation', () => {
    render(ConfirmDialog, {
      props: {
        title: 'Delete asset',
        message: 'Delete file.pdf?',
        cancelLabel: 'Cancel',
        confirmIcon: 'delete',
        confirmAriaLabel: 'Delete asset',
        variant: 'destructive',
        oncancel: vi.fn(),
        onconfirm: vi.fn(),
      },
    })

    expect(screen.getByRole('button', { name: 'Delete asset' })).toHaveClass(
      'confirm-dialog__confirm-icon--destructive'
    )
  })

  it('displays error text as an alert', () => {
    render(ConfirmDialog, { props: { ...baseProps, error: 'Delete failed' } })

    expect(screen.getByRole('alert')).toHaveTextContent('Delete failed')
  })
})
