import { memo, useCallback, useEffect, useRef, type CSSProperties, type KeyboardEvent, type MouseEvent } from 'react'
import { errorMessage, pickMusicFolder } from '../api/editor'
import type { BatchFile } from '../api/online'
import { useVirtualRows } from '../lib/virtual'
import { pathKey, type RowMark } from '../state/useBatchTags'
import { useWordLyrics, type WordLyrics } from '../state/useWordLyrics'
import { BATCH_ROW_HEIGHT, type BatchDrop } from './BatchTagsDialog'
import ConfirmDialog from './ConfirmDialog'
import Modal from './Modal'

/** 表头高度（px）：表头粘在滚动区顶部，键盘移动时要把它让出来；与样式里的 `--batch-head-height` 一致。 */
const HEAD_HEIGHT = 30

const COLUMNS = ['文件名', '状态', '所在目录'] as const
/** 只有三列，按比例占满列表宽度（不用横向滚动）；列表样式与批量修改标签共用。 */
const TABLE_STYLE = {
  '--batch-columns': 'minmax(180px, 1.3fr) minmax(200px, 1.5fr) minmax(140px, 1fr)',
  '--batch-width': '100%',
} as CSSProperties

/** 文件所在的目录（去掉文件名）。 */
function directoryOf(path: string): string {
  const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'))
  return cut > 0 ? path.slice(0, cut) : ''
}

interface SongRowProps {
  file: BatchFile
  mark: RowMark | undefined
  index: number
  selected: boolean
  focused: boolean
  onSelect: (index: number, event: MouseEvent) => void
}

const SongRow = memo(function SongRow({ file, mark, index, selected, focused, onSelect }: SongRowProps) {
  const className = ['batch-row', selected ? 'batch-row-selected' : '', focused ? 'batch-row-focused' : '']
    .filter((name) => name !== '')
    .join(' ')
  const directory = directoryOf(file.path)
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
    >
      <span className="batch-cell batch-cell-name" role="gridcell" title={file.path}>
        {file.name}
      </span>
      <span
        className={`batch-cell batch-status-${mark?.tone ?? 'muted'}`}
        role="gridcell"
        title={mark?.detail ?? mark?.text}
        aria-label={mark ? `${file.name} 的状态：${mark.text}` : undefined}
      >
        {mark?.text ?? ''}
      </span>
      <span className="batch-cell" role="gridcell" title={directory}>
        {directory}
      </span>
    </div>
  )
})

/** 歌曲列表：固定行高虚拟滚动；单击 / Ctrl / Shift / Ctrl+A / 方向键选歌。 */
function SongTable({ words }: { words: WordLyrics }) {
  const { files, marks, selected, cursor, select, move, selectAll } = words
  const { scrollerRef, onScroll, range, totalHeight } = useVirtualRows(files.length, BATCH_ROW_HEIGHT)
  const visible = files.slice(range.start, range.end)

  // 当前行跑出可视区（键盘移动）时滚回来；表头粘在顶部，占掉的高度要让出来。
  const focus = cursor.focus
  useEffect(() => {
    const scroller = scrollerRef.current
    if (!scroller || focus < 0) return
    const top = focus * BATCH_ROW_HEIGHT
    const bottom = top + BATCH_ROW_HEIGHT + HEAD_HEIGHT
    if (top < scroller.scrollTop) scroller.scrollTop = top
    else if (bottom > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = bottom - scroller.clientHeight
  }, [focus, scrollerRef])

  const onSelect = useCallback(
    (index: number, event: MouseEvent) => select(index, event.shiftKey, event.ctrlKey || event.metaKey),
    [select],
  )

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (files.length === 0) return
    const page = Math.max(1, Math.floor(((scrollerRef.current?.clientHeight ?? 0) - HEAD_HEIGHT) / BATCH_ROW_HEIGHT) - 1)
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
      selectAll()
      return
    }
    if (next === null) return
    event.preventDefault()
    move(Math.min(files.length - 1, Math.max(0, next)), event.shiftKey)
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
        {COLUMNS.map((label) => (
          <span key={label} className="batch-cell" role="columnheader">
            {label}
          </span>
        ))}
      </div>
      {files.length === 0 && <p className="batch-empty">还没有歌曲。选择音乐目录，或把音频文件、文件夹拖进窗口。</p>}
      <div className="batch-spacer" role="rowgroup" style={{ height: totalHeight }}>
        {visible.map((file, offset) => {
          const index = range.start + offset
          const key = pathKey(file.path)
          return (
            <SongRow
              key={key}
              file={file}
              mark={marks.get(key)}
              index={index}
              selected={selected.has(key)}
              focused={index === cursor.focus}
              onSelect={onSelect}
            />
          )
        })}
      </div>
    </div>
  )
}

interface WordLyricsDialogProps {
  /** 打开时要列出的目录（主界面给的是曲库目录）；留空就等用户选。 */
  initialFolder: string
  /** 窗口开着时拖入的文件。 */
  drop?: BatchDrop | null
  onClose: () => void
}

/**
 * 歌词转逐字：列出一个目录里的歌（或拖入的文件），把选中的歌（不选就是全部）现有的逐行歌词换成逐字歌词。
 * 分两步——先联网检查哪些能换（不写文件，结论写在每首歌的「状态」里），有能换的才弹确认框，确认后才写入。
 * 从主界面标题栏的「歌词转逐字」按钮打开。
 */
export default function WordLyricsDialog({ initialFolder, drop = null, onClose }: WordLyricsDialogProps) {
  const words = useWordLyrics(initialFolder, null)
  const { files, selected, phase, working, pending, scan, loadDropped, leaveDropped, report } = words
  const locked = working || phase === 'scanning'

  const browse = useCallback(() => {
    pickMusicFolder('选择要把歌词转成逐字的目录')
      .then((picked) => {
        if (!picked) return
        words.setFolder(picked)
        void scan(picked, words.recursive)
      })
      .catch((error: unknown) => report(`选择目录失败：${errorMessage(error, '请重试')}`, true))
  }, [report, scan, words])

  // 窗口开着时拖入了一批：没在处理就换成新的清单（同一个 seq 只处理一次，严格模式会把副作用跑两遍）。
  const handledSeq = useRef(0)
  useEffect(() => {
    if (!drop || drop.seq === handledSeq.current) return
    handledSeq.current = drop.seq
    loadDropped(drop.files)
  }, [drop, loadDropped])

  const { dispose } = words
  const requestClose = useCallback(() => {
    dispose()
    onClose()
  }, [dispose, onClose])

  const targetCount = words.targets.length
  const canStart = phase === 'idle' && targetCount > 0
  const percent = words.progress && words.progress.total > 0 ? (words.progress.done * 100) / words.progress.total : 0
  const countText = files.length === 0 ? '没有歌曲' : `已选 ${selected.size} / 共 ${files.length} 首`

  return (
    <>
      <Modal
        title="歌词转逐字"
        className="batch-dialog word-dialog"
        onClose={requestClose}
        footer={
          <>
            <div className="batch-foot-status">
              <span className="batch-foot-count">{countText}</span>
              <span
                className={words.problem ? 'batch-foot-text batch-foot-problem' : 'batch-foot-text'}
                role="status"
                aria-live="polite"
                title={words.status}
              >
                {words.status}
              </span>
            </div>
            {working ? (
              <button
                type="button"
                className="btn btn-danger"
                aria-label={phase === 'checking' ? '取消检查' : '取消转换'}
                disabled={words.cancelling}
                onClick={words.cancel}
              >
                {words.cancelling ? '正在取消…' : '取消'}
              </button>
            ) : (
              <button
                type="button"
                className="btn btn-primary"
                aria-label="检查歌词能否换成逐字歌词"
                title="先联网检查哪些歌能换（不改文件），有能换的会再问一次，确认后才写入"
                disabled={!canStart}
                onClick={() => void words.preview()}
              >
                {selected.size > 0 ? `检查并转换（选中的 ${targetCount} 首）` : `检查并转换（全部 ${targetCount} 首）`}
              </button>
            )}
            <button type="button" className="btn" aria-label="关闭歌词转逐字窗口" onClick={requestClose}>
              关闭
            </button>
          </>
        }
      >
        <div className="batch-layout">
          <div className="batch-source" aria-label="歌曲来源">
            {words.dropped ? (
              <>
                <span className="batch-dropped" aria-label="歌曲列表的来源">
                  拖入的 {words.dropped.length} 首音频（也可以继续拖入，换成新的一批）
                </span>
                <button
                  type="button"
                  className="btn"
                  aria-label="改为按目录列出音频"
                  disabled={locked}
                  onClick={() => {
                    leaveDropped()
                    browse()
                  }}
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
                  value={words.folder}
                  disabled={locked}
                  onChange={(event) => words.setFolder(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') void scan(words.folder, words.recursive)
                  }}
                />
                <button type="button" className="btn" aria-label="浏览音乐目录" disabled={locked} onClick={browse}>
                  浏览…
                </button>
                <button
                  type="button"
                  className="btn"
                  aria-label="重新列出目录里的音频"
                  disabled={locked || words.folder.trim() === ''}
                  onClick={() => void scan(words.folder, words.recursive)}
                >
                  刷新
                </button>
                <label className="ncm-check">
                  <input
                    type="checkbox"
                    aria-label="包含子文件夹"
                    checked={words.recursive}
                    disabled={locked}
                    onChange={(event) => {
                      const next = event.target.checked
                      words.setRecursive(next)
                      if (words.folder.trim() !== '') void scan(words.folder, next)
                    }}
                  />
                  <span>包含子文件夹</span>
                </label>
              </>
            )}
          </div>

          <p className="word-note" aria-label="歌词转逐字说明">
            把现有的逐行歌词换成逐字歌词：先联网检查哪些能换（不改文件），确认后才写入。已经是逐字的、没有歌词的不动；原有的译文会保留；同名
            .lrc 改写前会备份到 _backup 文件夹。
            {selected.size > 0 && (
              <button
                type="button"
                className="word-note-action"
                aria-label="取消选择，改为处理全部歌曲"
                disabled={locked}
                onClick={words.clearSelection}
              >
                取消选择（处理全部）
              </button>
            )}
          </p>

          <SongTable words={words} />

          <progress
            className={words.progress ? 'ncm-progress' : 'ncm-progress batch-progress-idle'}
            aria-label="处理进度"
            aria-hidden={words.progress === null}
            value={percent}
            max={100}
          />
        </div>
      </Modal>
      {pending && (
        <ConfirmDialog
          title="把歌词换成逐字歌词"
          message={`${pending.text}。要把这 ${pending.ready.length} 首的歌词换成逐字歌词吗？内嵌在音频文件里的歌词会写进音频文件；歌词文件（.lrc）会被改写，原文件先备份到它所在目录的 _backup 文件夹。现有的译文会保留，其他标签不动。`}
          confirmLabel={`转换 ${pending.ready.length} 首`}
          cancelLabel="先不转换"
          onConfirm={() => void words.apply()}
          onCancel={words.dismiss}
        />
      )}
    </>
  )
}
