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

/** 一次对比的结局；`stale` 表示期间换了歌单或又发起了新的对比，结果没有被采纳。 */
type RunResult =
  | { status: 'matched'; outcome: MatchOutcome | null }
  | { status: 'failed'; kind: LibraryErrorKind; message: string }
  | { status: 'stale' }

/**
 * 确认下载 / 同步前要用的对比结果：`ready` 与歌曲一一对应；`libraryMissing` 曲库目录不存在
 * （后端按本地为空处理，全部算缺失）；`aborted` 被取消、失败或换了歌单（原因已显示在对比工具条上）。
 */
export type EnsureResult =
  | { status: 'ready'; matches: LocalMatch[] }
  | { status: 'libraryMissing'; message: string }
  | { status: 'aborted' }

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

  const run = useCallback(async (playlistId: number, mode: Mode): Promise<RunResult> => {
    const token = ++tokenRef.current
    setNotice(null)
    setError(null)
    setBusy({ progress: null })
    try {
      if (mode === 'rescan') await scanLibrary()
      const result = await matchPlaylist(playlistId, mode === 'auto' ? 'never' : 'ifNeeded')
      if (token !== tokenRef.current) return { status: 'stale' }
      // 还没有扫描过：保持「未对比」。
      const outcome = result.scanned ? result : null
      setOutcome(outcome)
      return { status: 'matched', outcome }
    } catch (raised: unknown) {
      if (token !== tokenRef.current) return { status: 'stale' }
      const failure = toLibraryError(raised)
      if (failure.kind === 'cancelled') setNotice('已取消扫描本地曲库')
      else setError({ kind: failure.kind, message: failure.message })
      return { status: 'failed', kind: failure.kind, message: failure.message }
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
  const matches = matchesFor(outcome, playlist)

  // `ensure` 在等待对比时要读到最新的歌单与结果。
  const latest = useRef({ playlist, matches })
  useEffect(() => {
    latest.current = { playlist, matches }
  })

  /** 已经对比过就直接用；还没有（或上次失败）就先对比，必要时扫描曲库，表格同时刷新。 */
  const ensure = useCallback(async (): Promise<EnsureResult> => {
    const current = latest.current
    if (current.matches) return { status: 'ready', matches: current.matches }
    const target = current.playlist
    if (target === null) return { status: 'aborted' }
    const result = await run(target.id, 'compare')
    if (result.status === 'failed' && result.kind === 'directoryMissing') {
      return { status: 'libraryMissing', message: result.message }
    }
    if (result.status !== 'matched') return { status: 'aborted' }
    const fresh = matchesFor(result.outcome, target)
    return fresh && latest.current.playlist === target ? { status: 'ready', matches: fresh } : { status: 'aborted' }
  }, [run])

  return { outcome: matches ? outcome : null, matches, busy, error, notice, compare, rescan, cancel, ensure }
}

function matchesFor(outcome: MatchOutcome | null, playlist: PlaylistDetail | null): LocalMatch[] | null {
  return outcome !== null && playlist !== null && outcome.playlistId === playlist.id && outcome.matches.length === playlist.songs.length
    ? outcome.matches
    : null
}
