import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/** 本地状态：`available` 本地已存在 / `ncm` NCM 待解密 / `missing` 需下载。 */
export type LocalAvailability = 'available' | 'ncm' | 'missing'

/** 匹配方式：`neteaseId` 标签 ID / `metadata` 标题歌手时长评分 / `fileName` 标题取自文件名 / `none` 未匹配。 */
export type MatchMethod = 'none' | 'neteaseId' | 'metadata' | 'fileName'

/** 与 Rust 端 `LocalMatchView`（camelCase）一一对应：一首歌单歌曲的本地匹配结果。 */
export interface LocalMatch {
  songId: number
  availability: LocalAvailability
  method: MatchMethod
  /** 0–1。 */
  confidence: number
  /** 匹配到的本地文件完整路径。 */
  path: string | null
  fileName: string | null
  format: string | null
  localTitle: string | null
  localArtist: string | null
}

/** 对比统计（按歌单行数算，重复的歌曲各算一行）。 */
export interface MatchStats {
  total: number
  available: number
  ncm: number
  missing: number
}

/** `match_playlist` 的返回值。 */
export interface MatchOutcome {
  playlistId: number
  /** 曲库是否已经扫描过；`false` 时 `matches` 为空。 */
  scanned: boolean
  libraryDirectory: string
  totalFiles: number
  ncmFiles: number
  /** 与歌单歌曲一一对应（按歌单顺序）。 */
  matches: LocalMatch[]
  stats: MatchStats
}

/** `scan_library` 的返回值。 */
export interface LibrarySummary {
  directory: string
  totalFiles: number
  ncmFiles: number
  skippedDirectories: number
}

/** `library:progress` 事件的载荷。 */
export interface ScanProgress {
  scanned: number
  total: number
  fileName: string
}

/** 还没有扫描结果时：`never` 不对比；`ifNeeded` 先扫描。 */
export type ScanMode = 'never' | 'ifNeeded'

export type LibraryErrorKind = 'directoryMissing' | 'cancelled' | 'playlistNotLoaded' | 'failed'

/** 对比失败，`message` 已经是面向用户的中文说明。 */
export class LibraryError extends Error {
  readonly kind: LibraryErrorKind

  constructor(kind: LibraryErrorKind, message: string) {
    super(message)
    this.name = 'LibraryError'
    this.kind = kind
  }
}

/** 后端 Err 是 `{ kind, message }`；其他形态一律当作普通失败。 */
export function toLibraryError(raw: unknown): LibraryError {
  if (raw instanceof LibraryError) return raw
  if (typeof raw === 'object' && raw !== null) {
    const { kind, message } = raw as { kind?: unknown; message?: unknown }
    if (typeof message === 'string') {
      const known: LibraryErrorKind =
        kind === 'directoryMissing' || kind === 'cancelled' || kind === 'playlistNotLoaded' ? kind : 'failed'
      return new LibraryError(known, message)
    }
  }
  return new LibraryError('failed', typeof raw === 'string' ? raw : '对比本地曲库失败，请稍后重试')
}

/** 重新扫描设置里的曲库目录并缓存结果；进度走 `library:progress` 事件。 */
export async function scanLibrary(): Promise<LibrarySummary> {
  try {
    return await invoke<LibrarySummary>('scan_library')
  } catch (error) {
    throw toLibraryError(error)
  }
}

/** 把已加载的歌单与本地曲库对比；曲库还没扫描时按 `scan` 决定先扫描还是返回「尚未对比」。 */
export async function matchPlaylist(playlistId: number, scan: ScanMode): Promise<MatchOutcome> {
  try {
    return await invoke<MatchOutcome>('match_playlist', { request: { playlistId, scan } })
  } catch (error) {
    throw toLibraryError(error)
  }
}

/** 取消正在进行的扫描；没有扫描在运行时返回 `false`。 */
export function cancelLibraryScan(): Promise<boolean> {
  return invoke<boolean>('library_cancel')
}

/** 订阅扫描进度。 */
export function onLibraryProgress(handler: (progress: ScanProgress) => void): Promise<UnlistenFn> {
  return listen<ScanProgress>('library:progress', (event) => handler(event.payload))
}

/** 匹配方式的中文名（与 C# 版一致）。 */
export function methodLabel(method: MatchMethod): string {
  switch (method) {
    case 'neteaseId':
      return '标签 ID 匹配'
    case 'metadata':
      return '元数据匹配'
    case 'fileName':
      return '文件名匹配'
    default:
      return '已匹配'
  }
}

/** 本地状态的中文名（与 C# 版一致）。 */
export function availabilityLabel(availability: LocalAvailability): string {
  switch (availability) {
    case 'available':
      return '本地已存在'
    case 'ncm':
      return 'NCM 待解密'
    default:
      return '需下载'
  }
}
