import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

/** 与 Rust 端 `TagView`（camelCase）一一对应。 */
export interface TagView {
  path: string
  fileName: string
  title: string | null
  /** 多个艺术家用 `/` 连接。 */
  artist: string | null
  album: string | null
  albumArtist: string | null
  year: number | null
  trackNumber: number | null
  trackTotal: number | null
  discNumber: number | null
  discTotal: number | null
  genre: string | null
  composer: string | null
  lyricist: string | null
  /** 已去掉本程序写入的网易云 ID 标记。 */
  comment: string | null
  lyrics: string | null
  /** 字符串：避免超过 2^53 的 ID 在 JS 里丢精度。 */
  neteaseId: string | null
  hasCover: boolean
  /** 封面的 data URL；没有封面或封面过大时为 null。 */
  coverDataUrl: string | null
  coverSize: number | null
  coverTooLarge: boolean
  coverMimeType: string | null
  /** 封面像素尺寸；认不出时为 null。 */
  coverWidth: number | null
  coverHeight: number | null
  /** 封面的图片类型（中文名，如「正面封面」）。 */
  coverPictureType: string | null
  durationMs: number
  /** kbps。 */
  bitrate: number
  fileSize: number
  /** 大写扩展名，如 `MP3`。 */
  fileFormat: string | null
}

/** `invalid` 参数不对；`notFound` 文件不存在；`unsupported` 格式不支持；`failed` 其他失败。 */
export type TagErrorKind = 'invalid' | 'notFound' | 'unsupported' | 'failed'

/** 读取标签失败，`message` 已经是面向用户的中文说明。 */
export class TagLoadError extends Error {
  readonly kind: TagErrorKind

  constructor(kind: TagErrorKind, message: string) {
    super(message)
    this.name = 'TagLoadError'
    this.kind = kind
  }
}

const KINDS: readonly TagErrorKind[] = ['invalid', 'notFound', 'unsupported', 'failed']

/** 后端 Err 是 `{ kind, message }`；其他形态（字符串、Error）一律当作普通失败。 */
export function toTagLoadError(raw: unknown): TagLoadError {
  if (raw instanceof TagLoadError) return raw
  if (typeof raw === 'object' && raw !== null) {
    const { kind, message } = raw as { kind?: unknown; message?: unknown }
    if (typeof message === 'string') {
      const known = KINDS.find((item) => item === kind)
      return new TagLoadError(known ?? 'failed', message)
    }
  }
  return new TagLoadError('failed', typeof raw === 'string' ? raw : '读取标签失败，请重试')
}

/**
 * 读取本地音频文件的标签（只读，不改文件）。
 * 返回 `null` 表示格式不支持或文件已损坏；权限不足等 IO 故障抛 {@link TagLoadError}。
 */
export async function readTags(path: string): Promise<TagView | null> {
  try {
    return await invoke<TagView | null>('read_tags', { path })
  } catch (error) {
    throw toTagLoadError(error)
  }
}

/**
 * 「查看标签」能打开的扩展名：lofty 支持的容器。
 * `.wma` 没有列进来：C# 版靠 TagLib 能读，lofty 不支持 ASF（见 docs/TAURI_REWRITE.md 模块 6）。
 */
export const TAG_FILE_EXTENSIONS = ['mp3', 'flac', 'm4a', 'mp4', 'ogg', 'opus', 'wav', 'aac']

/** 弹出系统「打开文件」对话框选一个音频文件；取消返回 null。 */
export async function pickAudioFile(title = '选择要查看标签的音频文件'): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    title,
    filters: [
      { name: '音频文件', extensions: TAG_FILE_EXTENSIONS },
      { name: '所有文件', extensions: ['*'] },
    ],
  })
  return typeof selected === 'string' ? selected : null
}

/** 与 Rust 端 `WriteTagsRequest`（camelCase）对应：编辑器保存用 `replaceAll: true`，界面上展示的字段整体替换。 */
export interface WriteTagsRequest {
  title?: string | null
  artists?: string[]
  album?: string | null
  albumArtist?: string | null
  trackNumber?: number | null
  discNumber?: number | null
  year?: number | null
  /** 新封面图片字节的 Base64（不带 `data:` 前缀）；不传就不动文件里的封面。 */
  coverBase64?: string | null
  coverMimeType?: string | null
  /** 删除文件里的全部封面（同时给了 `coverBase64` 时以新封面为准）。 */
  removeCover?: boolean
  plainLyrics?: string | null
  neteaseId?: number | null
  comment?: string | null
  genre?: string | null
  composer?: string | null
  lyricist?: string | null
  /** 编辑器保存的语义：没填的字段会被清除，但没传封面时封面保持原样。 */
  replaceAll?: boolean
}

/** 写入标签（先写同目录临时副本再原子替换，失败保留原文件）。失败抛 {@link TagLoadError}，`message` 是中文说明。 */
export async function writeTags(path: string, request: WriteTagsRequest): Promise<void> {
  try {
    await invoke<void>('write_tags', { path, request })
  } catch (error) {
    throw toTagLoadError(error)
  }
}

// ───────────────────────────── 批量修改标签 ─────────────────────────────

/** 与 Rust 端 `TagRowView`（camelCase）对应：批量修改标签窗口列表里的一行，不带封面数据。 */
export interface TagRow {
  path: string
  fileName: string
  /** 所在目录。 */
  directory: string
  /** 大写扩展名，如 `FLAC`。 */
  fileFormat: string | null
  /** 标签是否读出来了；读不出来时 `error` 是原因，其余字段为空。 */
  readable: boolean
  error: string | null
  title: string | null
  /** 多个艺术家用 `/` 连接。 */
  artist: string | null
  album: string | null
  albumArtist: string | null
  year: number | null
  trackNumber: number | null
  discNumber: number | null
  genre: string | null
  composer: string | null
  lyricist: string | null
  comment: string | null
  /** 歌词的第一行（最多 120 个字符）。 */
  lyricsPreview: string | null
  hasLyrics: boolean
  hasCover: boolean
  durationMs: number
}

/** 读出这些文件的标签摘要，顺序与 `paths` 一致；读不出来的文件也有一行。 */
export async function readTagRows(paths: string[]): Promise<TagRow[]> {
  try {
    return await invoke<TagRow[]>('read_tag_rows', { paths })
  } catch (error) {
    throw toTagLoadError(error)
  }
}

/**
 * 与 Rust 端 `TagPatch`（camelCase）对应：**没出现的字段不动**，`null` / 空白 / 空数组表示清除，其余是新值。
 * 歌词不在这里改。
 */
export interface TagPatch {
  title?: string | null
  artists?: string[]
  album?: string | null
  albumArtist?: string | null
  year?: number | null
  trackNumber?: number | null
  discNumber?: number | null
  genre?: string | null
  composer?: string | null
  lyricist?: string | null
  comment?: string | null
  /** 新封面图片字节的 Base64（不带 `data:` 前缀）。 */
  coverBase64?: string
  coverMimeType?: string
  /** 删除全部封面。 */
  removeCover?: boolean
}

export interface TagPatchResult {
  path: string
  success: boolean
  /** 失败原因（中文）；成功为 null。 */
  message: string | null
  /** 写入之后重新读出的这一行。 */
  row: TagRow
}

/**
 * 把同一份改动写进这些文件，逐个返回结果（某个文件失败不影响其余）。
 * 改动本身不合法（一个字段都没提到等）时抛 {@link TagLoadError}。
 */
export async function patchTags(paths: string[], patch: TagPatch): Promise<TagPatchResult[]> {
  try {
    return await invoke<TagPatchResult[]>('patch_tags', { paths, patch })
  } catch (error) {
    throw toTagLoadError(error)
  }
}
