import { useEffect, useRef, useState } from 'react'
import type { LogEntry, LogLevel } from '../api/logs'

const LEVEL_LABEL: Record<LogLevel, string> = {
  info: '信息',
  success: '成功',
  warning: '警告',
  error: '错误',
}

function formatTime(timestampMs: number): string {
  const date = new Date(timestampMs)
  const two = (value: number) => String(value).padStart(2, '0')
  return `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`
}

/** 底部运行日志：最多 400 条（内存中），新日志到来时若用户停在底部则自动滚动。 */
export default function LogPanel({ logs }: { logs: LogEntry[] }) {
  const [expanded, setExpanded] = useState(true)
  const listRef = useRef<HTMLOListElement>(null)
  const stickToBottom = useRef(true)

  useEffect(() => {
    const list = listRef.current
    if (list && stickToBottom.current) list.scrollTop = list.scrollHeight
  }, [logs, expanded])

  return (
    <section className={`log-panel${expanded ? '' : ' log-panel-collapsed'}`} aria-label="运行日志">
      <header className="log-header">
        <h2 className="log-title">运行日志</h2>
        <span className="log-count" aria-label={`共 ${logs.length} 条日志`}>
          {logs.length} 条
        </span>
        <button
          type="button"
          className="btn btn-ghost btn-small"
          aria-label={expanded ? '收起运行日志' : '展开运行日志'}
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? '收起' : '展开'}
        </button>
      </header>
      {expanded && (
        <ol
          ref={listRef}
          className="log-list"
          role="log"
          aria-label="日志列表"
          aria-live="polite"
          tabIndex={0}
          onScroll={(event) => {
            const list = event.currentTarget
            stickToBottom.current = list.scrollHeight - list.scrollTop - list.clientHeight < 24
          }}
        >
          {logs.length === 0 && <li className="log-empty">暂无日志</li>}
          {logs.map((entry) => (
            <li key={entry.id} className={`log-item log-${entry.level}`}>
              <time className="log-time" dateTime={new Date(entry.timestampMs).toISOString()}>
                {formatTime(entry.timestampMs)}
              </time>
              <span className="log-level">{LEVEL_LABEL[entry.level]}</span>
              <span className="log-message">{entry.message}</span>
            </li>
          ))}
        </ol>
      )}
    </section>
  )
}
