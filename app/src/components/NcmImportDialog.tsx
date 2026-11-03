import { memo, useCallback, useEffect, useRef, useState } from 'react'
import {
  cancelNcmImport,
  getNcmState,
  ncmErrorText,
  onNcmProgress,
  pickNcmFolders,
  pickOutputFolder,
  saveNcmState,
  scanNcm,
  startNcmImport,
  type NcmFileEntry,
  type NcmFileResult,
  type NcmProgress,
  type NcmSummary,
} from '../api/ncm'
import { formatFileSize } from '../lib/format'
import { useVirtualRows } from '../lib/virtual'
import Modal from './Modal'

/** 固定行高（px）：虚拟滚动依赖它，样式里的 `--ncm-row-height` 必须与此一致。 */
export const NCM_ROW_HEIGHT = 38

const AUTO_MATCH_KEY = 'lp.ncm.autoMatch'
const INITIAL_STATUS = '请选择包含 .ncm 文件的文件夹'

/** 「解密后自动在线匹配」只是每个用户自己的界面偏好（C# 版每次打开都默认勾选）：存取失败时静默回退为勾选。 */
function readAutoMatch(): boolean {
  try {
    return window.localStorage.getItem(AUTO_MATCH_KEY) !== '0'
  } catch {
    return true
  }
}

function writeAutoMatch(value: boolean): void {
  try {
    window.localStorage.setItem(AUTO_MATCH_KEY, value ? '1' : '0')
  } catch {
    // 偏好存不下来不影响使用。
  }
}

/** 路径比较用的键：Windows 路径不区分大小写，也不区分 `/` 与 `\`。 */
function pathKey(path: string): string {
  return path.replace(/\//g, '\\').toLowerCase()
}

/** 逐文件状态。 */
type RowState =
  | { kind: 'pending' }
  | { kind: 'running' }
  | { kind: 'matching' }
  | { kind: 'done'; result: NcmFileResult }
  | { kind: 'cancelled' }

const PENDING: RowState = { kind: 'pending' }

/** 状态文案与样式，逐字对应 C# 的 `NcmImportResultRow.StatusText`。 */
function describeRow(state: RowState): { text: string; tone: 'muted' | 'busy' | 'success' | 'error'; title: string } {
  switch (state.kind) {
    case 'pending':
      return { text: '待处理', tone: 'muted', title: '' }
    case 'running':
      return { text: '解密中…', tone: 'busy', title: '' }
    case 'matching':
      return { text: '正在匹配在线标签、封面与歌词…', tone: 'busy', title: '' }
    case 'cancelled':
      return { text: '已取消', tone: 'muted', title: '' }
    case 'done': {
      const { result } = state
      if (!result.success) {
        const reason = result.errorMessage ?? '未知错误'
        return { text: `失败：${reason}`, tone: 'error', title: reason }
      }
      const format = result.format ? ` · ${result.format.toUpperCase()}` : ''
      const note = result.enrichmentMessage ? ` · ${result.enrichmentMessage}` : ''
      const target = result.outputFileName ?? ''
      return {
        text: `已解密${format} → ${target}${note}`,
        tone: 'success',
        title: [result.outputPath, result.enrichmentMessage].filter(Boolean).join('\n'),
      }
    }
  }
}

const NcmRow = memo(function NcmRow({ file, state, index }: { file: NcmFileEntry; state: RowState; index: number }) {
  const status = describeRow(state)
  return (
    <div
      className="ncm-row ncm-grid"
      role="row"
      aria-rowindex={index + 2}
      style={{ transform: `translateY(${index * NCM_ROW_HEIGHT}px)` }}
    >
      <span className="ncm-cell ncm-name" role="cell" title={file.path}>
        {file.name}
      </span>
      <span className="ncm-cell ncm-size" role="cell">
        {formatFileSize(file.size)}
      </span>
      <span
        className={`ncm-cell ncm-status ncm-status-${status.tone}`}
        role="cell"
        title={status.title || status.text}
        aria-label={`${file.name} 的状态：${status.text}`}
      >
        {status.text}
      </span>
    </div>
  )
})

function FileTable({ files, rows }: { files: NcmFileEntry[]; rows: Map<string, RowState> }) {
  const { scrollerRef, onScroll, range, totalHeight } = useVirtualRows(files.length, NCM_ROW_HEIGHT)
  const visible = files.slice(range.start, range.end)
  return (
    <div className="ncm-table" role="table" aria-label="NCM 文件列表" aria-rowcount={files.length + 1}>
      <div className="ncm-head ncm-grid" role="row" aria-rowindex={1}>
        <span className="ncm-cell" role="columnheader">
          文件
        </span>
        <span className="ncm-cell ncm-size" role="columnheader">
          大小
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
        aria-label="NCM 文件滚动区域"
        onScroll={onScroll}
      >
        {files.length === 0 && <p className="ncm-empty">还没有文件。添加包含 .ncm 的文件夹后会在这里列出。</p>}
        <div className="ncm-spacer" style={{ height: totalHeight }}>
          {visible.map((file, offset) => (
            <NcmRow
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

/** 合并文件夹列表：去掉空白与重复（不区分大小写），保持添加顺序。 */
function mergeFolders(current: string[], added: string[]): string[] {
  const seen = new Set(current.map(pathKey))
  const merged = [...current]
  for (const raw of added) {
    const folder = raw.trim()
    if (folder === '' || seen.has(pathKey(folder))) continue
    seen.add(pathKey(folder))
    merged.push(folder)
  }
  return merged
}

/**
 * NCM 导入对话框：输入文件夹（添加 / 移除，打开时恢复上次的并重新扫描）、输出目录、覆盖与在线匹配选项、
 * 文件列表与逐文件状态、总体进度、开始 / 取消。对应 C# 的 `NcmImportWindow`。
 */
export default function NcmImportDialog({ onClose }: { onClose: () => void }) {
  const [ready, setReady] = useState(false)
  const [folders, setFolders] = useState<string[]>([])
  const [outputDirectory, setOutputDirectory] = useState('')
  const [overwrite, setOverwrite] = useState(false)
  const [autoMatch, setAutoMatch] = useState(readAutoMatch)
  const [onlineAvailable, setOnlineAvailable] = useState(false)
  const [onlineHint, setOnlineHint] = useState('')
  const [files, setFiles] = useState<NcmFileEntry[]>([])
  const [rows, setRows] = useState<Map<string, RowState>>(new Map())
  const [scanning, setScanning] = useState(false)
  const [running, setRunning] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [percent, setPercent] = useState(0)
  const [status, setStatus] = useState(INITIAL_STATUS)
  const [problem, setProblem] = useState(false)
  /** 每次扫描加一：较慢的旧扫描结果不会盖掉新的。 */
  const scanToken = useRef(0)

  const scan = useCallback(async (list: string[]) => {
    const token = ++scanToken.current
    if (list.length === 0) {
      setFiles([])
      setRows(new Map())
      setStatus(INITIAL_STATUS)
      setProblem(false)
      return
    }
    setScanning(true)
    setProblem(false)
    setStatus('正在查找 .ncm 文件…')
    try {
      const result = await scanNcm(list)
      if (token !== scanToken.current) return
      setFiles(result.files)
      setRows(new Map())
      setPercent(0)
      const missing = result.missingFolders.length
      const skipped = result.skippedDirectories
      const notes = [
        missing > 0 ? `${missing} 个文件夹不存在` : '',
        skipped > 0 ? `${skipped} 个子文件夹无法访问已跳过` : '',
      ].filter(Boolean)
      const suffix = notes.length > 0 ? `（${notes.join('，')}）` : ''
      setStatus(
        result.files.length > 0
          ? `已发现 ${result.files.length} 个 .ncm 文件，点击「开始解密」${suffix}`
          : `所选文件夹中没有找到 .ncm 文件${suffix}`,
      )
    } catch (error) {
      if (token !== scanToken.current) return
      setProblem(true)
      setStatus(`扫描 .ncm 文件失败：${ncmErrorText(error, '请重试')}`)
    } finally {
      if (token === scanToken.current) setScanning(false)
    }
  }, [])

  // 打开时恢复记住的文件夹 / 输出目录，并重新扫描。
  useEffect(() => {
    let cancelled = false
    getNcmState()
      .then((state) => {
        if (cancelled) return
        setFolders(state.folders)
        setOutputDirectory(state.outputDirectory)
        setOnlineAvailable(state.onlineMatchAvailable)
        setOnlineHint(state.onlineMatchHint)
        setReady(true)
        void scan(state.folders)
      })
      .catch((error: unknown) => {
        if (cancelled) return
        setReady(true)
        setProblem(true)
        setStatus(`读取 NCM 导入设置失败：${ncmErrorText(error, '请重试')}`)
      })
    return () => {
      cancelled = true
      scanToken.current += 1
    }
  }, [scan])

  const remember = useCallback((list: string[], output: string) => {
    saveNcmState(list, output).catch(() => undefined) // 记忆是便利功能，失败不打断。
  }, [])

  const changeFolders = useCallback(
    (next: string[]) => {
      setFolders(next)
      remember(next, outputDirectory)
      void scan(next)
    },
    [outputDirectory, remember, scan],
  )

  const addFolders = useCallback(() => {
    pickNcmFolders()
      .then((picked) => {
        const merged = mergeFolders(folders, picked)
        if (merged.length !== folders.length) changeFolders(merged)
      })
      .catch((error: unknown) => {
        setProblem(true)
        setStatus(`选择文件夹失败：${ncmErrorText(error, '请重试')}`)
      })
  }, [folders, changeFolders])

  const removeFolder = useCallback(
    (folder: string) => changeFolders(folders.filter((item) => pathKey(item) !== pathKey(folder))),
    [folders, changeFolders],
  )

  const browseOutput = useCallback(() => {
    pickOutputFolder()
      .then((picked) => {
        if (picked) {
          setOutputDirectory(picked)
          remember(folders, picked)
        }
      })
      .catch((error: unknown) => {
        setProblem(true)
        setStatus(`选择文件夹失败：${ncmErrorText(error, '请重试')}`)
      })
  }, [folders, remember])

  const handleProgress = useCallback((progress: NcmProgress) => {
    const key = pathKey(progress.path)
    setRows((previous) => {
      const next = new Map(previous)
      if (progress.stage === 'finished' && progress.result) next.set(key, { kind: 'done', result: progress.result })
      else next.set(key, { kind: progress.stage === 'matching' ? 'matching' : 'running' })
      return next
    })
    if (progress.total > 0) setPercent(Math.min(100, (progress.completed * 100) / progress.total))
    const position = Math.min(progress.completed + 1, Math.max(progress.total, 1))
    if (progress.stage === 'finished') {
      setStatus(`已处理 ${progress.completed}/${progress.total}：${progress.fileName}`)
    } else {
      setStatus(`正在处理 ${position}/${progress.total}：${progress.fileName} · ${progress.message}`)
    }
  }, [])

  const finish = useCallback(
    (result: NcmSummary) => {
      setRows((previous) => {
        const next = new Map(previous)
        for (const item of result.results) next.set(pathKey(item.path), { kind: 'done', result: item })
        // 取消后没处理到的文件标成「已取消」。
        if (result.cancelled) {
          for (const file of files) {
            const key = pathKey(file.path)
            const current = next.get(key)
            if (!current || current.kind === 'pending' || current.kind === 'running' || current.kind === 'matching') {
              next.set(key, { kind: 'cancelled' })
            }
          }
        }
        return next
      })
      setPercent(result.total > 0 ? Math.min(100, (result.results.length * 100) / result.total) : 100)
      const matched = autoMatch && onlineAvailable ? ` · 在线匹配 ${result.matchedOnline} 个` : ''
      const text = result.cancelled
        ? `导入已取消：已处理 ${result.results.length}/${result.total}，成功 ${result.succeeded} 个，失败 ${result.failed} 个`
        : `解密完成：成功 ${result.succeeded} 个，失败 ${result.failed} 个（共 ${result.total} 个）${matched}`
      setStatus(text)
      setProblem(result.failed > 0)
    },
    [files, autoMatch, onlineAvailable],
  )

  const start = useCallback(async () => {
    if (files.length === 0 || running) return
    setRunning(true)
    setCancelling(false)
    setProblem(false)
    setPercent(0)
    setRows(new Map())
    setStatus(`开始解密 ${files.length} 个文件…`)
    let unlisten: (() => void) | undefined
    try {
      unlisten = await onNcmProgress(handleProgress)
      const result = await startNcmImport({
        paths: files.map((file) => file.path),
        folders,
        outputDirectory,
        overwrite,
        autoMatchOnline: autoMatch,
      })
      finish(result)
    } catch (error) {
      setProblem(true)
      setStatus(`导入失败：${ncmErrorText(error, '请重试')}`)
    } finally {
      unlisten?.()
      setRunning(false)
      setCancelling(false)
    }
  }, [files, folders, outputDirectory, overwrite, autoMatch, running, handleProgress, finish])

  const cancel = useCallback(() => {
    setCancelling(true)
    setStatus('正在取消…当前文件处理完后停止')
    cancelNcmImport().catch(() => undefined)
  }, [])

  const close = useCallback(() => {
    if (running) cancelNcmImport().catch(() => undefined)
    remember(folders, outputDirectory)
    onClose()
  }, [running, folders, outputDirectory, remember, onClose])

  const toggleAutoMatch = (value: boolean) => {
    setAutoMatch(value)
    writeAutoMatch(value)
  }

  const busy = running || scanning
  const canStart = ready && !busy && files.length > 0

  return (
    <Modal
      title="导入并解密 NCM"
      className="modal-wide ncm-dialog"
      onClose={close}
      footer={
        <>
          <span className="ncm-count" aria-live="polite">
            已发现 {files.length} 个文件
          </span>
          {running ? (
            <button
              type="button"
              className="btn btn-danger"
              aria-label="取消 NCM 导入"
              disabled={cancelling}
              onClick={cancel}
            >
              {cancelling ? '正在取消…' : '取消'}
            </button>
          ) : (
            <button
              type="button"
              className="btn btn-primary"
              aria-label="开始解密 NCM 文件"
              disabled={!canStart}
              onClick={() => void start()}
            >
              开始解密
            </button>
          )}
          <button type="button" className="btn" aria-label="关闭 NCM 导入" onClick={close}>
            关闭
          </button>
        </>
      }
    >
      <p className="ncm-intro">
        递归查找本地的 .ncm 文件，解密为 MP3/FLAC，写入文件内自带标签与封面；可选自动在线匹配标签、封面与歌词
      </p>

      <section className="ncm-section" aria-label="NCM 文件位置">
        <div className="ncm-section-head">
          <div>
            <h3 className="ncm-label">NCM 文件位置</h3>
            <p className="ncm-sub">可以添加多个文件夹，导入时会去重</p>
          </div>
          <button
            type="button"
            className="btn"
            aria-label="添加包含 NCM 文件的文件夹"
            disabled={running}
            onClick={addFolders}
          >
            添加文件夹…
          </button>
        </div>
        <ul className="ncm-folders" aria-label="已添加的文件夹">
          {folders.length === 0 && <li className="ncm-folders-empty">尚未添加文件夹，点击右上角「添加文件夹…」</li>}
          {folders.map((folder) => (
            <li key={pathKey(folder)} className="ncm-folder">
              <span className="ncm-folder-path" title={folder}>
                {folder}
              </span>
              <button
                type="button"
                className="btn btn-ghost btn-small"
                aria-label={`移除文件夹：${folder}`}
                disabled={running}
                onClick={() => removeFolder(folder)}
              >
                移除
              </button>
            </li>
          ))}
        </ul>
      </section>

      <section className="ncm-section" aria-label="输出设置">
        <h3 className="ncm-label">输出目录</h3>
        <div className="ncm-output">
          <input
            type="text"
            className="ncm-input"
            aria-label="输出目录"
            spellCheck={false}
            value={outputDirectory}
            disabled={running}
            onChange={(event) => setOutputDirectory(event.target.value)}
            onBlur={() => remember(folders, outputDirectory)}
          />
          <button type="button" className="btn" aria-label="浏览输出目录" disabled={running} onClick={browseOutput}>
            浏览…
          </button>
        </div>
        <label className="ncm-check">
          <input
            type="checkbox"
            checked={overwrite}
            disabled={running}
            onChange={(event) => setOverwrite(event.target.checked)}
          />
          <span>覆盖已存在的解密文件</span>
        </label>
        <label className="ncm-check">
          <input
            type="checkbox"
            aria-label="解密后自动获取在线标签、封面与歌词"
            checked={autoMatch}
            disabled={running}
            onChange={(event) => toggleAutoMatch(event.target.checked)}
          />
          <span>解密后自动获取在线标签、封面与歌词</span>
        </label>
        {autoMatch && <p className={onlineAvailable ? 'ncm-sub' : 'ncm-sub ncm-sub-warn'}>{onlineHint}</p>}
      </section>

      <div className="ncm-progress-block" aria-busy={busy}>
        <p className={problem ? 'ncm-status-line ncm-status-line-problem' : 'ncm-status-line'} role="status" aria-live="polite">
          {status}
        </p>
        <progress className="ncm-progress" aria-label="导入进度" value={percent} max={100} />
      </div>

      <FileTable files={files} rows={rows} />
    </Modal>
  )
}
