import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import type { LyricsSourceKind, MatchThreshold } from './online'

// 歌词与标签编辑器、拖放与启动参数（模块 10b）：与 Rust 端 `lp_core::editor` 的 camelCase DTO 一一对应。

/** 打开编辑器时需要恢复的内容。 */
export interface EditorState {
  /** 上次使用的在线搜索来源。 */
  searchSource: LyricsSourceKind
  /** 上次选的歌词格式：逐字优先。 */
  preferWordLyrics: boolean
  /** 匹配度阈值（只显示）。 */
  threshold: MatchThreshold
  /** 批量窗口显示的来源优先级文案，如「网易云音乐优先」。 */
  tagPriorityText: string
  lyricsPriorityText: string
  /** 是否以管理员身份运行：Windows 会阻止资源管理器向本程序拖放文件。 */
  isElevated: boolean
  dragDropBlockedHint: string
  /** 受支持的扩展名（不带点）。 */
  extensions: string[]
  /** 如「MP3 / FLAC / M4A / MP4 / OGG / OPUS / WAV / AAC / WMA」。 */
  extensionHint: string
}

export function getEditorState(): Promise<EditorState> {
  return invoke<EditorState>('editor_get_state')
}

/** 记住编辑器的搜索来源与歌词格式（只改这两个设置；失败后端只记日志）。 */
export function rememberEditorOptions(source: LyricsSourceKind, preferWordLyrics: boolean): Promise<void> {
  return invoke<void>('editor_remember_options', { source, preferWordLyrics })
}

/** 拖入的文件 / 文件夹的解析结果。 */
export interface DropResolution {
  /** `audio`：解析出了可编辑的音频；`ncm`：只有 .ncm（需要先解密）；`error`：没有可用目标。 */
  kind: 'audio' | 'ncm' | 'error'
  path: string | null
  fileName: string | null
  /** 给运行日志 / 状态栏的一句话。 */
  message: string
  /** 能解析成音频的目标数 / 拖入总数。 */
  resolvedCount: number
  total: number
}

/** 把拖入的路径解析成要载入的音频文件；`log` 为真时后端顺便写运行日志（主窗口拖放）。 */
export function resolveDrop(targets: string[], log: boolean): Promise<DropResolution> {
  return invoke<DropResolution>('resolve_drop', { targets, log })
}

/** 读进来的封面图片文件（Base64，不带 `data:` 前缀）。 */
export interface CoverFile {
  mimeType: string
  base64: string
  size: number
  fileName: string
}

/** 读取用户选的封面图片；失败抛中文字符串。 */
export function readCoverFile(path: string): Promise<CoverFile> {
  return invoke<CoverFile>('read_cover_file', { path })
}

let startupFile: Promise<string | null> | null = null

/**
 * 命令行 `--lyrics/--edit <文件>` 指定的文件。后端只交出一次，所以这里缓存同一个 Promise
 * （React 严格模式会让副作用跑两遍）；没有或不在 Tauri 环境里为 null。
 */
export function takeStartupFile(): Promise<string | null> {
  startupFile ??= invoke<string | null>('take_startup_file').catch(() => null)
  return startupFile
}

/** 弹出系统「选择图片」对话框；取消返回 null。 */
export async function pickCoverImage(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    title: '选择封面图片',
    filters: [
      { name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'] },
      { name: '所有文件', extensions: ['*'] },
    ],
  })
  return typeof selected === 'string' ? selected : null
}

/** 弹出系统「选择文件夹」对话框；取消返回 null。 */
export async function pickMusicFolder(title: string): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false, title })
  return typeof selected === 'string' ? selected : null
}

/** 任意失败对象里的中文说明（后端 Err 可能是字符串或 `{ message }`）。 */
export function errorMessage(raw: unknown, fallback: string): string {
  if (typeof raw === 'string' && raw.trim() !== '') return raw
  if (raw && typeof raw === 'object' && 'message' in raw) {
    const message = (raw as { message: unknown }).message
    if (typeof message === 'string' && message.trim() !== '') return message
  }
  return fallback
}

/** 从路径取文件名。 */
export function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path
}

/** 路径所在的文件夹；没有分隔符时为空串。 */
export function directoryOf(path: string): string {
  const index = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'))
  return index > 0 ? path.slice(0, index) : ''
}

/** 把文件名清理成搜索关键词（同 C# `BuildSearchKeywordFromFileName`）：去扩展名与音轨前缀，下划线换空格，合并空白。 */
export function keywordFromFileName(path: string): string {
  const base = fileNameOf(path).trim()
  const dot = base.lastIndexOf('.')
  const stem = (dot > 0 ? base.slice(0, dot) : base).trim()
  if (stem === '') return ''
  return stem
    .replace(/^\s*\d{1,3}[\s._-]+/, '')
    .replace(/_/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}
