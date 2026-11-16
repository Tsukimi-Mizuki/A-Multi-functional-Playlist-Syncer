import { useEffect, useId, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { motionReduced } from '../state/appearance'

/** 当前打开的模态对话框（后打开的在末尾）：Esc 只关最上面一个，编辑器上叠着批量窗口 / 确认框时不会一起关掉。 */
const openModals: symbol[] = []

/** 退场动画的最长时长；动画事件没触发（例如窗口最小化）时按这个时间兜底移除残影。 */
const LEAVE_FALLBACK_MS = 400

interface ScrollOffset {
  left: number
  top: number
}

/**
 * 滚动过的元素在对话框里的位置（从遮罩层往下每一层是第几个子元素）与滚动量。
 *
 * 复制出来的节点滚动量都是 0，而对话框离开文档之后原来的滚动量也读不到了，所以平时就把滚动量记下来，
 * 生成残影时按同样的路径找到对应的元素再滚回去。不这样做的话，滚动过的清单（虚拟滚动只画可见的那几行）
 * 在淡出的一瞬间会变成一片空白。
 */
function scrollPaths(backdrop: HTMLElement, scrolled: ReadonlyMap<Element, ScrollOffset>): { path: number[]; offset: ScrollOffset }[] {
  const result: { path: number[]; offset: ScrollOffset }[] = []
  for (const [element, offset] of scrolled) {
    if (offset.left === 0 && offset.top === 0) continue
    const path: number[] = []
    let node: Element | null = element
    while (node && node !== backdrop) {
      const parent: Element | null = node.parentElement
      if (!parent) break
      path.unshift(Array.prototype.indexOf.call(parent.children, node))
      node = parent
    }
    // 已经不在对话框里的元素（滚出去的虚拟行、换掉的分页）不用管。
    if (node === backdrop) result.push({ path, offset })
  }
  return result
}

function restoreScroll(ghost: HTMLElement, offsets: { path: number[]; offset: ScrollOffset }[]): void {
  for (const { path, offset } of offsets) {
    let node: Element | undefined = ghost
    for (const index of path) node = node?.children[index]
    if (!node) continue
    node.scrollLeft = offset.left
    node.scrollTop = offset.top
  }
}

/**
 * 对话框被卸载时留下一个静态“残影”播放退场动画。
 *
 * 调用方都是直接卸载对话框（`{open && <Dialog/>}`），React 不会等动画播完；
 * 这里复制卸载前的 DOM 挂到 body 上淡出，播完即删，各个对话框不用为了退场动画改写。
 * 严格模式下的模拟卸载不会真的移除节点，所以只有节点确实离开文档时才生成残影。
 */
function leaveWithGhost(backdrop: HTMLElement, scrolled: ReadonlyMap<Element, ScrollOffset>): void {
  if (motionReduced()) return
  const ghost = backdrop.cloneNode(true) as HTMLElement
  const offsets = scrollPaths(backdrop, scrolled)
  queueMicrotask(() => {
    if (backdrop.isConnected) return
    ghost.classList.add('modal-leaving')
    ghost.setAttribute('aria-hidden', 'true')
    ghost.inert = true
    document.body.appendChild(ghost)
    restoreScroll(ghost, offsets)
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
    // 滚动事件不冒泡，在捕获阶段听对话框里所有元素的滚动，记下各自滚到了哪里（退场的残影要用）。
    const scrolled = new Map<Element, ScrollOffset>()
    const onScroll = (event: Event) => {
      const element = event.target
      if (element instanceof Element) scrolled.set(element, { left: element.scrollLeft, top: element.scrollTop })
    }
    backdrop?.addEventListener('scroll', onScroll, { capture: true, passive: true })
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      backdrop?.removeEventListener('scroll', onScroll, { capture: true })
      const index = openModals.indexOf(token)
      if (index >= 0) openModals.splice(index, 1)
      previous?.focus()
      if (backdrop) leaveWithGhost(backdrop, scrolled)
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
