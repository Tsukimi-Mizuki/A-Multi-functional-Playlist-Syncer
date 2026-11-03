import { useCallback, useEffect, useRef, useState } from 'react'
import {
  cancelLibraryScan,
  matchPlaylist,
  onLibraryProgress,
  scanLibrary,
  toLibraryError,
  type LibraryErrorKind,
  type LocalMatch,
  type MatchOutcome,
  type ScanProgress,
} from '../api/library'
import type { PlaylistDetail } from '../api/playlists'

/** 正在扫描 / 对比；`progress` 在扫描开始前为 `null`。 */
export interface LocalBusy {
  progress: ScanProgress | null
}

export interface LocalError {
  kind: LibraryErrorKind
  message: string
}

type Mode = 'auto' | 'compare' | 'rescan'

/**
 * 当前歌单与本地曲库的对比状态。
 *
 * - 歌单曲目加载完成后自动对比：设置里开了「启动后自动对比本地曲库」就先扫描（已有扫描结果则直接复用），
 *   没开则只在已经扫描过的情况下对比（与 C# 一致）。
 * - 「对比本地」= 没有扫描结果就先扫描，再对比；「重新扫描」= 强制重新扫描，再对比。
 * - 扫描期间保留上一次的对比结果，不让表格闪回「未对比」；取消扫描后回到取消前的状态。
 * - 换歌单、重新加载曲目都会作废在途的请求，只采纳最后一次的结果。
 */
export function useLocalMatch(playlist: PlaylistDetail | null, autoMatch: boolean | null) {
  const [outcome, setOutcome] = useState<MatchOutcome | null>(null)
  const [busy, setBusy] = useState<LocalBusy | null>(null)
  const [error, setError] = useState<LocalError | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const tokenRef = useRef(0)

  const run = useCallback(async (playlistId: number, mode: Mode) => {
    const token = ++tokenRef.current
    setNotice(null)
    setError(null)
    setBusy({ progress: null })
    try {
      if (mode === 'rescan') await scanLibrary()
      const result = await matchPlaylist(playlistId, mode === 'auto' ? 'never' : 'ifNeeded')
      if (token !== tokenRef.current) return
      // 还没有扫描过：保持「未对比」。
      setOutcome(result.scanned ? result : null)
    } catch (raised: unknown) {
      if (token !== tokenRef.current) return
      const failure = toLibraryError(raised)
      if (failure.kind === 'cancelled') setNotice('已取消扫描本地曲库')
      else setError({ kind: failure.kind, message: failure.message })
    } finally {
      if (token === tokenRef.current) setBusy(null)
    }
  }, [])

  // 歌曲加载完成（或重新加载）后自动对比。
  const playlistId = playlist?.id ?? null
  useEffect(() => {
    tokenRef.current += 1
    setOutcome(null)
    setError(null)
    setNotice(null)
    setBusy(null)
    if (playlist === null || autoMatch === null) return
    void run(playlist.id, autoMatch ? 'compare' : 'auto')
    // `playlist` 的引用变化（重新加载）也要重新对比。
  }, [playlist, playlistId, autoMatch, run])

  // 扫描进度（后端的全局事件，只在忙碌时采纳）。
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    onLibraryProgress((progress) => {
      if (!disposed) setBusy((current) => (current ? { progress } : current))
    })
      .then((fn) => {
        if (disposed) fn()
        else unlisten = fn
      })
      .catch(() => undefined) // 非 Tauri 环境（纯浏览器预览）没有后端。
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  const compare = useCallback(() => {
    if (playlistId !== null) void run(playlistId, 'compare')
  }, [playlistId, run])

  const rescan = useCallback(() => {
    if (playlistId !== null) void run(playlistId, 'rescan')
  }, [playlistId, run])

  const cancel = useCallback(() => {
    void cancelLibraryScan().catch(() => undefined)
  }, [])

  // 只有结果确实属于当前歌单、且与歌曲一一对应时才交给表格。
  const matches: LocalMatch[] | null =
    outcome !== null && playlist !== null && outcome.playlistId === playlist.id && outcome.matches.length === playlist.songs.length
      ? outcome.matches
      : null

  return { outcome: matches ? outcome : null, matches, busy, error, notice, compare, rescan, cancel }
}
