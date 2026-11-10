import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from 'react'
import { errorMessage, pickMusicFolder } from '../api/editor'
import type { BatchFile } from '../api/online'
import type { TagRow } from '../api/tags'
import { useVirtualRows } from '../lib/virtual'
import {
  pathKey,
  rowValue,
  useBatchTags,
  type BatchTags,
  type FieldView,
  type RowMark,
  type TagFieldName,
} from '../state/useBatchTags'
import type { CoverMeta } from '../state/useLyricsEditor'
import ConfirmDialog from './ConfirmDialog'
import Modal from './Modal'

/** 拖进来交给批量修改的文件；`seq` 每次加一，同一批文件再拖一次也会重新载入。 */
export interface BatchDrop {
  seq: number
  files: BatchFile[]
}

/** 固定行高（px）：虚拟滚动依赖它，样式里的 `--batch-row-height` 必须与此一致。 */
export const BATCH_ROW_HEIGHT = 34
/** 表头高度（px）：表头粘在滚动区顶部，键盘移动时要把它让出来；与样式里的 `--batch-head-height` 一致。 */
const BATCH_HEAD_HEIGHT = 30

type ColumnKey = 'fileName' | 'status' | 'directory' | 'format' | TagFieldName

/** 列表的列：次序与参考的标签编辑器一致，多出来的「状态」放在文件名后面，不用横向滚动就能看到处理结果。 */
const COLUMNS: readonly { key: ColumnKey; label: string; width: number }[] = [
  { key: 'fileName', label: '文件名', width: 230 },
  { key: 'status', label: '状态', width: 180 },
  { key: 'directory', label: '目录', width: 170 },
  { key: 'format', label: '格式', width: 58 },
  { key: 'title', label: '标题', width: 180 },
  { key: 'artist', label: '艺术家', width: 150 },
  { key: 'album', label: '专辑', width: 170 },
  { key: 'albumArtist', label: '专辑艺术家', width: 130 },
  { key: 'year', label: '年份', width: 56 },
  { key: 'trackNumber', label: '音轨号', width: 56 },
  { key: 'discNumber', label: '碟号', width: 50 },
  { key: 'genre', label: '风格', width: 100 },
  { key: 'composer', label: '作曲家', width: 130 },
  { key: 'lyricist', label: '作词家', width: 130 },
]
const COLUMN_GAP = 12
const ROW_PADDING = 12
const GRID_TEMPLATE = COLUMNS.map((column) => `${column.width}px`).join(' ')
const GRID_WIDTH =
  COLUMNS.reduce((sum, column) => sum + column.width, 0) + COLUMN_GAP * (COLUMNS.length - 1) + ROW_PADDING * 2
/** 列宽写成 CSS 变量，表头与每一行共用同一套网格。 */
const TABLE_STYLE = { '--batch-columns': GRID_TEMPLATE, '--batch-width': `${GRID_WIDTH}px` } as CSSProperties

/**
 * 左边面板的字段，一组一行，次序同参考图。年份与音轨号、碟号并排（参考图里年份单独一行）：
 * 省下一行，默认窗口大小（1280×800）下整个面板连封面都不用滚动。
 */
const PANE_ROWS: readonly (readonly { name: TagFieldName; label: string }[])[] = [
  [{ name: 'title', label: '标题' }],
  [{ name: 'artist', label: '艺术家' }],
  [{ name: 'album', label: '专辑' }],
  [
    { name: 'year', label: '年份' },
    { name: 'trackNumber', label: '音轨号' },
    { name: 'discNumber', label: '碟号' },
  ],
  [{ name: 'genre', label: '风格' }],
  [{ name: 'albumArtist', label: '专辑艺术家' }],
  [{ name: 'composer', label: '作曲家' }],
  [{ name: 'lyricist', label: '作词家' }],
  [{ name: 'comment', label: '注释' }],
]

const KEEP_PLACEHOLDER = '〈保持〉'
const CLEAR_PLACEHOLDER = '〈清空〉'

function cellText(key: ColumnKey, file: BatchFile, row: TagRow | undefined): string {
  if (key === 'fileName') return file.name
  if (!row) return ''
  if (key === 'directory') return row.directory
  if (key === 'format') return row.fileFormat ?? ''
  if (key === 'status') return ''
  return rowValue(row, key)
}

/** 「状态」一栏：保存 / 在线获取的结果优先；否则是读取失败的原因。 */
function statusOf(row: TagRow | undefined, mark: RowMark | undefined): RowMark | null {
  if (mark) return mark
  if (!row) return { text: '正在读取…', tone: 'muted' }
  if (!row.readable) return { text: row.error ?? '无法读取标签', tone: 'error' }
  return null
}

interface SongRowProps {
  file: BatchFile
  row: TagRow | undefined
  mark: RowMark | undefined
  index: number
  selected: boolean
  focused: boolean
  onSelect: (index: number, event: MouseEvent) => void
  onOpen: (index: number) => void
}

const SongRow = memo(function SongRow({ file, row, mark, index, selected, focused, onSelect, onOpen }: SongRowProps) {
  const status = statusOf(row, mark)
  const className = ['batch-row', selected ? 'batch-row-selected' : '', focused ? 'batch-row-focused' : '']
    .filter((name) => name !== '')
    .join(' ')
  return (
    <div
      className={className}
      role="row"
      aria-rowindex={index + 2}
      aria-selected={selected}
      aria-label={file.name}
      style={{ transform: `translateY(${index * BATCH_ROW_HEIGHT}px)` }}
      onMouseDown={(event) => {
        // Shift 连选时不要顺带选中一大片文字。
        if (event.shiftKey) event.preventDefault()
      }}
      onClick={(event) => onSelect(index, event)}
      onDoubleClick={() => onOpen(index)}
    >
      {COLUMNS.map((column) => {
        if (column.key === 'status') {
          return (
            <span
              key={column.key}
              className={`batch-cell batch-status-${status?.tone ?? 'muted'}`}
              role="gridcell"
              title={status?.detail ?? status?.text}
              aria-label={status ? `${file.name} 的状态：${status.text}` : undefined}
            >
              {status?.text ?? ''}
            </span>
          )
        }
        const text = cellText(column.key, file, row)
        return (
          <span
            key={column.key}
            className={column.key === 'fileName' ? 'batch-cell batch-cell-name' : 'batch-cell'}
            role="gridcell"
            title={column.key === 'fileName' ? file.path : text}
          >
            {text}
          </span>
        )
      })}
    </div>
  )
})

interface SongTableProps {
  batch: BatchTags
  onSelect: (index: number, event: MouseEvent) => void
  onOpen: (index: number) => void
  onMove: (index: number, extend: boolean) => void
  onSelectAll: () => void
}

/** 右边的歌曲列表：固定行高虚拟滚动，列多于可视宽度时横向滚动（表头粘在顶部一起滚）。 */
function SongTable({ batch, onSelect, onOpen, onMove, onSelectAll }: SongTableProps) {
  const { files, rows, marks, selected, cursor } = batch
  const { scrollerRef, onScroll, range, totalHeight } = useVirtualRows(files.length, BATCH_ROW_HEIGHT)
  const visible = files.slice(range.start, range.end)

  // 当前行跑出可视区（键盘移动）时滚回来；表头粘在顶部，占掉的高度要让出来。
  const focus = cursor.focus
  useEffect(() => {
    const scroller = scrollerRef.current
    if (!scroller || focus < 0) return
    const top = focus * BATCH_ROW_HEIGHT
    const bottom = top + BATCH_ROW_HEIGHT + BATCH_HEAD_HEIGHT
    if (top < scroller.scrollTop) scroller.scrollTop = top
    else if (bottom > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = bottom - scroller.clientHeight
  }, [focus, scrollerRef])

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (files.length === 0) return
    const page = Math.max(1, Math.floor(((scrollerRef.current?.clientHeight ?? 0) - BATCH_HEAD_HEIGHT) / BATCH_ROW_HEIGHT) - 1)
    const current = cursor.focus
    let next: number | null = null
    if (event.key === 'ArrowDown') next = current + 1
    else if (event.key === 'ArrowUp') next = current < 0 ? 0 : current - 1
    else if (event.key === 'PageDown') next = current + page
    else if (event.key === 'PageUp') next = current - page
    else if (event.key === 'Home') next = 0
    else if (event.key === 'End') next = files.length - 1
    else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      onSelectAll()
      return
    }
    if (next === null) return
    event.preventDefault()
    onMove(Math.min(files.length - 1, Math.max(0, next)), event.shiftKey)
  }

  return (
    <div
      ref={scrollerRef}
      className="batch-table"
      role="grid"
      aria-label="歌曲列表"
      aria-multiselectable="true"
      aria-rowcount={files.length + 1}
      tabIndex={0}
      onScroll={onScroll}
      onKeyDown={onKeyDown}
      style={TABLE_STYLE}
    >
      <div className="batch-head" role="row" aria-rowindex={1}>
        {COLUMNS.map((column) => (
          <span key={column.key} className="batch-cell" role="columnheader">
            {column.label}
          </span>
        ))}
      </div>
      {files.length === 0 && (
        <p className="batch-empty">还没有歌曲。选择音乐目录，或把音频文件、文件夹拖进窗口。</p>
      )}
      <div className="batch-spacer" role="rowgroup" style={{ height: totalHeight }}>
        {visible.map((file, offset) => {
          const index = range.start + offset
          const key = pathKey(file.path)
          return (
            <SongRow
              key={key}
              file={file}
              row={rows.get(key)}
              mark={marks.get(key)}
              index={index}
              selected={selected.has(key)}
              focused={index === cursor.focus}
              onSelect={onSelect}
              onOpen={onOpen}
            />
          )
        })}
      </div>
    </div>
  )
}

/** 面板里的一个字段：值不一样时显示「〈保持〉」，动过的字段高亮并可以单独还原。 */
function PaneField({
  name,
  label,
  view,
  disabled,
  onChange,
  onReset,
}: {
  name: TagFieldName
  label: string
  view: FieldView
  disabled: boolean
  onChange: (name: TagFieldName, value: string) => void
  onReset: (name: TagFieldName) => void
}) {
  // 动过又留空 = 把这一项清掉；没动过且各首不一样 = 各自保持。
  const placeholder = view.edited ? CLEAR_PLACEHOLDER : view.mixed ? KEEP_PLACEHOLDER : ''
  return (
    <div className="tagpane-field">
      <label className="tagpane-label" htmlFor={`tagpane-${name}`}>
        {label}
      </label>
      <div className="tagpane-control">
        <input
          id={`tagpane-${name}`}
          type="text"
          className={view.edited ? 'editor-input tagpane-input tagpane-input-edited' : 'editor-input tagpane-input'}
          aria-label={label}
          spellCheck={false}
          placeholder={placeholder}
          value={view.value}
          disabled={disabled}
          onChange={(event) => onChange(name, event.target.value)}
        />
        {view.edited && (
          <button
            type="button"
            className="tagpane-reset"
            aria-label={`还原${label}`}
            title="还原这一项"
            disabled={disabled}
            onClick={() => onReset(name)}
          >
            <svg width="13" height="13" viewBox="0 0 24 24" aria-hidden="true">
              <path
                fill="none"
                stroke="currentColor"
                strokeWidth="2.2"
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M4 9h10a5.5 5.5 0 0 1 0 11H8M4 9l4-4M4 9l4 4"
              />
            </svg>
          </button>
        )}
      </div>
    </div>
  )
}

function CoverMetaList({ meta }: { meta: CoverMeta }) {
  const items = [
    { label: '格式', value: meta.mimeType },
    { label: '尺寸', value: meta.dimensions },
    { label: '大小', value: meta.size },
    { label: '类型', value: meta.pictureType },
  ].filter((item): item is { label: string; value: string } => item.value != null)
  return (
    <ul className="editor-cover-meta" aria-label="封面参数">
      {items.map((item) => (
        <li key={item.label} title={item.label} aria-label={`封面${item.label}：${item.value}`}>
          {item.value}
        </li>
      ))}
    </ul>
  )
}

/** 左边的标签面板：选中歌曲的标签、歌词预览与封面。 */
function TagPane({ batch, onEditLyrics }: { batch: BatchTags; onEditLyrics: () => void }) {
  const { targets, selected, fields, coverView, cover, phase } = batch
  const count = targets.length
  const working = phase === 'saving' || phase === 'online'
  const disabled = count === 0 || working
  const only = count === 1 ? targets[0] : undefined
  const pending = selected.size - count

  let heading = '在右边选择要修改的歌曲'
  if (only) heading = only.fileName
  else if (count > 1) heading = `已选 ${count} 首：值不同的字段显示${KEEP_PLACEHOLDER}`
  else if (selected.size > 0) heading = '选中的歌曲还没有读出标签，或无法读取'

  let lyricsText = ''
  if (only) lyricsText = only.lyricsPreview ?? '没有歌词'
  else if (count > 1) lyricsText = `${count} 首中 ${targets.filter((row) => row.hasLyrics).length} 首有歌词`

  const fieldNodes = PANE_ROWS.map((row) => {
    const nodes = row.map((field) => (
      <PaneField
        key={field.name}
        name={field.name}
        label={field.label}
        view={fields[field.name]}
        disabled={disabled}
        onChange={batch.setField}
        onReset={batch.resetField}
      />
    ))
    if (row.length === 1) return nodes[0]
    return (
      <div key={row.map((field) => field.name).join('-')} className="tagpane-row">
        {nodes}
      </div>
    )
  })

  return (
    <section className="tagpane" aria-label="标签面板">
      <p className="tagpane-heading" title={only?.path} aria-live="polite">
        {heading}
        {pending > 0 && count > 0 && <span className="tagpane-heading-note">（另有 {pending} 首无法修改）</span>}
      </p>
      {fieldNodes}
      <div className="tagpane-field">
        <span className="tagpane-label">歌词</span>
        <div className="tagpane-control">
          <span
            className={only?.lyricsPreview ? 'tagpane-lyrics' : 'tagpane-lyrics tagpane-lyrics-note'}
            aria-label="歌词预览"
            title={lyricsText}
          >
            {lyricsText}
          </span>
          <button
            type="button"
            className="btn btn-small"
            aria-label="用歌词与标签编辑器打开选中的歌曲"
            title={only ? '用歌词与标签编辑器打开这首歌，可以搜索在线歌词' : '选中一首歌后可以编辑歌词'}
            disabled={!only || working}
            onClick={onEditLyrics}
          >
            编辑…
          </button>
        </div>
      </div>
      <div className="tagpane-cover">
        <div className="editor-cover" aria-label={coverView.src ? '封面预览' : '没有封面预览'} role="img">
          {coverView.src ? <img src={coverView.src} alt="封面" draggable={false} /> : <span aria-hidden="true">♪</span>}
        </div>
        <div className="editor-cover-side">
          {coverView.meta && <CoverMetaList meta={coverView.meta} />}
          {coverView.caption !== '' && <p className="editor-cover-caption">{coverView.caption}</p>}
          <div className="editor-cover-actions">
            <button
              type="button"
              className="btn btn-small"
              aria-label="更换封面"
              disabled={disabled}
              onClick={() => void batch.chooseCover()}
            >
              更换封面…
            </button>
            <button
              type="button"
              className="btn btn-small"
              aria-label="移除封面"
              disabled={disabled || cover.kind === 'removed' || (cover.kind === 'keep' && !targets.some((row) => row.hasCover))}
              onClick={batch.removeCover}
            >
              移除封面
            </button>
            {cover.kind !== 'keep' && (
              <button
                type="button"
                className="btn btn-small btn-ghost"
                aria-label="还原封面"
                disabled={working}
                onClick={batch.restoreCover}
              >
                还原
              </button>
            )}
          </div>
        </div>
      </div>
    </section>
  )
}

interface BatchTagsDialogProps {
  /** 打开时要列出的目录（从编辑器进来时是当前文件所在的目录）；留空就等用户选。 */
  initialFolder: string
  /** 拖入的文件（打开时的，以及窗口开着时新拖入的）。 */
  drop?: BatchDrop | null
  onClose: () => void
  /** 用歌词与标签编辑器打开一首歌（叠在本窗口上面）；`onClose` 在编辑器关闭时调用。 */
  renderEditor?: (path: string, onClose: () => void) => ReactNode
}

/**
 * 批量修改标签：右边是歌曲列表（扫描目录或拖入的文件），选中歌曲后左边显示它们的标签；
 * 改动只写动过的字段，可以一次改多首。对选中的歌还能在线获取标签、封面与歌词
 * （最多 3 个文件并发，逐文件状态走 `batch:progress` 事件，可取消）。从编辑器打开时叠在编辑器之上。
 *
 * 窗口开着时再拖入会换成新的清单（正在处理时不换），「改用目录…」切回按目录扫描。
 */
export default function BatchTagsDialog({ initialFolder, drop = null, onClose, renderEditor }: BatchTagsDialogProps) {
  const batch = useBatchTags(initialFolder, drop?.files ?? null)
  const { files, selected, cursor, phase, dirty, config } = batch
  const [confirm, setConfirm] = useState<{ message: string; confirmLabel: string; run: () => void } | null>(null)
  const [editorPath, setEditorPath] = useState<string | null>(null)
  const handledSeq = useRef(drop?.seq ?? 0)
  const editorOpenRef = useRef(false)
  const stateRef = useRef({ files, selected, cursor })
  useEffect(() => {
    editorOpenRef.current = editorPath !== null
    stateRef.current = { files, selected, cursor }
  })

  const { dirtyRef, phaseRef } = batch
  /** 有未保存的修改时先确认，再执行 `action`。 */
  const guard = useCallback(
    (message: string, confirmLabel: string, action: () => void) => {
      if (dirtyRef.current) setConfirm({ message, confirmLabel, run: action })
      else action()
    },
    [dirtyRef],
  )
  const working = phase === 'saving' || phase === 'online'
  const locked = working || phase === 'scanning'

  // ───────────────────────────── 选择 ─────────────────────────────

  const { applySelection, setCursor } = batch
  /** 换成新的选择：选的还是那些歌就只挪当前行；否则先确认没保存的修改。 */
  const requestSelection = useCallback(
    (next: Set<string>, anchor: number, focus: number) => {
      const current = stateRef.current.selected
      const same = next.size === current.size && [...next].every((key) => current.has(key))
      if (same) setCursor({ anchor, focus })
      else guard('当前修改尚未保存，换成别的歌曲会丢弃这些修改。', '放弃修改并切换', () => applySelection(next, anchor, focus))
    },
    [applySelection, guard, setCursor],
  )

  const rangeKeys = useCallback((from: number, to: number) => {
    const list = stateRef.current.files
    const keys = new Set<string>()
    for (let index = Math.min(from, to); index <= Math.max(from, to); index += 1) {
      const file = list[index]
      if (file) keys.add(pathKey(file.path))
    }
    return keys
  }, [])

  const onSelect = useCallback(
    (index: number, event: MouseEvent) => {
      if (phaseRef.current === 'saving' || phaseRef.current === 'online') return
      const { files: list, selected: current, cursor: at } = stateRef.current
      const file = list[index]
      if (!file) return
      const key = pathKey(file.path)
      const additive = event.ctrlKey || event.metaKey
      if (event.shiftKey && at.anchor >= 0) {
        const keys = rangeKeys(at.anchor, index)
        if (additive) for (const existing of current) keys.add(existing)
        requestSelection(keys, at.anchor, index)
      } else if (additive) {
        const keys = new Set(current)
        if (!keys.delete(key)) keys.add(key)
        requestSelection(keys, index, index)
      } else {
        requestSelection(new Set([key]), index, index)
      }
    },
    [phaseRef, rangeKeys, requestSelection],
  )

  const onMove = useCallback(
    (index: number, extend: boolean) => {
      if (phaseRef.current === 'saving' || phaseRef.current === 'online') return
      const at = stateRef.current.cursor
      const anchor = extend && at.anchor >= 0 ? at.anchor : index
      requestSelection(rangeKeys(anchor, index), anchor, index)
    },
    [phaseRef, rangeKeys, requestSelection],
  )

  const onSelectAll = useCallback(() => {
    if (phaseRef.current === 'saving' || phaseRef.current === 'online') return
    const list = stateRef.current.files
    if (list.length === 0) return
    const at = stateRef.current.cursor
    requestSelection(rangeKeys(0, list.length - 1), 0, at.focus >= 0 ? at.focus : 0)
  }, [phaseRef, rangeKeys, requestSelection])

  // ───────────────────────────── 目录与拖放 ─────────────────────────────

  const { scan, loadDropped, leaveDropped, report } = batch
  const requestScan = useCallback(
    (target: string, includeSubfolders: boolean) => {
      guard('当前修改尚未保存，重新列出歌曲会丢弃这些修改。', '放弃修改并继续', () => void scan(target, includeSubfolders))
    },
    [guard, scan],
  )

  const browse = useCallback(() => {
    pickMusicFolder('选择要批量修改标签的目录')
      .then((picked) => {
        if (!picked) return
        batch.setFolder(picked)
        requestScan(picked, batch.recursive)
      })
      .catch((error: unknown) => report(`选择目录失败：${errorMessage(error, '请重试')}`, true))
  }, [batch, report, requestScan])

  // 窗口开着时又拖入了一批：没在处理就换成新的清单（同一个 seq 只处理一次，严格模式会把副作用跑两遍）。
  useEffect(() => {
    if (!drop || drop.seq === handledSeq.current) return
    handledSeq.current = drop.seq
    if (editorOpenRef.current) return
    if (phaseRef.current === 'saving' || phaseRef.current === 'online') {
      report('正在处理，完成或取消后再拖入新的文件。', true)
      return
    }
    const next = drop.files
    guard('当前修改尚未保存，换成拖入的文件会丢弃这些修改。', '放弃修改并换成这批', () => loadDropped(next))
  }, [drop, guard, loadDropped, phaseRef, report])

  // ───────────────────────────── 编辑器、保存与关闭 ─────────────────────────────

  const openEditor = useCallback(
    (path: string) => {
      if (!renderEditor || phaseRef.current === 'saving' || phaseRef.current === 'online') return
      guard('当前修改尚未保存，打开歌词与标签编辑器会丢弃这些修改。', '放弃修改并打开', () => {
        batch.discardEdits()
        setEditorPath(path)
      })
    },
    [batch, guard, phaseRef, renderEditor],
  )

  const onOpenRow = useCallback(
    (index: number) => {
      const file = stateRef.current.files[index]
      if (file && batch.rows.get(pathKey(file.path))?.readable) openEditor(file.path)
    },
    [batch.rows, openEditor],
  )

  const { dispose } = batch
  const requestClose = useCallback(() => {
    guard('当前修改尚未保存，关闭后会丢失这些修改。', '放弃修改并关闭', () => {
      dispose()
      onClose()
    })
  }, [dispose, guard, onClose])

  // Ctrl+S 保存（上面叠着编辑器或确认框时不响应）。
  const { save } = batch
  const coveredRef = useRef(false)
  useEffect(() => {
    coveredRef.current = editorPath !== null || confirm !== null
  })
  useEffect(() => {
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (coveredRef.current) return
      if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 's') {
        event.preventDefault()
        void save()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [save])

  const targetCount = batch.targets.length
  const onlineCount = batch.onlineTargets.length
  const canSave = dirty && phase === 'idle' && targetCount > 0
  const canOnline = !dirty && phase === 'idle' && onlineCount > 0
  const percent = batch.progress && batch.progress.total > 0 ? (batch.progress.done * 100) / batch.progress.total : 0
  const countText = useMemo(
    () => (files.length === 0 ? '没有歌曲' : `已选 ${selected.size} / 共 ${files.length} 首`),
    [files.length, selected.size],
  )

  return (
    <>
      <Modal
        title="批量修改标签"
        className="batch-dialog"
        onClose={requestClose}
        footer={
          <>
            <div className="batch-foot-status">
              <span className="batch-foot-count">{countText}</span>
              <span
                className={batch.problem ? 'batch-foot-text batch-foot-problem' : 'batch-foot-text'}
                role="status"
                aria-live="polite"
              >
                {dirty && <span className="editor-dirty">● 有未保存的修改 · </span>}
                {batch.status}
              </span>
            </div>
            {working ? (
              <button
                type="button"
                className="btn btn-danger"
                aria-label={phase === 'saving' ? '取消保存' : '取消在线获取'}
                disabled={batch.cancelling}
                onClick={batch.cancel}
              >
                {batch.cancelling ? '正在取消…' : '取消'}
              </button>
            ) : (
              <>
                <button
                  type="button"
                  className="btn"
                  aria-label="对选中的歌曲在线获取标签、封面与歌词"
                  title={dirty ? '请先保存或还原左边的修改' : '按标签 / 文件名在线匹配，写入标签、封面与歌词'}
                  disabled={!canOnline}
                  onClick={() => void batch.runOnline()}
                >
                  {selected.size > 0 ? `在线获取（选中的 ${onlineCount} 首）` : `在线获取（全部 ${onlineCount} 首）`}
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  aria-label="把修改保存到选中的歌曲"
                  title="Ctrl+S"
                  disabled={!canSave}
                  onClick={() => void batch.save()}
                >
                  {targetCount > 1 ? `保存到 ${targetCount} 首` : '保存修改'}
                </button>
              </>
            )}
            <button type="button" className="btn" aria-label="关闭批量修改标签窗口" onClick={requestClose}>
              关闭
            </button>
          </>
        }
      >
        <div className="batch-layout">
          <div className="batch-source" aria-label="歌曲来源">
            {batch.dropped ? (
              <>
                <span className="batch-dropped" aria-label="歌曲列表的来源">
                  拖入的 {batch.dropped.length} 首音频（也可以继续拖入，换成新的一批）
                </span>
                <button
                  type="button"
                  className="btn"
                  aria-label="改为按目录列出音频"
                  disabled={locked}
                  onClick={() =>
                    guard('当前修改尚未保存，改用目录会丢弃这些修改。', '放弃修改并继续', () => {
                      leaveDropped()
                      browse()
                    })
                  }
                >
                  改用目录…
                </button>
              </>
            ) : (
              <>
                <input
                  type="text"
                  className="ncm-input"
                  aria-label="音乐目录"
                  spellCheck={false}
                  placeholder="选择包含音频文件的目录，或把文件、文件夹拖进窗口"
                  value={batch.folder}
                  disabled={locked}
                  onChange={(event) => batch.setFolder(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') requestScan(batch.folder, batch.recursive)
                  }}
                />
                <button type="button" className="btn" aria-label="浏览音乐目录" disabled={locked} onClick={browse}>
                  浏览…
                </button>
                <button
                  type="button"
                  className="btn"
                  aria-label="重新列出目录里的音频"
                  disabled={locked || batch.folder.trim() === ''}
                  onClick={() => requestScan(batch.folder, batch.recursive)}
                >
                  刷新
                </button>
                <label className="ncm-check">
                  <input
                    type="checkbox"
                    aria-label="包含子文件夹"
                    checked={batch.recursive}
                    disabled={locked}
                    onChange={(event) => {
                      const next = event.target.checked
                      batch.setRecursive(next)
                      if (batch.folder.trim() !== '') requestScan(batch.folder, next)
                    }}
                  />
                  <span>包含子文件夹</span>
                </label>
              </>
            )}
          </div>

          <div className="batch-main">
            <TagPane
              batch={batch}
              onEditLyrics={() => {
                if (batch.singlePath) openEditor(batch.singlePath)
              }}
            />
            <div className="batch-list">
              <div className="batch-online" aria-label="在线获取设置">
                <span className="batch-online-title">在线获取</span>
                <label className="ncm-check">
                  <input
                    type="checkbox"
                    aria-label="覆盖已有标签与歌词"
                    checked={batch.overwrite}
                    disabled={working}
                    onChange={(event) => batch.setOverwrite(event.target.checked)}
                  />
                  <span>覆盖已有标签与歌词</span>
                </label>
                <label className="ncm-check">
                  <input
                    type="checkbox"
                    aria-label="同时获取歌词"
                    checked={batch.fetchLyrics}
                    disabled={working}
                    onChange={(event) => batch.setFetchLyrics(event.target.checked)}
                  />
                  <span>同时获取歌词</span>
                </label>
                <select
                  className="editor-select"
                  aria-label="批量歌词格式"
                  value={batch.preferWord ? 'word' : 'line'}
                  disabled={working || !batch.fetchLyrics}
                  onChange={(event) => batch.setPreferWord(event.target.value === 'word')}
                >
                  <option value="line">逐行歌词</option>
                  <option value="word">逐字歌词（有则优先）</option>
                </select>
                <span
                  className="batch-online-note"
                  aria-label="来源优先级与匹配度阈值"
                  title="来源顺序在设置的「在线匹配」里调整；低于匹配度阈值的歌曲会被跳过"
                >
                  标签 {config?.tagPriorityText ?? '网易云音乐优先'} · 歌词 {config?.lyricsPriorityText ?? '网易云音乐优先'} · 匹配度阈值{' '}
                  {config?.threshold.percent ?? '60'}%
                </span>
              </div>
              <SongTable batch={batch} onSelect={onSelect} onOpen={onOpenRow} onMove={onMove} onSelectAll={onSelectAll} />
            </div>
          </div>

          <progress
            className={batch.progress ? 'ncm-progress' : 'ncm-progress batch-progress-idle'}
            aria-label="处理进度"
            aria-hidden={batch.progress === null}
            value={percent}
            max={100}
          />
        </div>
      </Modal>
      {editorPath &&
        renderEditor?.(editorPath, () => {
          batch.refreshFile(editorPath)
          setEditorPath(null)
        })}
      {confirm && (
        <ConfirmDialog
          title="有未保存的修改"
          message={confirm.message}
          confirmLabel={confirm.confirmLabel}
          cancelLabel="继续编辑"
          danger
          onConfirm={() => {
            const run = confirm.run
            setConfirm(null)
            run()
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </>
  )
}
