import Modal from './Modal'

interface ConfirmDialogProps {
  title: string
  message: string
  confirmLabel: string
  cancelLabel?: string
  /** 危险操作（如退出登录）用强调色按钮。 */
  danger?: boolean
  onConfirm: () => void
  onCancel: () => void
}

/** 确认对话框，替代浏览器自带的 window.confirm。 */
export default function ConfirmDialog({
  title,
  message,
  confirmLabel,
  cancelLabel = '取消',
  danger = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Modal
      title={title}
      onClose={onCancel}
      footer={
        <>
          <button
            type="button"
            className={`btn ${danger ? 'btn-danger' : 'btn-primary'}`}
            aria-label={confirmLabel}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
          <button type="button" className="btn" aria-label={cancelLabel} onClick={onCancel}>
            {cancelLabel}
          </button>
        </>
      }
    >
      <p className="confirm-message">{message}</p>
    </Modal>
  )
}
