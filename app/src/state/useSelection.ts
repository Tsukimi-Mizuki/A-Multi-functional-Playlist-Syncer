import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { Song } from '../api/playlists'

/** 在 `[from, to]`（含两端、不分先后）区间内把对应歌曲全部加入或移出选择，返回新集合。 */
export function applyRange(
  selected: ReadonlySet<number>,
  songs: readonly Song[],
  from: number,
  to: number,
  select: boolean,
): Set<number> {
  const next = new Set(selected)
  const low = Math.max(0, Math.min(from, to))
  const high = Math.min(songs.length - 1, Math.max(from, to))
  for (let index = low; index <= high; index += 1) {
    const song = songs[index]
    if (!song) continue
    if (select) next.add(song.id)
    else next.delete(song.id)
  }
  return next
}

/**
 * 歌曲表格的多选状态（按歌曲 ID 记，歌单里重复出现的同一首歌一起勾选）。
 *
 * - 点复选框切换一行；按住 Shift 再点，把「上一次点的那一行」到这一行之间的所有行设为与这一行相同的状态；
 * - 表头复选框全选 / 全不选；
 * - 换歌单或重新加载曲目后清空。
 */
export function useSelection(songs: readonly Song[]) {
  const [selected, setSelected] = useState<ReadonlySet<number>>(() => new Set())
  const anchor = useRef<number | null>(null)

  useEffect(() => {
    setSelected(new Set())
    anchor.current = null
  }, [songs])

  const toggle = useCallback(
    (index: number, extend: boolean) => {
      const song = songs[index]
      if (!song) return
      const from = anchor.current
      anchor.current = index
      setSelected((previous) => {
        const select = !previous.has(song.id)
        if (extend && from !== null && from < songs.length) return applyRange(previous, songs, from, index, select)
        const next = new Set(previous)
        if (select) next.add(song.id)
        else next.delete(song.id)
        return next
      })
    },
    [songs],
  )

  const allSelected = useMemo(() => songs.length > 0 && songs.every((song) => selected.has(song.id)), [songs, selected])

  const toggleAll = useCallback(() => {
    setSelected((previous) => {
      const everything = songs.length > 0 && songs.every((song) => previous.has(song.id))
      return everything ? new Set() : new Set(songs.map((song) => song.id))
    })
    anchor.current = null
  }, [songs])

  const clear = useCallback(() => {
    setSelected(new Set())
    anchor.current = null
  }, [])

  return { selected, count: selected.size, allSelected, toggle, toggleAll, clear }
}
