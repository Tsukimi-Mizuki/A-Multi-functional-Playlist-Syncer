import { useEffect, useId, useRef, type ReactNode } from 'react'

interface ModalProps {
  /** 无障碍名称，同时显示为标题。 */
  title: string
  onClose: () => void
  children: ReactNode
  /** 底部按钮区。 */
  footer?: ReactNode
}

/** 模态对话框：半透明玻璃卡片 + 背景虚化；Esc / 点击遮罩关闭，打开时焦点落在对话框内。 */
export default function Modal({ title, onClose, children, footer }: ModalProps) {
  const titleId = useId()
  const dialogRef = useRef<HTMLDivElement>(null)
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  })

  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const dialog = dialogRef.current
    // 焦点优先给对话框里第一个按钮，没有则给对话框本身。
    const target = dialog?.querySelector<HTMLElement>('button:not([disabled])') ?? dialog
    target?.focus()

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation()
        onCloseRef.current()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      previous?.focus()
    }
  }, [])

  return (
    <div
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    >
      <div
        ref={dialogRef}
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-label={title}
        tabIndex={-1}
      >
        <h2 id={titleId} className="modal-title">
          {title}
        </h2>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-footer">{footer}</div>}
      </div>
    </div>
  )
}
