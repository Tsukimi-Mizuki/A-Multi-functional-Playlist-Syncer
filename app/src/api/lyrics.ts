import { invoke } from '@tauri-apps/api/core'
import type { Song } from './playlists'

/** `line`：逐行 LRC；`word`：逐字优先（没有逐字数据时后端回退逐行）。 */
export type LyricsMode = 'line' | 'word'

/** 与 Rust 端 `LyricsPreview`（camelCase）一一对应。 */
export interface LyricsPreview {
  songId: number
  /** 请求的形式。 */
  mode: LyricsMode
  /** 歌词文本（LRC / 增强 LRC，含翻译行）；没有歌词时为空串。 */
  text: string
  /** `text` 是否是逐字（增强 LRC）格式；请求逐字但歌曲没有逐字数据时为 false。 */
  isWordLevel: boolean
  /** 这首歌是否有逐字数据（不论请求的形式）。 */
  hasWordData: boolean
  /** `text` 是否带翻译行。 */
  hasTranslation: boolean
  /** 歌词行数（不含翻译行）。 */
  lineCount: number
  /** 没有歌词（纯音乐、未收录）。 */
  isEmpty: boolean
  /** 来源名称，例如「网易云音乐」「QQ 音乐」「酷狗音乐」；没有歌词时是尝试过的主来源（「、」连接）。 */
  source: string
}

/** `sessionExpired`：登录已失效并被清除；`failed`：其他失败（登录保留）。 */
export type LyricsErrorKind = 'sessionExpired' | 'failed'

/** 获取歌词失败，`message` 已经是面向用户的中文说明。 */
export class LyricsLoadError extends Error {
  readonly kind: LyricsErrorKind

  constructor(kind: LyricsErrorKind, message: string) {
    super(message)
    this.name = 'LyricsLoadError'
    this.kind = kind
  }
}

/** 后端 Err 是 `{ kind, message }`；其他形态（字符串、Error）一律当作普通失败。 */
export function toLyricsLoadError(raw: unknown): LyricsLoadError {
  if (raw instanceof LyricsLoadError) return raw
  if (typeof raw === 'object' && raw !== null) {
    const { kind, message } = raw as { kind?: unknown; message?: unknown }
    if (typeof message === 'string') {
      return new LyricsLoadError(kind === 'sessionExpired' ? 'sessionExpired' : 'failed', message)
    }
  }
  return new LyricsLoadError('failed', typeof raw === 'string' ? raw : '获取歌词失败，请稍后重试')
}

/**
 * 取一首歌的歌词预览（只读，不写入任何文件）。来源次序跟随设置里的「歌词来源」：
 * 网易云按歌曲 ID 取词，QQ 音乐与备用词源用歌名 / 歌手 / 专辑 / 时长检索。
 */
export async function getSongLyrics(song: Song, mode: LyricsMode): Promise<LyricsPreview> {
  try {
    return await invoke<LyricsPreview>('get_song_lyrics', {
      id: song.id,
      mode,
      title: song.name,
      artist: song.artists
        .map((artist) => artist.name.trim())
        .filter((name) => name !== '')
        .join('/'),
      album: song.album.name,
      durationMs: song.durationMs,
    })
  } catch (error) {
    throw toLyricsLoadError(error)
  }
}
