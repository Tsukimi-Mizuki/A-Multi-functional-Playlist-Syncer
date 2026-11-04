import { useEffect, useState } from 'react'
import { getEditorState } from '../api/editor'

/** 拖入文件时盖在整个窗口上的半透明提示层（不拦截鼠标）。 */
export default function DropOverlay({ editorOpen }: { editorOpen: boolean }) {
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

  return (
    <div className="drop-overlay" role="status" aria-live="polite" aria-label="拖放提示">
      <div className="drop-overlay-card">
        <p className="drop-overlay-title">{editorOpen ? '松开即可载入音频文件' : '松开即可用歌词编辑器打开'}</p>
        <p className="drop-overlay-sub">支持 {hint}，也可以拖入包含音频的文件夹</p>
      </div>
    </div>
  )
}
