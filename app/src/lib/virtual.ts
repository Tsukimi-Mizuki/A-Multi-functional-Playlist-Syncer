import { useCallback, useLayoutEffect, useRef, useState } from 'react'

export interface RowRange {
  /** 第一个要渲染的行（含）。 */
  start: number
  /** 最后一个要渲染的行（不含）。 */
  end: number
}

/** 固定行高的虚拟列表可见区间：可视范围上下各多渲染 `overscan` 行，避免快速滚动时露白。 */
export function computeRowRange(
  scrollTop: number,
  viewportHeight: number,
  rowHeight: number,
  count: number,
  overscan: number,
): RowRange {
  if (count <= 0 || rowHeight <= 0) return { start: 0, end: 0 }
  const first = Math.floor(Math.max(0, scrollTop) / rowHeight)
  const last = Math.ceil((Math.max(0, scrollTop) + Math.max(0, viewportHeight)) / rowHeight)
  return {
    start: Math.max(0, first - overscan),
    end: Math.min(count, last + overscan),
  }
}

/**
 * 固定行高的简单虚拟滚动：只渲染可视区附近的行。
 * 用法：把 `scrollerRef` 挂到滚动容器，`onScroll` 绑到同一容器，内部放一个高度为 `totalHeight` 的占位，
 * 行用绝对定位（`translateY(index * rowHeight)`）。
 */
export function useVirtualRows(count: number, rowHeight: number, overscan = 8) {
  const scrollerRef = useRef<HTMLDivElement>(null)
  const frame = useRef(0)
  const [viewport, setViewport] = useState({ scrollTop: 0, height: 0 })

  const measure = useCallback(() => {
    const scroller = scrollerRef.current
    if (!scroller) return
    setViewport((previous) =>
      previous.scrollTop === scroller.scrollTop && previous.height === scroller.clientHeight
        ? previous
        : { scrollTop: scroller.scrollTop, height: scroller.clientHeight },
    )
  }, [])

  useLayoutEffect(() => {
    const scroller = scrollerRef.current
    if (!scroller) return
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(scroller)
    return () => {
      observer.disconnect()
      cancelAnimationFrame(frame.current)
    }
  }, [measure])

  // 滚动事件很密集：合并到每帧最多更新一次。
  const onScroll = useCallback(() => {
    cancelAnimationFrame(frame.current)
    frame.current = requestAnimationFrame(measure)
  }, [measure])

  const range = computeRowRange(viewport.scrollTop, viewport.height, rowHeight, count, overscan)
  return { scrollerRef, onScroll, range, totalHeight: count * rowHeight }
}
