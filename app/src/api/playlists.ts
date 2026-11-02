import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/** 与 Rust 端 `ArtistRef`（camelCase）一一对应。 */
export interface ArtistRef {
  id: number
  name: string
}

/** 与 Rust 端 `AlbumRef` 一一对应。 */
export interface AlbumRef {
  id: number
  name: string
  coverUrl: string | null
}

/** 与 Rust 端 `NeteaseSong` 一一对应。 */
export interface Song {
  id: number
  name: string
  artists: ArtistRef[]
  album: AlbumRef
  /** 别名 / 副标题，多个以 `/` 连接。 */
  alias: string | null
  durationMs: number
  trackNumber: number
  discNumber: number
  /** `yyyy-MM-dd`。 */
  releaseDate: string | null
  fee: number
  maxBitrate: number
}

/** 歌单列表项。 */
export interface PlaylistSummary {
  id: number
  name: string
  coverUrl: string | null
  trackCount: number
  creatorNickname: string | null
  description: string | null
}

/** 歌单详情（含完整曲目）。 */
export interface PlaylistDetail {
  id: number
  name: string
  coverUrl: string | null
  description: string | null
  creatorNickname: string | null
  /** 接口声明的曲目数，可能大于 `songs.length`。 */
  trackCount: number
  songs: Song[]
  /** 有歌曲没有取回详情（某批请求失败，或歌曲已下架）。 */
  isPartial: boolean
}

/** `notLoggedIn`：还没登录；`sessionExpired`：登录已失效并被清除；`failed`：其他失败（登录保留）。 */
export type PlaylistErrorKind = 'notLoggedIn' | 'sessionExpired' | 'failed'

/** 加载歌单失败，`message` 已经是面向用户的中文说明。 */
export class PlaylistLoadError extends Error {
  readonly kind: PlaylistErrorKind

  constructor(kind: PlaylistErrorKind, message: string) {
    super(message)
    this.name = 'PlaylistLoadError'
    this.kind = kind
  }
}

/** 后端 Err 是 `{ kind, message }`；其他形态（字符串、Error）一律当作普通失败。 */
export function toPlaylistLoadError(raw: unknown): PlaylistLoadError {
  if (raw instanceof PlaylistLoadError) return raw
  if (typeof raw === 'object' && raw !== null) {
    const { kind, message } = raw as { kind?: unknown; message?: unknown }
    if (typeof message === 'string') {
      const known = kind === 'notLoggedIn' || kind === 'sessionExpired' ? kind : 'failed'
      return new PlaylistLoadError(known, message)
    }
  }
  return new PlaylistLoadError('failed', typeof raw === 'string' ? raw : '加载歌单失败，请稍后重试')
}

/** 大歌单分批补齐曲目详情的进度。 */
export interface PlaylistProgress {
  playlistId: number
  loaded: number
  total: number
}

/** 当前账号的全部歌单（创建的 + 收藏的）。 */
export async function listPlaylists(): Promise<PlaylistSummary[]> {
  try {
    return await invoke<PlaylistSummary[]>('list_playlists')
  } catch (error) {
    throw toPlaylistLoadError(error)
  }
}

/** 一个歌单的完整曲目；超过 500 首时后端会发 `playlist:progress` 事件。 */
export async function getPlaylistTracks(id: number): Promise<PlaylistDetail> {
  try {
    return await invoke<PlaylistDetail>('get_playlist_tracks', { id })
  } catch (error) {
    throw toPlaylistLoadError(error)
  }
}

/** 订阅歌单曲目加载进度。 */
export function onPlaylistProgress(handler: (progress: PlaylistProgress) => void): Promise<UnlistenFn> {
  return listen<PlaylistProgress>('playlist:progress', (event) => handler(event.payload))
}
