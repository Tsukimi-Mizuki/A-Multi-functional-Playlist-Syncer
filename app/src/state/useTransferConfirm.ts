import { useCallback, useEffect, useRef, useState } from 'react'
import type { LocalMatch } from '../api/library'
import type { PlaylistDetail, Song } from '../api/playlists'
import { getSettings } from '../api/settings'
import type { EnsureResult } from './useLocalMatch'

/** 按本地对比结果算出的任务计划；歌单里重复出现的歌只算一次（与后端一致，沿用第一次出现那一行的结果）。 */
export interface TransferPlan {
  /** 去重后的歌曲数。 */
  total: number
  /** 本地已存在的普通音频。 */
  available: number
  /** 待解密的 NCM。 */
  ncm: number
  /** 本地没有、需要下载的歌曲（按歌单顺序）。 */
  missing: Song[]
}

/** `matches` 为 `null`（曲库目录不存在）时按本地为空处理：全部算缺失，与后端一致。 */
export function planTransfer(songs: readonly Song[], matches: readonly LocalMatch[] | null): TransferPlan {
  const seen = new Set<number>()
  const missing: Song[] = []
  let available = 0
  let ncm = 0
  songs.forEach((song, index) => {
    if (seen.has(song.id)) return
    seen.add(song.id)
    const availability = matches?.[index]?.availability ?? 'missing'
    if (availability === 'available') available += 1
    else if (availability === 'ncm') ncm += 1
    else missing.push(song)
  })
  return { total: seen.size, available, ncm, missing }
}

export type ConfirmKind = 'missing' | 'sync'

/** 等待用户确认的下载缺失 / 一键同步。 */
export interface PendingTransfer {
  kind: ConfirmKind
  playlistName: string
  /** 歌单的行数（含重复的歌），用来说明重复的歌只处理一次。 */
  rowCount: number
  plan: TransferPlan
  /** 设置里的下载目录；读不到为 `null`。 */
  downloadDirectory: string | null
  /** 设置里开了「覆盖已存在文件」：同步会重新下载本地已存在的普通音频。 */
  overwriteExisting: boolean
  /** 曲库目录不存在时的说明（此时全部按缺失处理）。 */
  libraryNote: string | null
}

const NOTHING_MISSING = '没有需要下载的歌曲：本地已存在，或是待解密的 NCM（可以用「一键同步」解密）。'

interface TransferActions {
  downloadMissing: (ids: number[]) => void
  sync: () => void
  showNotice: (message: string) => void
}

/**
 * 「下载缺失」与「一键同步」的二次确认：先拿到本地对比结果（还没对比就先对比，表格同时刷新），
 * 再弹出确认框；「下载缺失」的清单可以取消勾选，只下载仍勾选的歌。没有缺失时只给一句提示，不弹窗。
 */
export function useTransferConfirm(
  detail: PlaylistDetail | null,
  ensure: () => Promise<EnsureResult>,
  { downloadMissing, sync, showNotice }: TransferActions,
) {
  const [pending, setPending] = useState<PendingTransfer | null>(null)
  const [preparing, setPreparing] = useState(false)
  const preparingRef = useRef(false)
  const detailRef = useRef(detail)
  useEffect(() => {
    detailRef.current = detail
  })

  // 换了歌单或重新加载曲目：还没确认的计划作废。
  useEffect(() => {
    setPending(null)
  }, [detail])

  const request = useCallback(
    async (kind: ConfirmKind) => {
      const target = detailRef.current
      if (target === null || preparingRef.current) return
      preparingRef.current = true
      setPreparing(true)
      try {
        const [local, settings] = await Promise.all([
          ensure(),
          getSettings()
            .then((state) => state.settings)
            .catch(() => null), // 读不到设置只是少显示目录，不影响确认。
        ])
        if (local.status === 'aborted' || detailRef.current !== target) return
        const plan = planTransfer(target.songs, local.status === 'ready' ? local.matches : null)
        if (kind === 'missing' && plan.missing.length === 0) {
          showNotice(NOTHING_MISSING)
          return
        }
        setPending({
          kind,
          playlistName: target.name.trim() === '' ? '未命名歌单' : target.name,
          rowCount: target.songs.length,
          plan,
          downloadDirectory: settings?.downloadDirectory ?? null,
          overwriteExisting: settings?.overwriteExisting ?? false,
          libraryNote:
            local.status === 'libraryMissing'
              ? `${local.message}现在开始会按本地曲库为空处理，歌单里的歌曲都算缺失。`
              : null,
        })
      } finally {
        preparingRef.current = false
        setPreparing(false)
      }
    },
    [ensure, showNotice],
  )

  const confirmMissing = useCallback(
    (ids: number[]) => {
      setPending(null)
      downloadMissing(ids)
    },
    [downloadMissing],
  )

  const confirmSync = useCallback(() => {
    setPending(null)
    sync()
  }, [sync])

  const dismiss = useCallback(() => setPending(null), [])

  return { pending, preparing, request, confirmMissing, confirmSync, dismiss }
}
