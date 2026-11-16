import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { getEditorState } from '../api/editor'

const TITLES = {
  main: '松开即可打开：一首用歌词编辑器，多首交给批量修改标签',
  editor: '松开即可载入：一首载入编辑器，多首交给批量修改标签',
  batch: '松开即可换成这批音频',
} as const

/** 松开后由谁接收拖入的文件。 */
export type DropTarget = keyof typeof TITLES

interface DropOverlayProps {
  target: DropTarget
  /** 拖放已经结束，正在播放退场动画（由 `usePresence` 给出）。 */
  leaving: boolean
  /** 退场动画播完。 */
  onLeft: () => void
}

/** 拖入文件时盖在整个窗口上的半透明提示层（不拦截鼠标）；`target` 是松开后由谁接收。 */
export default function DropOverlay({ target, leaving, onLeft }: DropOverlayProps) {
  const [hint, setHint] = useState('MP3 / FLAC / M4A / MP4 / OGG / OPUS / WAV / AAC / WMA')
  useEffect(() => {
    let cancelled = false
    getEditorState()
      .then((state) => {
        if (!cancelled) setHint(state.extensionHint)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [])

  // 挂到 body 上：编辑器打开时主界面（#root）是虚化的，提示层不能跟着一起糊掉。
  return createPortal(
    <div
      className={leaving ? 'drop-overlay drop-overlay-leaving' : 'drop-overlay'}
      role="status"
      aria-live="polite"
      aria-label="拖放提示"
      aria-hidden={leaving || undefined}
    >
      <div
        className="drop-overlay-card"
        onAnimationEnd={(event) => {
          if (leaving && event.target === event.currentTarget) onLeft()
        }}
      >
        <p className="drop-overlay-title">{TITLES[target]}</p>
        <p className="drop-overlay-sub">支持 {hint}，也可以拖入文件夹（含子文件夹）</p>
      </div>
    </div>,
    document.body,
  )
}
