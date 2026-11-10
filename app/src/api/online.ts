import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

// 在线匹配（模块 10a）：歌词与标签编辑器、批量修改标签窗口共用的后端命令与类型。
// 全部与 Rust 端（lp-core::online / lp-core::lyrics::batch）的 camelCase DTO 一一对应。

/** 歌词检索来源：主词源（网易云、QQ 音乐，可带回标签与封面）与备用词源（酷狗、LRCLIB，只提供歌词）。 */
export type LyricsSourceKind = 'Netease' | 'QQMusic' | 'Kugou' | 'Lrclib'

/** 主词源（`online_search` 只接受这两个）。 */
export type PrimarySource = 'Netease' | 'QQMusic'

/** 下拉框与状态提示里显示的来源名（与 C# `LyricsSourceKind.ToDisplayLabel` 一致）。 */
export const LYRICS_SOURCE_LABELS: Record<LyricsSourceKind, string> = {
  Netease: '网易云音乐',
  QQMusic: 'QQ 音乐',
  Kugou: '酷狗音乐（仅歌词）',
  Lrclib: 'LRCLIB（仅歌词）',
}

/** 是否为备用词源（只补歌词，不改写标签与封面）。 */
export function isFallbackSource(kind: LyricsSourceKind): boolean {
  return kind === 'Kugou' || kind === 'Lrclib'
}

/** 在线命令失败时 `invoke` 抛出的对象。 */
export interface OnlineCommandError {
  /** `emptyQuery` / `fallbackSource` / `primarySource` / `noFile` / `unknownSong` / `cancelled` / `failed`。 */
  kind: string
  /** 中文说明，可直接展示。 */
  message: string
}

/** 取失败原因文案（后端 `Err` 是 `{ kind, message }`，其他形态兜底）。 */
export function onlineErrorText(raw: unknown, fallback: string): string {
  if (raw && typeof raw === 'object' && 'message' in raw) {
    const message = (raw as { message: unknown }).message
    if (typeof message === 'string' && message.trim() !== '') return message
  }
  if (typeof raw === 'string' && raw.trim() !== '') return raw
  if (raw instanceof Error && raw.message.trim() !== '') return raw.message
  return fallback
}

// ───────────────────────────── 搜索 ─────────────────────────────

/** 本地文件信息，用来给搜索结果打分（编辑器打开了文件才有）。 */
export interface FileContext {
  /** 标签里的标题；留空则用文件名推。 */
  title?: string | null
  artist?: string | null
  album?: string | null
  /** 毫秒，未知为 0。 */
  durationMs: number
  /** 文件路径（标题为空时用文件名推标题 / 默认关键词）。 */
  filePath?: string | null
}

/** 一首在线歌曲（搜索结果的一行）。 */
export interface OnlineSong {
  source: PrimarySource
  /** 来源里的歌曲 ID（网易云是数字 ID，QQ 是 `songmid`）。传给 `onlineLoadResult` 的 `id`。 */
  sourceId: string
  title: string
  artist: string
  album: string
  /** 毫秒，未知为 0。 */
  durationMs: number
  year: number | null
  trackNumber: number | null
  discNumber: number | null
  genre: string | null
  composer: string | null
  coverUrl: string | null
  /** QQ 音乐的数值 song id。 */
  numericSourceId: string | null
  /** 来源名（「网易云音乐」「QQ 音乐」）。 */
  sourceLabel: string
  /** 列表里显示的歌名（空白时是「未知歌曲」）。 */
  displayTitle: string
  /** 「歌手 · 专辑」。 */
  subtitle: string
  /** `m:ss`，未知为 `--:--`。 */
  durationText: string
}

/** 匹配度：四路分值（0-1）与加权总分。 */
export interface MatchScore {
  title: number
  artist: number
  album: number
  duration: number
  total: number
  /** 总分的百分数文本，如「87」。 */
  percent: string
  /** 被直接排除（伴奏 / 纯音乐）。 */
  rejected: boolean
  rejectReason: string | null
  /** 标题与歌手都可信。 */
  confident: boolean
}

export interface OnlineSearchItem {
  song: OnlineSong
  /** 没有 `fileContext` 时为 `null`。 */
  score: MatchScore | null
  /** 总分低于匹配度阈值（界面标注“匹配度较低”）。 */
  belowThreshold: boolean
}

export interface OnlineSearchResponse {
  source: LyricsSourceKind
  /** 实际使用的关键词（请求里留空时由文件名推出）。 */
  query: string
  /** 是否按本地文件打了分；打分时 `items` 已按总分从高到低排序。 */
  scored: boolean
  /** 设置里的匹配度阈值（0.30–1.00）。 */
  threshold: number
  /** 阈值的百分数文本，如「60」。 */
  thresholdPercent: string
  items: OnlineSearchItem[]
}

/**
 * 搜索歌曲（只接受主词源）。带了 `fileContext` 就给结果打分并排序；`query` 留空时用文件名推关键词。
 * 失败抛 `{ kind, message }`：`emptyQuery` / `fallbackSource` / `failed`。
 */
export function onlineSearch(
  source: LyricsSourceKind,
  query: string,
  fileContext?: FileContext | null,
): Promise<OnlineSearchResponse> {
  return invoke<OnlineSearchResponse>('online_search', { source, query, fileContext: fileContext ?? null })
}

// ───────────────────────────── 取详情 ─────────────────────────────

/** 封面（Base64，不带 `data:` 前缀；可直接填进 `write_tags` 的 `coverBase64` / `coverMimeType`）。 */
export interface OnlineCover {
  mimeType: string
  base64: string
  /** 字节数。 */
  size: number
  /** 像素尺寸；认不出时为 null。 */
  width: number | null
  height: number | null
}

export interface LoadedResult {
  /** 详情接口返回的歌曲信息（字段比搜索摘要更全，用它填标签）。 */
  song: OnlineSong
  /** 按偏好选好的歌词（双语优先；逐字不可用时回退逐行）；没有歌词为 `null`。 */
  lyrics: string | null
  /** 歌词格式说明，如「双语逐字歌词」「逐行歌词（该来源无逐字）（该来源无翻译）」。 */
  lyricsLabel: string | null
  /** 请求了逐字但只有逐行。 */
  wordFallback: boolean
  hasWordLyrics: boolean
  hasTranslation: boolean
  lineLyrics: string | null
  wordLyrics: string | null
  bilingualLineLyrics: string | null
  bilingualWordLyrics: string | null
  cover: OnlineCover | null
  /** 与桌面版一致的状态栏文案：「已载入QQ 音乐的标签双语逐字歌词、封面，请确认后保存。」 */
  statusText: string
}

/**
 * 取一首搜索结果的标签、歌词与封面。`id` 是搜索结果的 `sourceId`；`song` 可选地传回搜索结果摘要
 * （不传就从最近搜索的缓存里找，QQ 音乐必须先搜索过）。
 * 失败抛 `{ kind, message }`：`unknownSong` / `primarySource` / `failed`。
 */
export function onlineLoadResult(
  source: LyricsSourceKind,
  id: string,
  wordLyrics: boolean,
  song?: OnlineSong | null,
): Promise<LoadedResult> {
  return invoke<LoadedResult>('online_load_result', { source, id, wordLyrics, song: song ?? null })
}

// ───────────────────────────── 备用词源 ─────────────────────────────

export interface FallbackLyrics {
  /** 「酷狗音乐」「LRCLIB」。 */
  sourceLabel: string
  title: string
  artist: string
  album: string
  durationMs: number
  score: MatchScore
  lyrics: string | null
  lyricsLabel: string | null
  hasWordLyrics: boolean
  hasTranslation: boolean
  lineLyrics: string | null
  wordLyrics: string | null
  bilingualLineLyrics: string | null
  bilingualWordLyrics: string | null
  /** 如「已从酷狗音乐载入双语逐字歌词（仅歌词，标签与封面保持当前内容），请确认后保存。」 */
  statusText: string
}

/**
 * 备用词源（`Kugou` / `Lrclib`，只提供歌词）按本地标题、歌手与时长检索一份歌词；没有匹配返回 `null`。
 * 失败抛 `{ kind, message }`：`noFile`（标题为空）/ `primarySource` / `failed`。
 */
export function lyricsSourceFetch(
  source: LyricsSourceKind,
  title: string,
  artist: string,
  durationMs: number,
  options: { album?: string | null; wordLyrics?: boolean } = {},
): Promise<FallbackLyrics | null> {
  return invoke<FallbackLyrics | null>('lyrics_source_fetch', {
    source,
    title,
    artist,
    durationMs,
    album: options.album ?? null,
    wordLyrics: options.wordLyrics ?? false,
  })
}

/** 当前设置里的在线匹配度阈值。 */
export interface MatchThreshold {
  /** 0.30–1.00。 */
  threshold: number
  /** 百分数文本，如「60」。 */
  percent: string
  min: number
  max: number
  default: number
}

export function getMatchThreshold(): Promise<MatchThreshold> {
  return invoke<MatchThreshold>('get_match_threshold')
}

// ───────────────────────────── 批量获取 ─────────────────────────────

/** 来源优先级（与 `settings.json` 里的名字一致）。 */
export type SourcePriority = 'NeteaseFirst' | 'QQMusicFirst' | 'NeteaseOnly' | 'QQMusicOnly'

export interface BatchFile {
  path: string
  name: string
  /** 字节。 */
  size: number
}

export interface BatchScan {
  folder: string
  recursive: boolean
  /** 目录为空白或不存在。 */
  directoryMissing: boolean
  /** 支持的音频文件（`.mp3 .flac .m4a .ogg .wav .aac .wma .opus`），按路径排序。 */
  files: BatchFile[]
}

export interface BatchRequest {
  /** 要处理的文件（取自 `batchScan` 的结果）。 */
  files: string[]
  /** 标签 / 封面来源次序；不传用设置里的值。 */
  tagPriority?: SourcePriority
  /** 歌词来源次序；不传用设置里的值。 */
  lyricsPriority?: SourcePriority
  /** 覆盖已有的标签、封面与歌词（默认 false：只补缺失）。 */
  overwriteExisting?: boolean
  /** 是否获取歌词（默认 true）。 */
  fetchLyrics?: boolean
  /** 优先逐字歌词（默认 false）。 */
  preferWordLyrics?: boolean
}

export type BatchStatus = 'success' | 'skipped' | 'failed'

export interface BatchResult {
  filePath: string
  fileName: string
  status: BatchStatus
  success: boolean
  skipped: boolean
  /** 中文说明，如「已写入标签、封面、双语逐字歌词（歌词来源：酷狗音乐）」「标签、封面与歌词已完整，跳过」。 */
  message: string
  matchedTitle: string | null
  matchedArtist: string | null
  /** 来源名（「网易云音乐」「QQ 音乐」「酷狗音乐」「LRCLIB」）。 */
  sourceLabel: string | null
  /** 写入的歌词格式，如「双语逐字歌词」。 */
  lyricsFormat: string | null
}

/** `batch:progress` 事件的载荷。 */
export interface BatchProgress {
  /** 已处理完的文件数（`finished` 事件才会增加）。 */
  completed: number
  total: number
  /** 这个文件在请求里的下标（去重后）。 */
  index: number
  path: string
  fileName: string
  /** `started`（开始处理）/ `finished`（处理完，`result` 有值）。 */
  stage: 'started' | 'finished'
  result: BatchResult | null
}

export interface BatchSummary {
  /** 请求的文件总数（去重后）。 */
  total: number
  /** 实际处理完的文件数（取消时小于 `total`）。 */
  processed: number
  success: number
  skipped: number
  failed: number
  cancelled: boolean
  /** 已处理文件的结果，按请求顺序。 */
  results: BatchResult[]
}

/** 批量任务失败时抛出的对象：`busy`（已有任务在跑）/ `noFiles`。 */
export interface BatchJobError {
  kind: 'busy' | 'noFiles'
  message: string
}

/** 扫描文件夹里的音频文件。 */
export function batchScan(folder: string, recursive: boolean): Promise<BatchScan> {
  return invoke<BatchScan>('batch_scan', { folder, recursive })
}

/**
 * 批量获取标签、封面与歌词：最多 3 个文件并发，进度走 `batch:progress` 事件，返回时给出汇总
 * （被取消时是已处理的部分）。匹配度阈值取自设置。
 */
export function batchRun(options: BatchRequest): Promise<BatchSummary> {
  return invoke<BatchSummary>('batch_run', { options })
}

/** 取消正在进行的批量任务；没有任务时返回 false。 */
export function batchCancel(): Promise<boolean> {
  return invoke<boolean>('batch_cancel')
}

/** 订阅批量进度；返回取消订阅函数。 */
export function onBatchProgress(handler: (progress: BatchProgress) => void): Promise<UnlistenFn> {
  return listen<BatchProgress>('batch:progress', (event) => handler(event.payload))
}
