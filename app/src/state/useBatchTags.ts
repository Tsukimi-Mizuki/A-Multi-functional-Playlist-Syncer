import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  errorMessage,
  getEditorState,
  pickCoverImage,
  readCoverFile,
  type EditorState,
} from '../api/editor'
import {
  batchCancel,
  batchRun,
  batchScan,
  onBatchProgress,
  type BatchFile,
  type BatchProgress,
  type BatchResult,
} from '../api/online'
import {
  patchTags,
  readTagRows,
  readTags,
  toTagLoadError,
  type TagPatch,
  type TagRow,
  type TagView,
} from '../api/tags'
import { coverMeta, parseOptionalUInt, splitArtists, type CoverView } from './useLyricsEditor'

/** 左边面板里能改的字段（数字字段也用字符串，保存时再校验）。 */
export type TagFieldName =
  | 'title'
  | 'artist'
  | 'album'
  | 'year'
  | 'trackNumber'
  | 'discNumber'
  | 'genre'
  | 'albumArtist'
  | 'composer'
  | 'lyricist'
  | 'comment'

export const TAG_FIELD_NAMES: readonly TagFieldName[] = [
  'title',
  'artist',
  'album',
  'year',
  'trackNumber',
  'discNumber',
  'genre',
  'albumArtist',
  'composer',
  'lyricist',
  'comment',
]

/** 一个字段在面板里的样子：`mixed` = 选中的歌这一项不一样（不改就保持各自的值）；`edited` = 用户动过。 */
export interface FieldView {
  value: string
  mixed: boolean
  edited: boolean
}

export type TagEdits = Partial<Record<TagFieldName, string>>

/** 封面的改动：`keep` 不动；`new` 换成这张；`removed` 保存时删除。 */
export type CoverEdit =
  | { kind: 'keep' }
  | { kind: 'new'; base64: string; mimeType: string; size: number; width: number | null; height: number | null; origin: string }
  | { kind: 'removed' }

/** 列表「状态」一栏：保存 / 在线获取的结果。 */
export interface RowMark {
  text: string
  tone: 'muted' | 'busy' | 'success' | 'error'
  /** 鼠标悬停时的完整说明。 */
  detail?: string
}

/** `scanning` 扫描目录；`loading` 正在读列表里的标签（可以先选、先看）；`saving` / `online` 正在写文件。 */
export type BatchPhase = 'idle' | 'scanning' | 'loading' | 'saving' | 'online'

/** 每次读多少个文件的标签：分批读，列表边读边填，窗口关掉也不会留下一个很长的后台任务。 */
const READ_CHUNK = 100
/** 每次保存多少个文件：分批写才能显示进度、中途取消。 */
const SAVE_CHUNK = 20

/** 路径比较用的键：Windows 路径不区分大小写，也不区分 `/` 与 `\`。 */
export function pathKey(path: string): string {
  return path.replace(/\//g, '\\').toLowerCase()
}

/** 一行里某个字段的文本（没有值为空串）。 */
export function rowValue(row: TagRow, name: TagFieldName): string {
  const value = row[name]
  return value == null ? '' : String(value)
}

/** 这些行每个字段的共同值；不一样的字段标成 `mixed`。 */
export function commonValues(rows: readonly TagRow[]): Record<TagFieldName, { value: string; mixed: boolean }> {
  const result = {} as Record<TagFieldName, { value: string; mixed: boolean }>
  for (const name of TAG_FIELD_NAMES) {
    const first = rows[0] ? rowValue(rows[0], name) : ''
    const mixed = rows.some((row) => rowValue(row, name) !== first)
    result[name] = mixed ? { value: '', mixed: true } : { value: first, mixed: false }
  }
  return result
}

const NUMBER_LABELS = { year: '年份', trackNumber: '音轨号', discNumber: '碟号' } as const

/** 把面板里的改动变成后端的补丁：只带动过的字段，空白表示清除；数字填得不对时返回原因。 */
export function buildPatch(edits: TagEdits, cover: CoverEdit): { patch: TagPatch } | { error: string } {
  const patch: TagPatch = {}
  for (const name of ['title', 'album', 'albumArtist', 'genre', 'composer', 'lyricist', 'comment'] as const) {
    const text = edits[name]
    if (text !== undefined) patch[name] = text.trim() === '' ? null : text.trim()
  }
  if (edits.artist !== undefined) patch.artists = splitArtists(edits.artist)
  for (const name of ['year', 'trackNumber', 'discNumber'] as const) {
    const text = edits[name]
    if (text === undefined) continue
    const value = parseOptionalUInt(text)
    if (value === undefined) return { error: `${NUMBER_LABELS[name]}必须是数字。` }
    patch[name] = value
  }
  if (cover.kind === 'new') {
    patch.coverBase64 = cover.base64
    patch.coverMimeType = cover.mimeType
  } else if (cover.kind === 'removed') {
    patch.removeCover = true
  }
  return { patch }
}

function markOf(result: BatchResult): RowMark {
  const matched = result.matchedTitle
    ? result.matchedArtist
      ? `${result.matchedTitle} - ${result.matchedArtist}`
      : result.matchedTitle
    : ''
  const from = [matched, result.sourceLabel ?? ''].filter((part) => part !== '').join(' · ')
  return {
    text: result.message,
    tone: result.success ? 'success' : result.skipped ? 'muted' : 'error',
    detail: from === '' ? result.message : `${result.message}（${from}）`,
  }
}

const IDLE_STATUS = '选择音乐目录，或把音频文件、文件夹拖进窗口。'

function readyStatus(count: number): string {
  return `共 ${count} 首。选中歌曲后在左边修改标签，也可以在线获取标签、封面与歌词。`
}

/**
 * 批量修改标签窗口的全部状态与操作：右边的歌曲列表（扫描目录 / 拖入的文件，标签分批读出）、选中的歌、
 * 左边面板里的改动（只把动过的字段写进所有选中的文件）、封面改动，以及对选中的歌在线获取标签、封面与歌词。
 * 界面只负责渲染与「有未保存的修改」确认。
 */
export function useBatchTags(initialFolder: string, initialFiles: BatchFile[] | null) {
  const [config, setConfig] = useState<EditorState | null>(null)
  const [folder, setFolder] = useState(initialFolder)
  const [recursive, setRecursive] = useState(true)
  // 拖入模式下的清单；为 null 时按目录扫描。
  const [dropped, setDropped] = useState<BatchFile[] | null>(initialFiles)
  const [files, setFiles] = useState<BatchFile[]>(initialFiles ?? [])
  const [rows, setRows] = useState<Map<string, TagRow>>(new Map())
  const [marks, setMarks] = useState<Map<string, RowMark>>(new Map())
  const [selected, setSelected] = useState<Set<string>>(new Set())
  // 选择的起点（Shift 连选从这里算）与当前行（键盘移动从这里走）。
  const [cursor, setCursor] = useState({ anchor: -1, focus: -1 })
  const [edits, setEdits] = useState<TagEdits>({})
  const [cover, setCover] = useState<CoverEdit>({ kind: 'keep' })
  // 只选中一首时读出它的完整标签（封面预览与封面参数）。
  const [single, setSingle] = useState<TagView | null>(null)
  const [singleVersion, setSingleVersion] = useState(0)
  const [phase, setPhase] = useState<BatchPhase>('idle')
  const [cancelling, setCancelling] = useState(false)
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null)
  const [status, setStatus] = useState(IDLE_STATUS)
  const [problem, setProblem] = useState(false)
  const [overwrite, setOverwrite] = useState(true)
  const [fetchLyrics, setFetchLyrics] = useState(true)
  const [preferWord, setPreferWord] = useState(false)

  // 清单每换一次加一：还在路上的读取 / 刷新发现对不上就作废。
  const listToken = useRef(0)
  const cancelRef = useRef(false)
  const phaseRef = useRef(phase)
  const filesRef = useRef(files)
  const selectedRef = useRef(selected)
  const editsRef = useRef(edits)
  const coverRef = useRef(cover)

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

  const report = useCallback((text: string, isProblem = false) => {
    setStatus(text)
    setProblem(isProblem)
  }, [])

  // ───────────────────────────── 选中的歌与面板里的值 ─────────────────────────────

  /** 选中的歌里读出了标签、可以写入的那些（按列表顺序）。 */
  const targets = useMemo(() => {
    if (selected.size === 0) return []
    const result: TagRow[] = []
    for (const file of files) {
      const key = pathKey(file.path)
      if (!selected.has(key)) continue
      const row = rows.get(key)
      if (row?.readable) result.push(row)
    }
    return result
  }, [files, rows, selected])

  const common = useMemo(() => commonValues(targets), [targets])
  const fields = useMemo(() => {
    const result = {} as Record<TagFieldName, FieldView>
    for (const name of TAG_FIELD_NAMES) {
      const edit = edits[name]
      result[name] =
        edit === undefined ? { ...common[name], edited: false } : { value: edit, mixed: common[name].mixed, edited: true }
    }
    return result
  }, [common, edits])

  const dirty = Object.keys(edits).length > 0 || cover.kind !== 'keep'
  const singlePath = selected.size === 1 && targets.length === 1 ? (targets[0]?.path ?? null) : null

  const targetsRef = useRef(targets)
  const commonRef = useRef(common)
  const dirtyRef = useRef(dirty)
  useEffect(() => {
    phaseRef.current = phase
    filesRef.current = files
    selectedRef.current = selected
    editsRef.current = edits
    coverRef.current = cover
    targetsRef.current = targets
    commonRef.current = common
    dirtyRef.current = dirty
  })

  useEffect(() => {
    if (!singlePath) return
    let cancelled = false
    readTags(singlePath)
      .then((info) => {
        if (!cancelled) setSingle(info)
      })
      .catch(() => {
        if (!cancelled) setSingle(null)
      })
    return () => {
      cancelled = true
    }
  }, [singlePath, singleVersion])

  const discardEdits = useCallback(() => {
    setEdits({})
    setCover({ kind: 'keep' })
  }, [])

  /** 换成新的选择；面板里没保存的改动随之丢弃（调用方先确认）。 */
  const applySelection = useCallback(
    (keys: Set<string>, anchor: number, focus: number) => {
      setSelected(keys)
      setCursor({ anchor, focus })
      discardEdits()
    },
    [discardEdits],
  )

  const setField = useCallback((name: TagFieldName, value: string) => {
    setEdits((previous) => {
      const next = { ...previous }
      const base = commonRef.current[name]
      // 改回原来的值就不算改动；值不一样的字段只要动过就算（清空表示把这些歌的这一项都清掉）。
      if (!base.mixed && value === base.value) delete next[name]
      else next[name] = value
      return next
    })
  }, [])

  const resetField = useCallback((name: TagFieldName) => {
    setEdits((previous) => {
      if (!(name in previous)) return previous
      const next = { ...previous }
      delete next[name]
      return next
    })
  }, [])

  // ───────────────────────────── 列表 ─────────────────────────────

  const mergeRows = useCallback((loaded: TagRow[]) => {
    setRows((previous) => {
      const next = new Map(previous)
      for (const row of loaded) next.set(pathKey(row.path), row)
      return next
    })
  }, [])

  /** 重新读这些文件的标签并更新列表（读不到就保持原样）。 */
  const refreshRows = useCallback(
    async (paths: string[]) => {
      const token = listToken.current
      for (let start = 0; start < paths.length; start += READ_CHUNK) {
        try {
          const loaded = await readTagRows(paths.slice(start, start + READ_CHUNK))
          if (token !== listToken.current) return
          mergeRows(loaded)
        } catch {
          return
        }
      }
    },
    [mergeRows],
  )

  /** 换成新的清单并分批读出标签。 */
  const replaceList = useCallback(
    async (next: BatchFile[]) => {
      const token = ++listToken.current
      setFiles(next)
      setRows(new Map())
      setMarks(new Map())
      setSelected(new Set())
      setCursor({ anchor: -1, focus: -1 })
      setSingle(null)
      discardEdits()
      if (next.length === 0) {
        setPhase('idle')
        setProgress(null)
        return
      }
      setPhase('loading')
      setProgress({ done: 0, total: next.length })
      report(`正在读取标签… 0/${next.length}`)
      for (let start = 0; start < next.length; start += READ_CHUNK) {
        const chunk = next.slice(start, start + READ_CHUNK)
        let loaded: TagRow[]
        try {
          loaded = await readTagRows(chunk.map((file) => file.path))
        } catch (error) {
          if (token !== listToken.current) return
          setPhase('idle')
          setProgress(null)
          report(`读取标签失败：${toTagLoadError(error).message}`, true)
          return
        }
        if (token !== listToken.current) return
        mergeRows(loaded)
        const done = Math.min(next.length, start + chunk.length)
        setProgress({ done, total: next.length })
        report(`正在读取标签… ${done}/${next.length}`)
      }
      setPhase('idle')
      setProgress(null)
      report(readyStatus(next.length))
    },
    [discardEdits, mergeRows, report],
  )

  /** 扫描目录并列出其中的音频。 */
  const scan = useCallback(
    async (target: string, includeSubfolders: boolean) => {
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
        if (result.directoryMissing) {
          await replaceList([])
          report('请选择有效的音乐目录。', true)
          return
        }
        await replaceList(result.files)
        if (result.files.length === 0) report('目录中没有找到支持的音频文件。', true)
      } catch (error) {
        if (token !== listToken.current) return
        setPhase('idle')
        report(`扫描目录失败：${errorMessage(error, '请重试')}`, true)
      }
    },
    [replaceList, report],
  )

  /** 换成拖进来的一批文件。 */
  const loadDropped = useCallback(
    (next: BatchFile[]) => {
      setDropped(next)
      void replaceList(next)
    },
    [replaceList],
  )

  /** 从拖入的清单切回按目录扫描（清空列表，等用户选目录）。 */
  const leaveDropped = useCallback(() => {
    setDropped(null)
    void replaceList([]).then(() => report(IDLE_STATUS))
  }, [replaceList, report])

  // 打开时：拖入的文件直接读标签；带着目录打开（从编辑器进来）就先扫描一遍。严格模式下副作用会跑两遍，只做一次。
  const started = useRef(false)
  useEffect(() => {
    if (started.current) return
    started.current = true
    if (initialFiles) void replaceList(initialFiles)
    else if (initialFolder.trim() !== '') void scan(initialFolder, true)
  }, [initialFiles, initialFolder, replaceList, scan])

  // ───────────────────────────── 封面 ─────────────────────────────

  const chooseCover = useCallback(async () => {
    try {
      const path = await pickCoverImage()
      if (!path) return
      const image = await readCoverFile(path)
      setCover({
        kind: 'new',
        base64: image.base64,
        mimeType: image.mimeType,
        size: image.size,
        width: image.width,
        height: image.height,
        origin: image.fileName,
      })
      report(`已选择封面「${image.fileName}」，保存后写入选中的歌曲。`)
    } catch (error) {
      report(`更换封面失败：${errorMessage(error, '请重试')}`, true)
    }
  }, [report])

  const removeCover = useCallback(() => {
    setCover({ kind: 'removed' })
    report('已标记移除封面，保存后生效。')
  }, [report])

  const restoreCover = useCallback(() => setCover({ kind: 'keep' }), [])

  /** 面板里要显示的封面：新选的图片、单首歌文件里的封面，或多首时的统计。 */
  const coverView = useMemo((): CoverView => {
    if (cover.kind === 'new') {
      return {
        src: `data:${cover.mimeType};base64,${cover.base64}`,
        meta: coverMeta(cover.mimeType, cover.width, cover.height, cover.size, '正面封面'),
        caption: `新封面：${cover.origin}（保存后写入）`,
        tooLarge: false,
      }
    }
    if (cover.kind === 'removed') return { src: null, meta: null, caption: '封面将在保存时移除', tooLarge: false }
    if (targets.length === 0) return { src: null, meta: null, caption: '', tooLarge: false }
    if (targets.length > 1) {
      const withCover = targets.filter((row) => row.hasCover).length
      return { src: null, meta: null, caption: `${targets.length} 首中 ${withCover} 首有封面`, tooLarge: false }
    }
    const row = targets[0]
    if (!row?.hasCover) return { src: null, meta: null, caption: '没有封面', tooLarge: false }
    const info = single && pathKey(single.path) === pathKey(row.path) ? single : null
    if (!info) return { src: null, meta: null, caption: '正在读取封面…', tooLarge: false }
    const meta = coverMeta(info.coverMimeType, info.coverWidth, info.coverHeight, info.coverSize, info.coverPictureType)
    if (info.coverDataUrl) return { src: info.coverDataUrl, meta, caption: '', tooLarge: false }
    return { src: null, meta, caption: '封面过大，未预览', tooLarge: true }
  }, [cover, targets, single])

  // ───────────────────────────── 保存 ─────────────────────────────

  /** 把面板里动过的字段写进所有选中的文件。 */
  const save = useCallback(async () => {
    if (phaseRef.current !== 'idle' || !dirtyRef.current) return
    const built = buildPatch(editsRef.current, coverRef.current)
    if ('error' in built) {
      report(built.error, true)
      return
    }
    const paths = targetsRef.current.map((row) => row.path)
    if (paths.length === 0) return
    cancelRef.current = false
    setCancelling(false)
    setPhase('saving')
    setProgress({ done: 0, total: paths.length })
    report(`正在保存标签… 0/${paths.length}`)
    let saved = 0
    let failed = 0
    let firstFailure: string | null = null
    let done = 0
    try {
      for (let start = 0; start < paths.length && !cancelRef.current; start += SAVE_CHUNK) {
        const results = await patchTags(paths.slice(start, start + SAVE_CHUNK), built.patch)
        mergeRows(results.map((result) => result.row))
        setMarks((previous) => {
          const next = new Map(previous)
          for (const result of results) {
            const failure = result.message ?? '保存失败，请重试。'
            next.set(
              pathKey(result.path),
              result.success ? { text: '已保存', tone: 'success' } : { text: failure, tone: 'error' },
            )
          }
          return next
        })
        for (const result of results) {
          if (result.success) saved += 1
          else {
            failed += 1
            firstFailure ??= result.message
          }
        }
        done = Math.min(paths.length, start + SAVE_CHUNK)
        setProgress({ done, total: paths.length })
        report(`正在保存标签… ${done}/${paths.length}`)
      }
    } catch (error) {
      setPhase('idle')
      setProgress(null)
      setSingleVersion((value) => value + 1)
      report(`保存失败：${toTagLoadError(error).message}`, true)
      return
    }
    const stopped = done < paths.length
    // 全部写完才清掉面板里的改动；有失败或中途取消时留着，方便修正后再保存一次。
    if (failed === 0 && !stopped) discardEdits()
    setSingleVersion((value) => value + 1)
    setPhase('idle')
    setProgress(null)
    setCancelling(false)
    if (stopped) report(`已取消保存：已保存 ${saved} 首，还有 ${paths.length - done} 首没有处理。`, true)
    else if (failed > 0) report(`已保存 ${saved} 首，失败 ${failed} 首：${firstFailure ?? '请重试'}`, true)
    else report(`已保存 ${saved} 首。`)
  }, [discardEdits, mergeRows, report])

  // ───────────────────────────── 在线获取 ─────────────────────────────

  /** 在线获取的对象：选中的歌；一首都没选就是整个清单。 */
  const onlineTargets = useMemo(
    () => (selected.size > 0 ? files.filter((file) => selected.has(pathKey(file.path))) : files),
    [files, selected],
  )

  const runOnline = useCallback(async () => {
    if (phaseRef.current !== 'idle' || dirtyRef.current) return
    const chosen = selectedRef.current
    const list = filesRef.current
    const paths = (chosen.size > 0 ? list.filter((file) => chosen.has(pathKey(file.path))) : list).map((file) => file.path)
    if (paths.length === 0) return
    const token = listToken.current
    setCancelling(false)
    setPhase('online')
    setProgress({ done: 0, total: paths.length })
    report(`正在在线获取标签、封面与歌词… 0/${paths.length}`)
    setMarks((previous) => {
      const next = new Map(previous)
      for (const path of paths) next.set(pathKey(path), { text: '等待处理', tone: 'muted' })
      return next
    })
    const refreshed = new Set<string>()
    const onProgress = (event: BatchProgress) => {
      if (token !== listToken.current) return
      const key = pathKey(event.path)
      const result = event.stage === 'finished' ? event.result : null
      setMarks((previous) => {
        const next = new Map(previous)
        next.set(key, result ? markOf(result) : { text: '正在匹配…', tone: 'busy' })
        return next
      })
      if (!result) return
      setProgress({ done: event.completed, total: event.total })
      report(`正在在线获取标签、封面与歌词… ${event.completed}/${event.total}`)
      if (result.success) {
        refreshed.add(key)
        void refreshRows([event.path])
      }
    }
    let unlisten: (() => void) | undefined
    try {
      unlisten = await onBatchProgress(onProgress)
      const summary = await batchRun({
        files: paths,
        overwriteExisting: overwrite,
        fetchLyrics,
        preferWordLyrics: preferWord,
      })
      if (token !== listToken.current) return
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
      await refreshRows(
        summary.results
          .filter((result) => result.success && !refreshed.has(pathKey(result.filePath)))
          .map((result) => result.filePath),
      )
      if (summary.cancelled) report(`已取消在线获取：已处理 ${summary.processed}/${summary.total}。`)
      else {
        report(
          `在线获取完成：成功 ${summary.success}，跳过 ${summary.skipped}，失败 ${summary.failed}。`,
          summary.failed > 0,
        )
      }
    } catch (error) {
      if (token === listToken.current) report(`在线获取失败：${errorMessage(error, '请重试')}`, true)
    } finally {
      unlisten?.()
      if (token === listToken.current) {
        setPhase('idle')
        setProgress(null)
        setCancelling(false)
        setSingleVersion((value) => value + 1)
      }
    }
  }, [overwrite, fetchLyrics, preferWord, refreshRows, report])

  /** 取消正在进行的保存 / 在线获取：当前这一批处理完后停下。 */
  const cancel = useCallback(() => {
    if (phaseRef.current === 'online') {
      setCancelling(true)
      report('正在取消…当前文件处理完后停止')
      batchCancel().catch(() => undefined)
    } else if (phaseRef.current === 'saving') {
      cancelRef.current = true
      setCancelling(true)
      report('正在取消…当前这一批保存完后停止')
    }
  }, [report])

  /** 窗口关闭时：停掉还在跑的任务，作废还在路上的读取。 */
  const dispose = useCallback(() => {
    if (phaseRef.current === 'online') batchCancel().catch(() => undefined)
    cancelRef.current = true
    listToken.current += 1
  }, [])

  /** 某个文件在别处（歌词与标签编辑器）改过了：重新读它这一行与面板。 */
  const refreshFile = useCallback(
    (path: string) => {
      setMarks((previous) => {
        if (!previous.has(pathKey(path))) return previous
        const next = new Map(previous)
        next.delete(pathKey(path))
        return next
      })
      void refreshRows([path])
      setSingleVersion((value) => value + 1)
    },
    [refreshRows],
  )

  return {
    config,
    folder,
    setFolder,
    recursive,
    setRecursive,
    dropped,
    files,
    rows,
    marks,
    selected,
    cursor,
    setCursor,
    applySelection,
    targets,
    singlePath,
    fields,
    setField,
    resetField,
    dirty,
    dirtyRef,
    discardEdits,
    cover,
    coverView,
    chooseCover,
    removeCover,
    restoreCover,
    phase,
    phaseRef,
    cancelling,
    progress,
    status,
    problem,
    report,
    scan,
    loadDropped,
    leaveDropped,
    save,
    onlineTargets,
    runOnline,
    cancel,
    dispose,
    refreshFile,
    overwrite,
    setOverwrite,
    fetchLyrics,
    setFetchLyrics,
    preferWord,
    setPreferWord,
  }
}

export type BatchTags = ReturnType<typeof useBatchTags>
