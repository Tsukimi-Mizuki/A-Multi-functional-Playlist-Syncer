import { memo, useCallback, useEffect, useRef, useState } from 'react'
import { errorMessage, getEditorState, pickMusicFolder, type EditorState } from '../api/editor'
import {
  batchCancel,
  batchRun,
  batchScan,
  onBatchProgress,
  type BatchFile,
  type BatchProgress,
  type BatchResult,
} from '../api/online'
import { useVirtualRows } from '../lib/virtual'
import Modal from './Modal'

/** 固定行高（px）：虚拟滚动依赖它，样式里的 `--ncm-row-height` 必须与此一致（复用 NCM 导入的表格样式）。 */
export const BATCH_ROW_HEIGHT = 38

/** 路径比较用的键：Windows 路径不区分大小写，也不区分 `/` 与 `\`。 */
function pathKey(path: string): string {
  return path.replace(/\//g, '\\').toLowerCase()
}

type RowState =
  | { kind: 'pending' }
  | { kind: 'running' }
  | { kind: 'done'; result: BatchResult }
  | { kind: 'cancelled' }

const PENDING: RowState = { kind: 'pending' }

/** 一行的展示文字与颜色（同 C# `BatchLyricsItemViewModel`：成功绿、跳过灰、失败红）。 */
function describeRow(state: RowState): {
  matched: string
  source: string
  status: string
  tone: 'muted' | 'busy' | 'success' | 'error'
} {
  switch (state.kind) {
    case 'pending':
      return { matched: '', source: '', status: '等待处理', tone: 'muted' }
    case 'running':
      return { matched: '', source: '', status: '正在匹配…', tone: 'busy' }
    case 'cancelled':
      return { matched: '', source: '', status: '已取消', tone: 'muted' }
    case 'done': {
      const { result } = state
      const matched = result.matchedTitle
        ? result.matchedArtist
          ? `${result.matchedTitle} - ${result.matchedArtist}`
          : result.matchedTitle
        : ''
      return {
        matched,
        source: result.sourceLabel ?? '',
        status: result.message,
        tone: result.success ? 'success' : result.skipped ? 'muted' : 'error',
      }
    }
  }
}

const BatchRow = memo(function BatchRow({ file, state, index }: { file: BatchFile; state: RowState; index: number }) {
  const view = describeRow(state)
  return (
    <div
      className="ncm-row batch-grid"
      role="row"
      aria-rowindex={index + 2}
      style={{ transform: `translateY(${index * BATCH_ROW_HEIGHT}px)` }}
    >
      <span className="ncm-cell ncm-name" role="cell" title={file.path}>
        {file.name}
      </span>
      <span className="ncm-cell" role="cell" title={view.matched}>
        {view.matched}
      </span>
      <span className="ncm-cell ncm-status-muted" role="cell">
        {view.source}
      </span>
      <span
        className={`ncm-cell ncm-status-${view.tone}`}
        role="cell"
        title={view.status}
        aria-label={`${file.name} 的状态：${view.status}`}
      >
        {view.status}
      </span>
    </div>
  )
})

function BatchTable({ files, rows }: { files: BatchFile[]; rows: Map<string, RowState> }) {
  const { scrollerRef, onScroll, range, totalHeight } = useVirtualRows(files.length, BATCH_ROW_HEIGHT)
  const visible = files.slice(range.start, range.end)
  return (
    <div className="ncm-table" role="table" aria-label="批量获取结果列表" aria-rowcount={files.length + 1}>
      <div className="ncm-head batch-grid" role="row" aria-rowindex={1}>
        <span className="ncm-cell" role="columnheader">
          文件
        </span>
        <span className="ncm-cell" role="columnheader">
          匹配歌曲
        </span>
        <span className="ncm-cell" role="columnheader">
          来源
        </span>
        <span className="ncm-cell" role="columnheader">
          状态
        </span>
      </div>
      <div
        ref={scrollerRef}
        className="ncm-scroller"
        role="rowgroup"
        tabIndex={0}
        aria-label="批量获取滚动区域"
        onScroll={onScroll}
      >
        {files.length === 0 && (
          <p className="ncm-empty">还没有文件。选择音乐目录并点击「开始批量获取」后会在这里列出。</p>
        )}
        <div className="ncm-spacer" style={{ height: totalHeight }}>
          {visible.map((file, offset) => (
            <BatchRow
              key={file.path}
              file={file}
              index={range.start + offset}
              state={rows.get(pathKey(file.path)) ?? PENDING}
            />
          ))}
        </div>
      </div>
    </div>
  )
}

/**
 * 批量获取标签、封面与歌词（对应 C# 的 `BatchLyricsWindow`）：选择目录 → 扫描 → 最多 3 个文件并发匹配，
 * 逐文件状态走 `batch:progress` 事件，列表虚拟滚动，可取消。叠在编辑器之上，以编辑器为父面板。
 */
export default function BatchLyricsDialog({ initialFolder, onClose }: { initialFolder: string; onClose: () => void }) {
  const [config, setConfig] = useState<EditorState | null>(null)
  const [folder, setFolder] = useState(initialFolder)
  const [recursive, setRecursive] = useState(true)
  const [overwrite, setOverwrite] = useState(true)
  const [fetchLyrics, setFetchLyrics] = useState(true)
  const [preferWord, setPreferWord] = useState(false)
  const [files, setFiles] = useState<BatchFile[]>([])
  const [rows, setRows] = useState<Map<string, RowState>>(new Map())
  const [counts, setCounts] = useState({ processed: 0, success: 0, skipped: 0, failed: 0 })
  const [running, setRunning] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [percent, setPercent] = useState(0)
  const [status, setStatus] = useState('选择目录后开始批量获取标签、封面与歌词。')
  const [problem, setProblem] = useState(false)
  const runningRef = useRef(false)

  useEffect(() => {
    let cancelled = false
    getEditorState()
      .then((state) => {
        if (!cancelled) setConfig(state)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [])

  const browse = useCallback(() => {
    pickMusicFolder('选择要批量匹配标签与封面的目录')
      .then((picked) => {
        if (picked) setFolder(picked)
      })
      .catch((error: unknown) => {
        setProblem(true)
        setStatus(`选择目录失败：${errorMessage(error, '请重试')}`)
      })
  }, [])

  const handleProgress = useCallback((progress: BatchProgress) => {
    const key = pathKey(progress.path)
    setRows((previous) => {
      const next = new Map(previous)
      if (progress.stage === 'finished' && progress.result) next.set(key, { kind: 'done', result: progress.result })
      else if (progress.stage === 'started') next.set(key, { kind: 'running' })
      return next
    })
    if (progress.stage === 'finished' && progress.result) {
      const result = progress.result
      setCounts((previous) => ({
        processed: previous.processed + 1,
        success: previous.success + (result.success ? 1 : 0),
        skipped: previous.skipped + (!result.success && result.skipped ? 1 : 0),
        failed: previous.failed + (!result.success && !result.skipped ? 1 : 0),
      }))
    }
    if (progress.total > 0) setPercent(Math.min(100, (progress.completed * 100) / progress.total))
  }, [])

  const start = useCallback(async () => {
    if (runningRef.current) return
    if (folder.trim() === '') {
      setProblem(true)
      setStatus('请选择有效的音乐目录。')
      return
    }
    runningRef.current = true
    setRunning(true)
    setCancelling(false)
    setProblem(false)
    setFiles([])
    setRows(new Map())
    setCounts({ processed: 0, success: 0, skipped: 0, failed: 0 })
    setPercent(0)
    setStatus('正在扫描音频文件...')
    let unlisten: (() => void) | undefined
    try {
      const scan = await batchScan(folder, recursive)
      if (scan.directoryMissing) {
        setProblem(true)
        setStatus('请选择有效的音乐目录。')
        return
      }
      setFiles(scan.files)
      if (scan.files.length === 0) {
        setStatus('目录中没有找到支持的音频文件。')
        return
      }
      setStatus('正在批量获取标签、封面与歌词...')
      unlisten = await onBatchProgress(handleProgress)
      const summary = await batchRun({
        files: scan.files.map((file) => file.path),
        overwriteExisting: overwrite,
        fetchLyrics,
        preferWordLyrics: preferWord,
      })
      // 事件与命令返回值不保证先后：以汇总里的结果为准，取消后没处理到的文件标成「已取消」。
      setRows(() => {
        const next = new Map<string, RowState>()
        for (const result of summary.results) next.set(pathKey(result.filePath), { kind: 'done', result })
        if (summary.cancelled) {
          for (const file of scan.files) {
            if (!next.has(pathKey(file.path))) next.set(pathKey(file.path), { kind: 'cancelled' })
          }
        }
        return next
      })
      setCounts({
        processed: summary.processed,
        success: summary.success,
        skipped: summary.skipped,
        failed: summary.failed,
      })
      setPercent(summary.total > 0 ? Math.min(100, (summary.processed * 100) / summary.total) : 100)
      setStatus(
        summary.cancelled
          ? '已取消批量标签获取。'
          : `完成：成功 ${summary.success}，跳过 ${summary.skipped}，失败 ${summary.failed}。`,
      )
      setProblem(!summary.cancelled && summary.failed > 0)
    } catch (error) {
      setProblem(true)
      setStatus(`批量标签获取失败：${errorMessage(error, '请重试')}`)
    } finally {
      unlisten?.()
      runningRef.current = false
      setRunning(false)
      setCancelling(false)
    }
  }, [folder, recursive, overwrite, fetchLyrics, preferWord, handleProgress])

  const cancel = useCallback(() => {
    setCancelling(true)
    setStatus('正在取消…当前文件处理完后停止')
    batchCancel().catch(() => undefined)
  }, [])

  const close = useCallback(() => {
    if (runningRef.current) batchCancel().catch(() => undefined)
    onClose()
  }, [onClose])

  const canStart = !running && folder.trim() !== ''
  const progressText = `已处理 ${counts.processed}/${files.length} · 成功 ${counts.success} · 跳过 ${counts.skipped} · 失败 ${counts.failed}`

  return (
    <Modal
      title="批量获取标签、封面与歌词"
      className="modal-wide ncm-dialog batch-dialog"
      onClose={close}
      footer={
        <>
          <span className="ncm-count" aria-live="polite">
            {progressText}
          </span>
          {running ? (
            <button
              type="button"
              className="btn btn-danger"
              aria-label="取消批量获取"
              disabled={cancelling}
              onClick={cancel}
            >
              {cancelling ? '正在取消…' : '取消'}
            </button>
          ) : (
            <button
              type="button"
              className="btn btn-primary"
              aria-label="开始批量获取标签、封面与歌词"
              disabled={!canStart}
              onClick={() => void start()}
            >
              开始批量获取
            </button>
          )}
          <button type="button" className="btn" aria-label="关闭批量标签窗口" onClick={close}>
            关闭
          </button>
        </>
      }
    >
      <p className="ncm-intro">
        扫描本地音频，按标签/文件名从网易云或 QQ 音乐匹配歌曲，批量写入标签、封面和歌词；来源顺序由设置中的优先级决定。
      </p>

      <section className="ncm-section" aria-label="批量获取设置">
        <h3 className="ncm-label">音乐目录</h3>
        <div className="ncm-output">
          <input
            type="text"
            className="ncm-input"
            aria-label="音乐目录"
            spellCheck={false}
            placeholder="选择包含音频文件的目录"
            value={folder}
            disabled={running}
            onChange={(event) => setFolder(event.target.value)}
          />
          <button type="button" className="btn" aria-label="浏览音乐目录" disabled={running} onClick={browse}>
            浏览…
          </button>
        </div>
        <div className="batch-options">
          <span className="ncm-sub" aria-label="来源优先级">
            来源优先级 标签 {config?.tagPriorityText ?? '网易云音乐优先'} · 歌词{' '}
            {config?.lyricsPriorityText ?? '网易云音乐优先'}
          </span>
          <label className="ncm-check">
            <input
              type="checkbox"
              aria-label="覆盖已有标签与歌词"
              checked={overwrite}
              disabled={running}
              onChange={(event) => setOverwrite(event.target.checked)}
            />
            <span>覆盖已有标签与歌词</span>
          </label>
          <label className="ncm-check">
            <input
              type="checkbox"
              aria-label="同时获取歌词"
              checked={fetchLyrics}
              disabled={running}
              onChange={(event) => setFetchLyrics(event.target.checked)}
            />
            <span>同时获取歌词</span>
          </label>
          <select
            className="editor-select"
            aria-label="批量歌词格式"
            value={preferWord ? 'word' : 'line'}
            disabled={running || !fetchLyrics}
            onChange={(event) => setPreferWord(event.target.value === 'word')}
          >
            <option value="line">逐行歌词</option>
            <option value="word">逐字歌词（有则优先）</option>
          </select>
          <label className="ncm-check">
            <input
              type="checkbox"
              aria-label="包含子文件夹"
              checked={recursive}
              disabled={running}
              onChange={(event) => setRecursive(event.target.checked)}
            />
            <span>包含子文件夹</span>
          </label>
        </div>
        <p className="ncm-sub" aria-label="批量匹配度阈值">
          匹配度阈值 {config?.threshold.percent ?? '60'}%（在「设置」的「在线匹配度」里调整，低于该值的歌曲会被跳过）
        </p>
      </section>

      <div className="ncm-progress-block" aria-busy={running}>
        <p
          className={problem ? 'ncm-status-line ncm-status-line-problem' : 'ncm-status-line'}
          role="status"
          aria-live="polite"
        >
          {status}
        </p>
        <progress className="ncm-progress" aria-label="批量获取进度" value={percent} max={100} />
      </div>

      <BatchTable files={files} rows={rows} />
    </Modal>
  )
}
