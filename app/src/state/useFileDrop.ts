import { listen, TauriEvent, type UnlistenFn } from '@tauri-apps/api/event'
import { useEffect, useRef, useState } from 'react'

/** 拖离窗口后延迟多久才隐藏提示层（同 C# `DropOverlayController`）：目标切换时会成对出现 leave / enter，立刻隐藏会闪烁。 */
export const DROP_HIDE_DELAY_MS = 180

interface DragPayload {
  paths?: string[]
}

/**
 * 监听 Tauri 的文件拖放事件（`tauri://drag-enter / over / drop / leave`；系统文件拖入网页时 WebView 自己收不到，
 * 必须走这组事件）。拖入时 `dragging` 为真，用来显示半透明提示层；拖离后延迟 180 ms 才隐藏，期间再次进入就取消。
 * `enabled` 为假时不处理（例如别的对话框正占着界面）。
 */
export function useFileDrop(enabled: boolean, onDrop: (paths: string[]) => void): { dragging: boolean } {
  const [dragging, setDragging] = useState(false)
  const onDropRef = useRef(onDrop)
  useEffect(() => {
    onDropRef.current = onDrop
  })

  useEffect(() => {
    if (!enabled) {
      setDragging(false)
      return
    }
    let timer: number | undefined
    const clearTimer = () => {
      if (timer !== undefined) window.clearTimeout(timer)
      timer = undefined
    }
    const show = () => {
      clearTimer()
      setDragging(true)
    }

    const pending: Promise<UnlistenFn>[] = [
      listen<DragPayload>(TauriEvent.DRAG_ENTER, (event) => {
        if ((event.payload.paths?.length ?? 0) > 0) show()
      }),
      listen(TauriEvent.DRAG_OVER, show),
      listen(TauriEvent.DRAG_LEAVE, () => {
        clearTimer()
        timer = window.setTimeout(() => setDragging(false), DROP_HIDE_DELAY_MS)
      }),
      listen<DragPayload>(TauriEvent.DRAG_DROP, (event) => {
        clearTimer()
        setDragging(false)
        onDropRef.current(event.payload.paths ?? [])
      }),
    ]
    // 非 Tauri 环境（纯浏览器预览）没有事件通道，静默忽略。
    for (const promise of pending) promise.catch(() => undefined)

    return () => {
      clearTimer()
      setDragging(false)
      for (const promise of pending) promise.then((unlisten) => unlisten()).catch(() => undefined)
    }
  }, [enabled])

  return { dragging }
}
