import { useCallback, useEffect, useState } from 'react'

/** 退场动画的最长时长；动画事件没触发（例如窗口最小化）时按这个时间兜底卸载。 */
const LEAVE_FALLBACK_MS = 400

export interface Presence<T> {
  /** 现在该画的内容：在场时是最新的值，离场期间是离场前最后一次的值，退场播完后是 `null`。 */
  value: T | null
  /** 正在播放退场动画：内容还画着，但不该再响应操作。 */
  leaving: boolean
  /** 退场动画播完时调用（只在 `leaving` 时调用），内容随即卸载。 */
  done: () => void
}

/**
 * 让一块内容在条件变假之后多留一会儿，播完退场动画再卸载。
 *
 * 直接写 `{value && <X />}` 的话，值一变成 `null` 内容立刻消失，没有机会播退场动画，而且这时往往已经
 * 没有数据可画了（任务结束后进度就是 `null`）。这里把离场前最后一次的值留着，交给调用方照常画，
 * 调用方给元素加上退场的样式，并在 `animationend` 时调用 `done`。
 */
export function usePresence<T>(value: T | null): Presence<T> {
  const [kept, setKept] = useState<T | null>(value)
  // 在场时跟着最新的值走（渲染期间调整状态，不多画一帧旧内容）。
  if (value !== null && kept !== value) setKept(value)
  const leaving = value === null && kept !== null

  useEffect(() => {
    if (!leaving) return
    const timer = window.setTimeout(() => setKept(null), LEAVE_FALLBACK_MS)
    return () => window.clearTimeout(timer)
  }, [leaving])

  const done = useCallback(() => setKept(null), [])
  return { value: value ?? kept, leaving, done }
}
