import { useEffect, useState } from 'react'
import { getLogs, MAX_LOG_ENTRIES, onLog, type LogEntry } from '../api/logs'

/** 合并日志：按 id 去重、按 id 排序，只保留最近 MAX_LOG_ENTRIES 条。 */
export function mergeLogs(previous: LogEntry[], incoming: LogEntry[]): LogEntry[] {
  const seen = new Set(previous.map((entry) => entry.id))
  const fresh = incoming.filter((entry) => !seen.has(entry.id))
  if (fresh.length === 0) return previous
  return [...previous, ...fresh].sort((a, b) => a.id - b.id).slice(-MAX_LOG_ENTRIES)
}

/** 订阅后端的运行日志（先监听、再拉取已有日志，按 id 去重，避免漏掉或重复）。 */
export function useLogs(): LogEntry[] {
  const [logs, setLogs] = useState<LogEntry[]>([])

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    onLog((entry) => {
      if (!disposed) setLogs((previous) => mergeLogs(previous, [entry]))
    })
      .then((fn) => {
        if (disposed) fn()
        else unlisten = fn
        return getLogs()
      })
      .then((existing) => {
        if (!disposed) setLogs((previous) => mergeLogs(previous, existing))
      })
      .catch(() => undefined) // 非 Tauri 环境（纯浏览器预览）没有后端，静默忽略。

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  return logs
}
