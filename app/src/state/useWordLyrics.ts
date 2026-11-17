import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { errorMessage } from '../api/editor'
import {
  batchCancel,
  batchScan,
  onWordLyricsProgress,
  wordLyricsApply,
  wordLyricsPreview,
  type BatchFile,
  type WordLyricsProgress,
  type WordLyricsResult,
  type WordLyricsSummary,
} from '../api/online'
import { pathKey, type RowMark } from './useBatchTags'

/** 检查完、等用户确认要不要写入的结果。 */
export interface WordLyricsPending {
  /** 可以转换的文件。 */
  ready: string[]
  /** 各种结论各有多少首，给确认框用的一句话。 */
  text: string
}

/** `scanning` 扫描目录；`checking` 联网检查（不写文件）；`writing` 把检查好的歌词写进文件。 */
export type WordLyricsPhase = 'idle' | 'scanning' | 'checking' | 'writing'

/** 一首歌的结论在列表里怎么显示：能换的与换好的算成功，读写失败算错误，其余只是说明。 */
function markOf(result: WordLyricsResult): RowMark {
  const tone = result.status === 'ready' || result.status === 'converted' ? 'success' : result.status === 'failed' ? 'error' : 'muted'
  return { text: result.message, tone, detail: result.message }
}

/** 各种结论各有多少首，拼成一句话（数量是 0 的不提）。 */
export function wordSummaryText(summary: WordLyricsSummary): string {
  const count = (status: WordLyricsResult['status']) => summary.results.filter((result) => result.status === status).length
  const parts: [number, string][] = [
    [count('ready'), '可以转换'],
    [count('converted'), '已转换'],
    [count('alreadyWord'), '已经是逐字'],
    [count('noLyrics'), '没有歌词'],
    [count('noMatch'), '没找到这首歌'],
    [count('noWordLyrics'), '没有逐字版本'],
    [count('mismatch'), '版本对不上'],
    [count('failed'), '失败'],
  ]
  const text = parts
    .filter(([amount]) => amount > 0)
    .map(([amount, label]) => `${label} ${amount} 首`)
    .join('，')
  return text === '' ? '没有处理任何歌曲' : text
}

const IDLE_STATUS = '选择音乐目录，或把音频文件、文件夹拖进窗口。'

function readyStatus(count: number): string {
  return `共 ${count} 首。可以先选中要处理的歌曲（不选就是全部），再点「检查并转换」。`
}

/**
 * 「歌词转逐字」窗口的全部状态与操作：歌曲列表（扫描目录 / 拖入的文件）、选中的歌，以及两步走的转换——
 * 先联网检查哪些能换（不写文件），用户确认后才把检查好的逐字歌词写进文件。界面只负责渲染。
 */
export function useWordLyrics(initialFolder: string, initialFiles: BatchFile[] | null) {
  const [folder, setFolder] = useState(initialFolder)
  const [recursive, setRecursive] = useState(true)
  // 拖入模式下的清单；为 null 时按目录扫描。
  const [dropped, setDropped] = useState<BatchFile[] | null>(initialFiles)
  const [files, setFiles] = useState<BatchFile[]>(initialFiles ?? [])
  const [marks, setMarks] = useState<Map<string, RowMark>>(new Map())
  const [selected, setSelected] = useState<Set<string>>(new Set())
  // 选择的起点（Shift 连选从这里算）与当前行（键盘移动从这里走）。
  const [cursor, setCursor] = useState({ anchor: -1, focus: -1 })
  const [phase, setPhase] = useState<WordLyricsPhase>('idle')
  const [cancelling, setCancelling] = useState(false)
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null)
  const [status, setStatus] = useState(initialFiles && initialFiles.length > 0 ? readyStatus(initialFiles.length) : IDLE_STATUS)
  const [problem, setProblem] = useState(false)
  const [pending, setPending] = useState<WordLyricsPending | null>(null)

  // 清单每换一次加一：还在路上的扫描 / 任务发现对不上就作废。
  const listToken = useRef(0)
  const stateRef = useRef({ phase, files, selected, cursor, pending })
  useEffect(() => {
    stateRef.current = { phase, files, selected, cursor, pending }
  })

  const report = useCallback((text: string, isProblem = false) => {
    setStatus(text)
    setProblem(isProblem)
  }, [])

  const working = phase === 'checking' || phase === 'writing'
  /** 正在联网 / 写文件，或者确认框还开着：这时不换清单、不改选择。 */
  const isBusy = useCallback(() => {
    const { phase: now, pending: waiting } = stateRef.current
    return now === 'checking' || now === 'writing' || waiting !== null
  }, [])

  // ───────────────────────────── 列表 ─────────────────────────────

  const replaceList = useCallback(
    (next: BatchFile[]) => {
      listToken.current += 1
      setFiles(next)
      setMarks(new Map())
      setSelected(new Set())
      setCursor({ anchor: -1, focus: -1 })
      setPhase('idle')
      setProgress(null)
      report(next.length > 0 ? readyStatus(next.length) : IDLE_STATUS)
    },
    [report],
  )

  /** 扫描目录并列出其中的音频。`quiet`：打开窗口时自动扫描曲库目录，目录不存在就当作还没选，不报错。 */
  const scan = useCallback(
    async (target: string, includeSubfolders: boolean, quiet = false) => {
      if (isBusy()) return
      if (target.trim() === '') {
        report('请选择有效的音乐目录。', true)
        return
      }
      const token = ++listToken.current
      setDropped(null)
      setPhase('scanning')
      setProgress(null)
      report('正在扫描音频文件...')
      try {
        const result = await batchScan(target, includeSubfolders)
        if (token !== listToken.current) return
        replaceList(result.directoryMissing ? [] : result.files)
        if (result.directoryMissing && quiet) setFolder('')
        else if (result.directoryMissing) report('请选择有效的音乐目录。', true)
        else if (result.files.length === 0) report('目录中没有找到支持的音频文件。', true)
      } catch (error) {
        if (token !== listToken.current) return
        setPhase('idle')
        report(`扫描目录失败：${errorMessage(error, '请重试')}`, true)
      }
    },
    [isBusy, replaceList, report],
  )

  /** 换成拖进来的一批文件；正在处理时不换，返回 false。 */
  const loadDropped = useCallback(
    (next: BatchFile[]) => {
      if (isBusy()) {
        report('正在处理，完成或取消后再拖入新的文件。', true)
        return false
      }
      setDropped(next)
      replaceList(next)
      return true
    },
    [isBusy, replaceList, report],
  )

  /** 从拖入的清单切回按目录扫描（清空列表，等用户选目录）。 */
  const leaveDropped = useCallback(() => {
    if (isBusy()) return
    setDropped(null)
    replaceList([])
  }, [isBusy, replaceList])

  // 带着目录打开就先扫描一遍。严格模式下副作用会跑两遍，只做一次。
  const started = useRef(false)
  useEffect(() => {
    if (started.current) return
    started.current = true
    if (!initialFiles && initialFolder.trim() !== '') void scan(initialFolder, true, true)
  }, [initialFiles, initialFolder, scan])

  // ───────────────────────────── 选择 ─────────────────────────────

  const rangeKeys = useCallback((from: number, to: number) => {
    const list = stateRef.current.files
    const keys = new Set<string>()
    for (let index = Math.min(from, to); index <= Math.max(from, to); index += 1) {
      const file = list[index]
      if (file) keys.add(pathKey(file.path))
    }
    return keys
  }, [])

  /** 点一行：单击只选这一首，Ctrl 加选 / 取消，Shift 从起点连选。 */
  const select = useCallback(
    (index: number, shift: boolean, additive: boolean) => {
      if (isBusy()) return
      const { files: list, selected: current, cursor: at } = stateRef.current
      const file = list[index]
      if (!file) return
      const key = pathKey(file.path)
      if (shift && at.anchor >= 0) {
        const keys = rangeKeys(at.anchor, index)
        if (additive) for (const existing of current) keys.add(existing)
        setSelected(keys)
        setCursor({ anchor: at.anchor, focus: index })
      } else if (additive) {
        const keys = new Set(current)
        if (!keys.delete(key)) keys.add(key)
        setSelected(keys)
        setCursor({ anchor: index, focus: index })
      } else {
        setSelected(new Set([key]))
        setCursor({ anchor: index, focus: index })
      }
    },
    [isBusy, rangeKeys],
  )

  /** 键盘移动到某一行；`extend`（按着 Shift）时从起点连选到这一行。 */
  const move = useCallback(
    (index: number, extend: boolean) => {
      if (isBusy()) return
      const at = stateRef.current.cursor
      const anchor = extend && at.anchor >= 0 ? at.anchor : index
      setSelected(rangeKeys(anchor, index))
      setCursor({ anchor, focus: index })
    },
    [isBusy, rangeKeys],
  )

  const selectAll = useCallback(() => {
    if (isBusy()) return
    const { files: list, cursor: at } = stateRef.current
    if (list.length === 0) return
    setSelected(rangeKeys(0, list.length - 1))
    setCursor({ anchor: 0, focus: at.focus >= 0 ? at.focus : 0 })
  }, [isBusy, rangeKeys])

  const clearSelection = useCallback(() => {
    if (isBusy()) return
    setSelected(new Set())
  }, [isBusy])

  /** 要处理的歌：选中的那些；一首都没选就是整个清单。 */
  const targets = useMemo(
    () => (selected.size > 0 ? files.filter((file) => selected.has(pathKey(file.path))) : files),
    [files, selected],
  )

  // ───────────────────────────── 检查与转换 ─────────────────────────────

  /**
   * 跑一步：`preview` 联网核对（不写文件），`apply` 把核对好的结果写进文件。
   * 逐个文件的结论显示在列表的状态列里；返回汇总，出错或清单已经换掉时返回 `null`。
   */
  const runStep = useCallback(
    async (step: 'preview' | 'apply', paths: string[]): Promise<WordLyricsSummary | null> => {
      const token = listToken.current
      const doing = step === 'preview' ? '正在检查歌词能否换成逐字' : '正在把歌词换成逐字'
      setCancelling(false)
      setPhase(step === 'preview' ? 'checking' : 'writing')
      setProgress({ done: 0, total: paths.length })
      report(`${doing}… 0/${paths.length}`)
      setMarks((previous) => {
        const next = new Map(previous)
        for (const path of paths) next.set(pathKey(path), { text: '等待处理', tone: 'muted' })
        return next
      })
      const onProgress = (event: WordLyricsProgress) => {
        if (token !== listToken.current) return
        const result = event.stage === 'finished' ? event.result : null
        setMarks((previous) => {
          const next = new Map(previous)
          next.set(
            pathKey(event.path),
            result ? markOf(result) : { text: step === 'preview' ? '正在核对…' : '正在写入…', tone: 'busy' },
          )
          return next
        })
        if (!result) return
        setProgress({ done: event.completed, total: event.total })
        report(`${doing}… ${event.completed}/${event.total}`)
      }
      let unlisten: (() => void) | undefined
      try {
        unlisten = await onWordLyricsProgress(onProgress)
        const summary = await (step === 'preview' ? wordLyricsPreview(paths) : wordLyricsApply(paths))
        if (token !== listToken.current) return null
        // 事件与命令返回值不保证先后：以汇总里的结果为准，取消后没处理到的文件标成「已取消」。
        const finished = new Set(summary.results.map((result) => pathKey(result.filePath)))
        setMarks((previous) => {
          const next = new Map(previous)
          for (const result of summary.results) next.set(pathKey(result.filePath), markOf(result))
          for (const path of paths) {
            if (!finished.has(pathKey(path))) next.set(pathKey(path), { text: '已取消', tone: 'muted' })
          }
          return next
        })
        return summary
      } catch (error) {
        if (token === listToken.current) {
          report(`${step === 'preview' ? '检查歌词' : '歌词转逐字'}失败：${errorMessage(error, '请重试')}`, true)
        }
        return null
      } finally {
        unlisten?.()
        if (token === listToken.current) {
          setPhase('idle')
          setProgress(null)
          setCancelling(false)
        }
      }
    },
    [report],
  )

  /**
   * 第一步：检查选中的歌（一首都没选就是整个清单）里哪些能换。只联网、不写文件；
   * 有能换的就把结果交给界面去问用户（`pending`）。
   */
  const preview = useCallback(async () => {
    const { phase: now, files: list, selected: chosen, pending: waiting } = stateRef.current
    if (now !== 'idle' || waiting) return
    const paths = (chosen.size > 0 ? list.filter((file) => chosen.has(pathKey(file.path))) : list).map((file) => file.path)
    if (paths.length === 0) return
    const summary = await runStep('preview', paths)
    if (!summary) return
    const ready = summary.results.filter((result) => result.status === 'ready').map((result) => result.filePath)
    const text = wordSummaryText(summary)
    const lead = summary.cancelled ? `已取消检查（处理了 ${summary.processed}/${summary.total} 首）` : '检查完成'
    if (ready.length > 0) {
      setPending({ ready, text })
      report(`${lead}：${text}。`)
    } else {
      report(`${lead}，没有可以换成逐字歌词的歌曲：${text}。`, summary.failed > 0)
    }
  }, [report, runStep])

  /** 第二步：用户确认后把检查好的结果写进文件。 */
  const apply = useCallback(async () => {
    const { phase: now, pending: waiting } = stateRef.current
    if (!waiting || now !== 'idle') return
    setPending(null)
    const summary = await runStep('apply', waiting.ready)
    if (!summary) return
    const lead = summary.cancelled ? `已取消（处理了 ${summary.processed}/${summary.total} 首）` : '歌词转逐字完成'
    report(`${lead}：${wordSummaryText(summary)}。`, summary.failed > 0)
  }, [report, runStep])

  /** 看完检查结果决定先不换。 */
  const dismiss = useCallback(() => {
    setPending(null)
    report('没有转换，文件都没有改动。各首歌的检查结果留在列表里。')
  }, [report])

  /** 取消正在进行的检查 / 写入：当前文件处理完后停下。 */
  const cancel = useCallback(() => {
    const now = stateRef.current.phase
    if (now !== 'checking' && now !== 'writing') return
    setCancelling(true)
    report('正在取消…当前文件处理完后停止')
    batchCancel().catch(() => undefined)
  }, [report])

  /** 窗口关闭时：停掉还在跑的任务，作废还在路上的扫描。 */
  const dispose = useCallback(() => {
    const now = stateRef.current.phase
    if (now === 'checking' || now === 'writing') batchCancel().catch(() => undefined)
    listToken.current += 1
  }, [])

  return {
    folder,
    setFolder,
    recursive,
    setRecursive,
    dropped,
    files,
    marks,
    selected,
    cursor,
    select,
    move,
    selectAll,
    clearSelection,
    targets,
    phase,
    working,
    cancelling,
    progress,
    status,
    problem,
    report,
    scan,
    loadDropped,
    leaveDropped,
    pending,
    preview,
    apply,
    dismiss,
    cancel,
    dispose,
  }
}

export type WordLyrics = ReturnType<typeof useWordLyrics>
