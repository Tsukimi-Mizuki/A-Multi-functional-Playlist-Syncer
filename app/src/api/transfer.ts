import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/** 任务类别：`selected` 下载选中 / `missing` 下载缺失 / `sync` 一键同步。 */
export type TransferKind = 'selected' | 'missing' | 'sync'

/** 一首歌最终是怎么处理的。 */
export type ItemSource = 'downloaded' | 'decryptedNcm' | 'skipped' | 'failed'

/** 一首歌正在处理的阶段。 */
export type DownloadPhase = 'resolving' | 'downloading' | 'decrypting' | 'tagging' | 'matching'

/** 与 Rust 端 `ItemFinish`（camelCase）一一对应：一首歌处理完的最终结果。 */
export interface ItemFinish {
  success: boolean
  skipped: boolean
  source: ItemSource
  outputPath: string | null
  /** 失败原因，或成功时的补充说明。 */
  message: string | null
  /** 实际音质与请求不一致时的说明。 */
  qualityNote: string | null
}

/** `download:progress` 事件的载荷（`DownloadProgress`）。 */
export interface DownloadProgress {
  completed: number
  total: number
  index: number
  songId: number
  songName: string
  stage: string
  phase: DownloadPhase
  message: string | null
  percent: number | null
  bytesDone: number | null
  bytesTotal: number | null
  /** 这首歌处理完时才有。 */
  finish: ItemFinish | null
}

/** `download:started` 事件的载荷：本次任务会处理哪些歌曲。 */
export interface TransferStarted {
  kind: TransferKind
  total: number
  songIds: number[]
}

/** 一首歌的最终结果（`TransferItemView`）。 */
export interface TransferItem {
  songId: number
  songName: string
  success: boolean
  skipped: boolean
  source: ItemSource
  outputPath: string | null
  message: string | null
  qualityNote: string | null
}

/** 任务汇总（`TransferSummaryView`）。 */
export interface TransferSummary {
  kind: TransferKind
  total: number
  succeeded: number
  downloaded: number
  decrypted: number
  skipped: number
  failed: number
  cancelled: boolean
  onlineMatchSkipped: boolean
  /** 结束后后端是否已经重新扫描了本地曲库。 */
  libraryRefreshed: boolean
  results: TransferItem[]
}

export type TransferErrorKind = 'busy' | 'noSongs' | 'playlistNotLoaded' | 'cancelled' | 'failed'

/** 任务无法开始 / 完成，`message` 已经是面向用户的中文说明。 */
export class TransferError extends Error {
  readonly kind: TransferErrorKind

  constructor(kind: TransferErrorKind, message: string) {
    super(message)
    this.name = 'TransferError'
    this.kind = kind
  }
}

/** 后端 Err 是 `{ kind, message }`；其他形态一律当作普通失败。 */
export function toTransferError(raw: unknown): TransferError {
  if (raw instanceof TransferError) return raw
  if (typeof raw === 'object' && raw !== null) {
    const { kind, message } = raw as { kind?: unknown; message?: unknown }
    if (typeof message === 'string') {
      const known: TransferErrorKind =
        kind === 'busy' || kind === 'noSongs' || kind === 'playlistNotLoaded' || kind === 'cancelled' ? kind : 'failed'
      return new TransferError(known, message)
    }
  }
  return new TransferError('failed', typeof raw === 'string' ? raw : '下载失败，请稍后重试')
}

async function call(command: string, args?: Record<string, unknown>): Promise<TransferSummary> {
  try {
    return await invoke<TransferSummary>(command, args)
  } catch (error) {
    throw toTransferError(error)
  }
}

/** 下载用户勾选的歌曲（只传歌曲 ID，歌单曲目已在后端）。 */
export function downloadTracks(playlistId: number, trackIds: number[]): Promise<TransferSummary> {
  return call('download_tracks', { playlistId, trackIds })
}

/** 下载本地曲库里没有的歌曲。 */
export function downloadMissing(playlistId: number): Promise<TransferSummary> {
  return call('download_missing', { playlistId })
}

/** 一键同步整个歌单。 */
export function syncPlaylist(playlistId: number): Promise<TransferSummary> {
  return call('sync_playlist', { playlistId })
}

/** 取消正在进行的下载 / 同步；没有任务时返回 `false`。 */
export function cancelTransfer(): Promise<boolean> {
  return invoke<boolean>('download_cancel')
}

/** 修改下载音质（只写 `settings.json` 里的 `quality`）。 */
export function setQuality(quality: string): Promise<void> {
  return invoke<void>('set_quality', { quality })
}

/** 订阅任务开始事件。 */
export function onTransferStarted(handler: (started: TransferStarted) => void): Promise<UnlistenFn> {
  return listen<TransferStarted>('download:started', (event) => handler(event.payload))
}

/** 订阅逐曲进度事件。 */
export function onDownloadProgress(handler: (progress: DownloadProgress) => void): Promise<UnlistenFn> {
  return listen<DownloadProgress>('download:progress', (event) => handler(event.payload))
}
