import { useEffect, useId, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { motionReduced } from '../state/appearance'

/** 当前打开的模态对话框（后打开的在末尾）：Esc 只关最上面一个，编辑器上叠着批量窗口 / 确认框时不会一起关掉。 */
const openModals: symbol[] = []

/** 退场动画的最长时长；动画事件没触发（例如窗口最小化）时按这个时间兜底移除残影。 */
const LEAVE_FALLBACK_MS = 400

/**
 * 对话框被卸载时留下一个静态“残影”播放退场动画。
 *
 * 调用方都是直接卸载对话框（`{open && <Dialog/>}`），React 不会等动画播完；
 * 这里复制卸载前的 DOM 挂到 body 上淡出，播完即删，各个对话框不用为了退场动画改写。
 * 严格模式下的模拟卸载不会真的移除节点，所以只有节点确实离开文档时才生成残影。
 */
function leaveWithGhost(backdrop: HTMLElement): void {
  if (motionReduced()) return
  const ghost = backdrop.cloneNode(true) as HTMLElement
  queueMicrotask(() => {
    if (backdrop.isConnected) return
    ghost.classList.add('modal-leaving')
    ghost.setAttribute('aria-hidden', 'true')
    ghost.inert = true
    document.body.appendChild(ghost)
    const remove = () => ghost.remove()
    ghost.addEventListener('animationend', (event) => {
      if (event.target === ghost) remove()
    })
    window.setTimeout(remove, LEAVE_FALLBACK_MS)
  })
}

interface ModalProps {
  /** 无障碍名称，同时显示为标题。 */
  title: string
  onClose: () => void
  children: ReactNode
  /** 底部按钮区。 */
  footer?: ReactNode
  /** 额外的样式类（例如 `modal-wide` 加宽）。 */
  className?: string
}

/** 模态对话框：半透明玻璃卡片 + 背景虚化；Esc / 点击遮罩关闭，打开时焦点落在对话框内。 */
export default function Modal({ title, onClose, children, footer, className }: ModalProps) {
  const titleId = useId()
  const dialogRef = useRef<HTMLDivElement>(null)
  const backdropRef = useRef<HTMLDivElement>(null)
  const onCloseRef = useRef(onClose)
  useEffect(() => {
    onCloseRef.current = onClose
  })

  useEffect(() => {
    const token = Symbol('modal')
    openModals.push(token)
    const backdrop = backdropRef.current
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const dialog = dialogRef.current
    // 焦点优先给对话框里第一个按钮，没有则给对话框本身。
    const target = dialog?.querySelector<HTMLElement>('button:not([disabled])') ?? dialog
    target?.focus()

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && openModals[openModals.length - 1] === token) {
        event.stopPropagation()
        onCloseRef.current()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      const index = openModals.indexOf(token)
      if (index >= 0) openModals.splice(index, 1)
      previous?.focus()
      if (backdrop) leaveWithGhost(backdrop)
    }
  }, [])

  // 挂到 body 上：对话框带 backdrop-filter，若叠放的对话框（编辑器上的批量窗口、确认框）嵌在它里面，
  // 会被它截成局部的定位容器，也只能模糊到它为止；放在 body 下就都是覆盖整个窗口的同级层。
  return createPortal(
    <div
      ref={backdropRef}
      className="modal-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    >
      <div
        ref={dialogRef}
        className={className ? `modal ${className}` : 'modal'}
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
    </div>,
    document.body,
  )
}
