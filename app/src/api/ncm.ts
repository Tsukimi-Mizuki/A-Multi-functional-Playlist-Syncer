import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open } from '@tauri-apps/plugin-dialog'

/** 与 Rust 端 `NcmDialogState`（camelCase）一一对应：打开对话框时恢复的内容。 */
export interface NcmDialogState {
  /** 上次记住的输入文件夹。 */
  folders: string[]
  /** 输出目录；记住的目录为空时是下载目录。 */
  outputDirectory: string
  downloadDirectory: string
  /** 在线匹配是否可用（后端装配了在线补全）。 */
  onlineMatchAvailable: boolean
  onlineMatchHint: string
}

export interface NcmFileEntry {
  path: string
  name: string
  /** 字节。 */
  size: number
}

export interface NcmScanResult {
  files: NcmFileEntry[]
  /** 不存在（或不是文件夹）的输入文件夹。 */
  missingFolders: string[]
  /** 因权限不足等原因跳过的子文件夹数。 */
  skippedDirectories: number
}

/** 一个文件的最终结果。 */
export interface NcmFileResult {
  /** 源 `.ncm` 的完整路径。 */
  path: string
  success: boolean
  outputPath: string | null
  outputFileName: string | null
  /** 小写 `mp3` / `flac`。 */
  format: string | null
  errorMessage: string | null
  enrichmentMessage: string | null
  title: string | null
  artists: string | null
  album: string | null
}

export type NcmStage = 'decrypting' | 'matching' | 'finished'

/** `ncm:progress` 事件的载荷。 */
export interface NcmProgress {
  completed: number
  total: number
  index: number
  path: string
  fileName: string
  stage: NcmStage
  message: string
  /** `finished` 时才有。 */
  result: NcmFileResult | null
}

export interface NcmSummary {
  results: NcmFileResult[]
  total: number
  succeeded: number
  failed: number
  cancelled: boolean
  outputDirectory: string
  /** 带有在线匹配成功说明的数量。 */
  matchedOnline: number
}

export interface NcmImportRequest {
  paths: string[]
  folders: string[]
  outputDirectory: string
  overwrite: boolean
  autoMatchOnline: boolean
}

/** 后端 Err 是中文字符串；其他形态一律兜底成通用提示。 */
export function ncmErrorText(raw: unknown, fallback: string): string {
  if (typeof raw === 'string' && raw.trim() !== '') return raw
  if (raw instanceof Error && raw.message.trim() !== '') return raw.message
  return fallback
}

/** 打开对话框时读取记住的文件夹 / 输出目录与在线匹配状态。 */
export function getNcmState(): Promise<NcmDialogState> {
  return invoke<NcmDialogState>('ncm_get_state')
}

/** 递归扫描这些文件夹里的 `.ncm` 文件。 */
export function scanNcm(folders: string[]): Promise<NcmScanResult> {
  return invoke<NcmScanResult>('ncm_scan', { folders })
}

/** 记住输入文件夹与输出目录（失败后端只记日志，不抛）。 */
export function saveNcmState(folders: string[], outputDirectory: string): Promise<void> {
  return invoke<void>('ncm_save_state', { folders, outputDirectory })
}

/** 开始导入；进度走 `ncm:progress` 事件，返回时给出汇总（被取消时是已完成的部分）。 */
export function startNcmImport(request: NcmImportRequest): Promise<NcmSummary> {
  return invoke<NcmSummary>('ncm_import', { request })
}

/** 取消正在进行的导入；没有任务时返回 false。 */
export function cancelNcmImport(): Promise<boolean> {
  return invoke<boolean>('ncm_cancel')
}

/** 订阅导入进度；返回取消订阅函数。 */
export function onNcmProgress(handler: (progress: NcmProgress) => void): Promise<UnlistenFn> {
  return listen<NcmProgress>('ncm:progress', (event) => handler(event.payload))
}

/** 系统「选择文件夹」对话框（可多选）；取消返回空数组。 */
export async function pickNcmFolders(): Promise<string[]> {
  const selected = await open({ directory: true, multiple: true, title: '选择包含 .ncm 文件的文件夹' })
  if (Array.isArray(selected)) return selected
  return typeof selected === 'string' ? [selected] : []
}

/** 系统「选择文件夹」对话框（单选）；取消返回 null。 */
export async function pickOutputFolder(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false, title: '选择解密输出目录' })
  return typeof selected === 'string' ? selected : null
}
