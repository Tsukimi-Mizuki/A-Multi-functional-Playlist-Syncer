import { useCallback, useEffect, useRef, useState } from 'react'
import type { AccountSummary } from '../api/auth'
import {
  getPlaylistTracks,
  listPlaylists,
  onPlaylistProgress,
  toPlaylistLoadError,
  type PlaylistDetail,
  type PlaylistProgress,
  type PlaylistSummary,
} from '../api/playlists'

export type ListState =
  | { status: 'idle' }
  | { status: 'loading' }
  | {
      status: 'ready'
      items: PlaylistSummary[]
      /** 正在后台刷新（列表保持可用）。 */
      refreshing: boolean
      /** 刷新失败的中文说明；此时仍保留上一次成功的列表。 */
      error: string | null
    }
  | { status: 'error'; message: string }

export type DetailState =
  | { status: 'idle' }
  | { status: 'loading'; progress: PlaylistProgress | null }
  | { status: 'ready'; detail: PlaylistDetail }
  | { status: 'error'; message: string }

/**
 * 歌单列表与当前歌单曲目。
 *
 * - 登录成功（或启动校验结束且已登录）后自动加载歌单列表，并选中第一个；退出登录后清空。
 * - 选中歌单即加载其曲目；快速切换时只采纳最后一次请求的结果。
 * - 错误文案由后端给出（中文），同时后端已经写入运行日志。
 */
export function usePlaylists(account: AccountSummary, validated: boolean) {
  const [list, setList] = useState<ListState>({ status: 'idle' })
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [detail, setDetail] = useState<DetailState>({ status: 'idle' })
  const [listNonce, setListNonce] = useState(0)
  const [detailNonce, setDetailNonce] = useState(0)
  const selectedIdRef = useRef<number | null>(null)

  // 只有“账号 ID”变化才算换了账号；昵称 / 会员状态刷新不应重新加载歌单。
  const userId = validated && account.loggedIn && account.userId > 0 ? account.userId : null

  useEffect(() => {
    selectedIdRef.current = selectedId
  }, [selectedId])

  // 歌单列表。
  useEffect(() => {
    if (userId === null) {
      setList({ status: 'idle' })
      setSelectedId(null)
      setDetail({ status: 'idle' })
      return
    }
    let stale = false
    setList((previous) =>
      previous.status === 'ready' ? { ...previous, refreshing: true, error: null } : { status: 'loading' },
    )
    listPlaylists()
      .then((items) => {
        if (stale) return
        setList({ status: 'ready', items, refreshing: false, error: null })
        setSelectedId((current) =>
          current !== null && items.some((item) => item.id === current) ? current : (items[0]?.id ?? null),
        )
      })
      .catch((error: unknown) => {
        if (stale) return
        const message = toPlaylistLoadError(error).message
        setList((previous) =>
          previous.status === 'ready'
            ? { ...previous, refreshing: false, error: message }
            : { status: 'error', message },
        )
      })
    return () => {
      stale = true
    }
  }, [userId, listNonce])

  // 当前歌单曲目。
  useEffect(() => {
    if (selectedId === null) {
      setDetail({ status: 'idle' })
      return
    }
    let stale = false
    setDetail({ status: 'loading', progress: null })
    getPlaylistTracks(selectedId)
      .then((loaded) => {
        if (!stale) setDetail({ status: 'ready', detail: loaded })
      })
      .catch((error: unknown) => {
        if (!stale) setDetail({ status: 'error', message: toPlaylistLoadError(error).message })
      })
    return () => {
      stale = true
    }
  }, [selectedId, detailNonce])

  // 分批补齐曲目详情的进度，只认当前选中的歌单。
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    onPlaylistProgress((progress) => {
      if (disposed || progress.playlistId !== selectedIdRef.current) return
      setDetail((current) => (current.status === 'loading' ? { status: 'loading', progress } : current))
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

  const select = useCallback((id: number) => setSelectedId(id), [])
  const reloadList = useCallback(() => setListNonce((value) => value + 1), [])
  const reloadDetail = useCallback(() => setDetailNonce((value) => value + 1), [])

  return { list, selectedId, detail, select, reloadList, reloadDetail }
}
