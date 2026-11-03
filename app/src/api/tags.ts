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
export async function pickAudioFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    title: '选择要查看标签的音频文件',
    filters: [
      { name: '音频文件', extensions: TAG_FILE_EXTENSIONS },
      { name: '所有文件', extensions: ['*'] },
    ],
  })
  return typeof selected === 'string' ? selected : null
}
