import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect, useState, type ReactNode } from 'react'

/** 在非 Tauri 环境（如纯浏览器预览）下 getCurrentWindow 会抛错，这里统一兜底。 */
function currentWindow() {
  try {
    return getCurrentWindow()
  } catch {
    return null
  }
}

function MinimizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M0 5h10" stroke="currentColor" strokeWidth="1" fill="none" />
    </svg>
  )
}

function MaximizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" strokeWidth="1" fill="none" />
    </svg>
  )
}

function RestoreIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path
        d="M2.5 2.5V0.5h7v7h-2M0.5 2.5h7v7h-7z"
        stroke="currentColor"
        strokeWidth="1"
        fill="none"
      />
    </svg>
  )
}

function CloseIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" strokeWidth="1" fill="none" />
    </svg>
  )
}

/** 自绘标题栏：68px 高，整条可拖动，双击切换最大化。 */
export default function TitleBar({ toolsSlot, accountSlot }: { toolsSlot?: ReactNode; accountSlot?: ReactNode }) {
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    const win = currentWindow()
    if (!win) return
    let disposed = false
    let unlisten: (() => void) | undefined

    const sync = () => {
      win
        .isMaximized()
        .then((value) => {
          if (!disposed) setMaximized(value)
        })
        .catch(() => undefined)
    }

    sync()
    win
      .onResized(sync)
      .then((fn) => {
        if (disposed) fn()
        else unlisten = fn
      })
      .catch(() => undefined)

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  const toggleMaximize = () => {
    void currentWindow()?.toggleMaximize()
  }

  return (
    <header className="titlebar" data-tauri-drag-region onDoubleClick={toggleMaximize}>
      <div className="titlebar-brand" data-tauri-drag-region>
        <span className="titlebar-title" data-tauri-drag-region>
          Lunatic Princess
        </span>
      </div>
      {(toolsSlot || accountSlot) && (
        <div className="titlebar-actions" onDoubleClick={(event) => event.stopPropagation()}>
          {toolsSlot}
          {accountSlot && <div className="titlebar-account">{accountSlot}</div>}
        </div>
      )}
      <div className="titlebar-controls" onDoubleClick={(event) => event.stopPropagation()}>
        <button
          type="button"
          className="caption-button"
          aria-label="最小化窗口"
          title="最小化"
          onClick={() => void currentWindow()?.minimize()}
        >
          <MinimizeIcon />
        </button>
        <button
          type="button"
          className="caption-button"
          aria-label={maximized ? '还原窗口' : '最大化窗口'}
          title={maximized ? '还原' : '最大化'}
          onClick={toggleMaximize}
        >
          {maximized ? <RestoreIcon /> : <MaximizeIcon />}
        </button>
        <button
          type="button"
          className="caption-button caption-close"
          aria-label="关闭窗口"
          title="关闭"
          onClick={() => void currentWindow()?.close()}
        >
          <CloseIcon />
        </button>
      </div>
    </header>
  )
}
