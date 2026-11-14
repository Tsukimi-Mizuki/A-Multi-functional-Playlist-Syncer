import { useEffect, useRef, useState } from 'react'
import {
  EDITOR_DISPLAY_ITEMS,
  displayItemLabel,
  type EditorDisplay,
  type EditorDisplayItem,
} from '../state/editorDisplay'

/** 退场动画的最长时长；动画事件没触发（例如窗口被最小化）时按这个时间兜底卸载浮层。 */
const LEAVE_FALLBACK_MS = 400

interface EditorDisplayMenuProps {
  display: EditorDisplay
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * 「显示选项」：一个按钮加一块浮在面板上的小浮层，勾选要显示的标签项目，改了立刻生效。
 * 歌词与标签编辑器的「标签信息」和批量修改标签左边的标签面板各放一个，管的是同一份设置。
 * 用浮层而不是对话框：对话框会把下面的窗口虚化，看不到勾选之后左边变成什么样。
 * 浮层相对最近的定位祖先铺满一行（编辑器的 `.editor-card-head`、批量窗口的 `.tagpane-head`），
 * 所以要放在这样的标题行里面。
 */
export default function EditorDisplayMenu({ display, open, onOpenChange }: EditorDisplayMenuProps) {
  const buttonRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)
  const onOpenChangeRef = useRef(onOpenChange)
  useEffect(() => {
    onOpenChangeRef.current = onOpenChange
  })

  // 关闭时不马上卸载：浮层先留在原处播完退场动画（`leaving`），动画结束再卸载。
  // 退场期间又被打开就直接回到打开的样子（去掉退场的样式类，进场动画重新播）。
  const [mounted, setMounted] = useState(open)
  if (open && !mounted) setMounted(true)
  const leaving = mounted && !open
  useEffect(() => {
    if (!leaving) return
    const timer = window.setTimeout(() => setMounted(false), LEAVE_FALLBACK_MS)
    return () => window.clearTimeout(timer)
  }, [leaving])

  useEffect(() => {
    if (!open) return
    const panel = panelRef.current
    // 卡片滚到了下面（从底部的「已隐藏 N 项」打开）时把浮层带回视野。
    panel?.scrollIntoView({ block: 'nearest' })
    panel?.querySelector<HTMLElement>('input')?.focus()

    const close = () => onOpenChangeRef.current(false)
    // 捕获阶段拦下 Esc：否则会冒泡到编辑器对话框，把整个编辑器关掉。
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.stopPropagation()
      close()
      buttonRef.current?.focus()
    }
    const onMouseDown = (event: MouseEvent) => {
      const target = event.target as Node
      if (!panel?.contains(target) && !buttonRef.current?.contains(target)) close()
    }
    document.addEventListener('keydown', onKeyDown, true)
    document.addEventListener('mousedown', onMouseDown)
    return () => {
      document.removeEventListener('keydown', onKeyDown, true)
      document.removeEventListener('mousedown', onMouseDown)
    }
  }, [open])

  const hiddenCount = display.hidden.length
  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        className="btn btn-small btn-ghost"
        aria-label="显示选项"
        aria-haspopup="true"
        aria-expanded={open}
        title="选择显示哪些标签项目（歌词与标签编辑器、批量修改标签共用）"
        onClick={() => onOpenChange(!open)}
      >
        显示选项
      </button>
      {mounted && (
        <div
          ref={panelRef}
          className={leaving ? 'editor-display-panel editor-display-leaving' : 'editor-display-panel'}
          role="group"
          aria-label="显示选项"
          // 退场中的浮层只是个影子：不能点、不能聚焦，读屏也不读。
          inert={leaving}
          onAnimationEnd={(event) => {
            if (leaving && event.target === event.currentTarget) setMounted(false)
          }}
        >
          <p className="editor-display-title">显示这些项目</p>
          <div className="editor-display-grid">
            {EDITOR_DISPLAY_ITEMS.map((item) => (
              <label key={item.id} className="editor-display-item">
                <input
                  type="checkbox"
                  aria-label={`显示${item.label}`}
                  checked={display.isVisible(item.id)}
                  onChange={() => display.toggle(item.id)}
                />
                <span>{item.label}</span>
              </label>
            ))}
          </div>
          <p className="editor-display-note">隐藏的项目不会被清除，保存时照常保留。</p>
          <div className="editor-display-foot">
            <button
              type="button"
              className="btn btn-small"
              aria-label="全部显示"
              disabled={hiddenCount === 0}
              onClick={display.showAll}
            >
              全部显示
            </button>
            <button type="button" className="btn btn-small btn-ghost" aria-label="关闭显示选项" onClick={() => onOpenChange(false)}>
              完成
            </button>
          </div>
        </div>
      )}
    </>
  )
}

/**
 * 面板底部的「已隐藏 N 项」：点它打开显示选项；被隐藏的项目有未保存的修改时在后面点名，
 * 免得看不见的改动不知不觉写进文件。没有隐藏任何项目时什么都不画。
 */
export function HiddenItemsNote({
  hidden,
  changed,
  onOpen,
}: {
  hidden: readonly EditorDisplayItem[]
  /** 被隐藏、却有未保存修改的项目。 */
  changed: readonly EditorDisplayItem[]
  onOpen: () => void
}) {
  if (hidden.length === 0) return null
  return (
    <p className="editor-hidden-note">
      <button
        type="button"
        className="editor-hidden-link"
        aria-label={`已隐藏 ${hidden.length} 项，打开显示选项`}
        title={`已隐藏：${hidden.map(displayItemLabel).join('、')}`}
        onClick={onOpen}
      >
        已隐藏 {hidden.length} 项
      </button>
      {changed.length > 0 && (
        <span className="editor-hint-warn" role="status">
          {' '}
          · 其中「{changed.map(displayItemLabel).join('、')}」有未保存的修改
        </span>
      )}
    </p>
  )
}
