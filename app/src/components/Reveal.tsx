import type { ReactNode } from 'react'
import { usePresence } from '../lib/presence'

interface RevealProps<T> {
  /** 要显示的内容所依据的值；`null` 表示不显示。 */
  value: T | null
  children: (value: T) => ReactNode
}

/**
 * 排在版面里、时有时无的一行（提示条、进度行、结果行）：出现时把下面的内容平滑推开，消失时平滑收起。
 *
 * 直接插入 / 移除这样的一行，下面的歌曲表格会突然跳一下。这里在外面套一层，高度在 0 与内容高度之间过渡
 * （样式见 glass.css 的 `.reveal`）；消失时内容按离场前最后一次的值再画一会儿，播完才卸载。
 */
export default function Reveal<T>({ value, children }: RevealProps<T>) {
  const presence = usePresence(value)
  if (presence.value === null) return null
  return (
    <div
      className={presence.leaving ? 'reveal reveal-leaving' : 'reveal'}
      inert={presence.leaving}
      onAnimationEnd={(event) => {
        if (presence.leaving && event.target === event.currentTarget) presence.done()
      }}
    >
      {children(presence.value)}
    </div>
  )
}
