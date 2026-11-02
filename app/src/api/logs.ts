import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type LogLevel = 'info' | 'success' | 'warning' | 'error'

/** 与 Rust 端 `LogEntry` 一一对应。 */
export interface LogEntry {
  /** 单调递增的编号，用来去重。 */
  id: number
  level: LogLevel
  message: string
  /** Unix 毫秒。 */
  timestampMs: number
}

/** 前端最多保留的日志条数（与后端一致）。 */
export const MAX_LOG_ENTRIES = 400

/** 后端当前保留的日志，用来补齐监听建立之前产生的内容。 */
export function getLogs(): Promise<LogEntry[]> {
  return invoke<LogEntry[]>('get_logs')
}

/** 订阅新日志。 */
export function onLog(handler: (entry: LogEntry) => void): Promise<UnlistenFn> {
  return listen<LogEntry>('app:log', (event) => handler(event.payload))
}
