import { memo, useEffect, useRef } from 'react'
import { availabilityLabel, methodLabel, type LocalMatch } from '../api/library'
import type { Song } from '../api/playlists'
import { artistText, formatDuration, formatFileSize } from '../lib/format'
import { useVirtualRows } from '../lib/virtual'
import type { RowState, TransferRows } from '../state/useTransfer'

/** 固定行高（px）：虚拟滚动依赖它，样式里的 `--song-row-height` 必须与此一致。 */
export const SONG_ROW_HEIGHT = 48

/** 本地状态悬停提示：匹配到的文件、匹配方式与分数、完整路径。 */
function localTooltip(match: LocalMatch | null): string {
  if (match === null) return '尚未对比本地曲库，可点击上方的「对比本地」'
  if (match.availability === 'missing' || match.path === null) return '未在本地曲库中找到匹配文件'
  const score = match.method === 'neteaseId' ? '' : ` · ${Math.round(match.confidence * 100)}%`
  const lines = [
    `本地文件：${match.fileName ?? match.path}`,
    `匹配方式：${methodLabel(match.method)}${score}`,
    `路径：${match.path}`,
  ]
  if (match.availability === 'ncm') lines.push('这是未解密的 NCM 文件，需要先解密才能播放')
  return lines.join('\n')
}

/** 下载状态徽标的颜色类与文字。 */
function transferBadge(state: RowState): { tone: string; label: string } {
  switch (state.status) {
    case 'queued':
      return { tone: 'idle', label: '排队中' }
    case 'done':
      return { tone: 'ok', label: state.source === 'decryptedNcm' ? '已解密' : '已下载' }
    case 'skipped':
      return { tone: 'idle', label: '已跳过' }
    case 'failed':
      return { tone: 'bad', label: '失败' }
    case 'cancelled':
      return { tone: 'idle', label: '已取消' }
    case 'downloading':
      return {
        tone: 'busy',
        label: state.percent !== null ? `下载中 ${Math.round(state.percent)}%` : '下载中',
      }
    case 'decrypting':
      return { tone: 'busy', label: '解密中' }
    case 'tagging':
      return { tone: 'busy', label: '写入标签' }
    case 'matching':
      return { tone: 'busy', label: '在线匹配' }
    default:
      return { tone: 'busy', label: '准备中' }
  }
}

/** 下载状态第二行的说明：失败原因、实际音质、字节进度或补充说明。 */
function transferDetail(state: RowState): string | null {
  if (state.status === 'failed') return state.message
  if (state.status === 'downloading' && state.bytesDone !== null) {
    return state.bytesTotal !== null
      ? `${formatFileSize(state.bytesDone)} / ${formatFileSize(state.bytesTotal)}`
      : formatFileSize(state.bytesDone)
  }
  return state.qualityNote ?? state.message
}

/** 悬停提示：完整的说明文字与输出路径。 */
function transferTooltip(state: RowState): string {
  const lines = [state.stage]
  if (state.qualityNote) lines.push(state.qualityNote)
  if (state.message) lines.push(state.message)
  if (state.outputPath) lines.push(`文件：${state.outputPath}`)
  return lines.join('\n')
}

function TransferCell({ state }: { state: RowState | null }) {
  if (state === null) return <span className="song-cell song-transfer" role="cell" />
  const badge = transferBadge(state)
  const detail = transferDetail(state)
  const showBar = state.status === 'downloading' && state.percent !== null
  return (
    <span
      className="song-cell song-transfer"
      role="cell"
      aria-label={`下载状态：${badge.label}${detail ? `，${detail}` : ''}`}
      title={transferTooltip(state)}
    >
      <span className={`transfer-badge transfer-${badge.tone}`}>{badge.label}</span>
      {detail && <span className={state.status === 'failed' ? 'transfer-detail transfer-detail-bad' : 'transfer-detail'}>{detail}</span>}
      {showBar && (
        <span className="transfer-bar" aria-hidden="true">
          <span className="transfer-bar-fill" style={{ width: `${state.percent}%` }} />
        </span>
      )}
    </span>
  )
}

const SongRow = memo(function SongRow({
  song,
  index,
  match,
  checked,
  transfer,
  onShowLyrics,
  onEditFile,
  onToggle,
}: {
  song: Song
  index: number
  /** 本地匹配结果；`null` 表示还没有对比。 */
  match: LocalMatch | null
  checked: boolean
  /** 这首歌的下载状态；`null` 表示没有任务涉及它。 */
  transfer: RowState | null
  onShowLyrics: (song: Song) => void
  /** 在歌词与标签编辑器中打开这首歌的本地文件。 */
  onEditFile: (path: string) => void
  /** 点复选框：第二个参数是点击时是否按着 Shift。 */
  onToggle: (index: number, extend: boolean) => void
}) {
  const name = song.name.trim() === '' ? '未知歌曲' : song.name
  const artists = artistText(song)
  const album = song.album.name.trim() === '' ? '—' : song.album.name
  return (
    <div
      className={checked ? 'song-row song-grid song-row-selected' : 'song-row song-grid'}
      role="row"
      aria-rowindex={index + 2}
      aria-selected={checked}
      style={{ transform: `translateY(${index * SONG_ROW_HEIGHT}px)` }}
      onDoubleClick={() => onShowLyrics(song)}
    >
      <span className="song-cell song-check" role="cell">
        <input
          type="checkbox"
          className="song-checkbox"
          aria-label={`选择：${name}`}
          checked={checked}
          onChange={() => undefined}
          onClick={(event) => onToggle(index, event.shiftKey)}
        />
      </span>
      <span className="song-cell song-index" role="cell">
        {index + 1}
      </span>
      <span className="song-cell song-title-cell" role="cell" title={song.alias ? `${name}（${song.alias}）` : name}>
        <span className="song-title">{name}</span>
        {song.alias && <span className="song-alias">{song.alias}</span>}
      </span>
      <span className="song-cell song-secondary" role="cell" title={artists}>
        {artists}
      </span>
      <span className="song-cell song-secondary song-album" role="cell" title={album}>
        {album}
      </span>
      <span className="song-cell song-duration" role="cell">
        {formatDuration(song.durationMs)}
      </span>
      <span
        className="song-cell song-local"
        role="cell"
        aria-label={`本地状态：${match ? availabilityLabel(match.availability) : '尚未对比'}`}
        title={localTooltip(match)}
      >
        {match ? <span className={`local-badge local-${match.availability}`}>{availabilityLabel(match.availability)}</span> : '—'}
      </span>
      <TransferCell state={transfer} />
      <span className="song-cell song-action" role="cell">
        {match?.availability === 'available' && match.path !== null && (
          <button
            type="button"
            className="song-lyrics-btn"
            aria-label={`在编辑器中打开：${name}`}
            title={`在歌词与标签编辑器中打开本地文件
${match.path}`}
            onClick={() => onEditFile(match.path ?? '')}
          >
            编
          </button>
        )}
        <button
          type="button"
          className="song-lyrics-btn"
          aria-label={`查看歌词：${name}`}
          title="查看歌词（也可双击这一行）"
          onClick={() => onShowLyrics(song)}
        >
          词
        </button>
      </span>
    </div>
  )
})

/** 表头的全选复选框：部分选中时显示横线（indeterminate）。「下载缺失」的确认清单也用它。 */
export function HeaderCheckbox({ checked, indeterminate, onChange }: { checked: boolean; indeterminate: boolean; onChange: () => void }) {
  const ref = useRef<HTMLInputElement>(null)
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = indeterminate
  }, [indeterminate])
  return (
    <input
      ref={ref}
      type="checkbox"
      className="song-checkbox"
      aria-label="全选或取消全选"
      title="全选 / 取消全选"
      checked={checked}
      onChange={onChange}
    />
  )
}

/** 歌曲表格：复选框、序号、标题、歌手、专辑、时长、本地状态、下载状态；固定行高 + 虚拟滚动，几千首也不卡。 */
export default function SongTable({
  songs,
  matches,
  selected,
  allSelected,
  transfers,
  onShowLyrics,
  onEditFile,
  onToggle,
  onToggleAll,
}: {
  songs: Song[]
  /** 与 `songs` 一一对应的本地匹配结果；`null` 表示还没有对比。 */
  matches: LocalMatch[] | null
  /** 勾选的歌曲 ID。 */
  selected: ReadonlySet<number>
  allSelected: boolean
  /** 逐曲下载状态（按歌曲 ID）。 */
  transfers: TransferRows
  /** 双击一行或点行尾的「词」按钮时打开歌词预览。 */
  onShowLyrics: (song: Song) => void
  /** 点行尾的「编」按钮：在编辑器中打开已匹配到的本地文件。 */
  onEditFile: (path: string) => void
  onToggle: (index: number, extend: boolean) => void
  onToggleAll: () => void
}) {
  const { scrollerRef, onScroll, range, totalHeight } = useVirtualRows(songs.length, SONG_ROW_HEIGHT)
  const visible = songs.slice(range.start, range.end)

  return (
    <div className="song-table" role="table" aria-label="歌曲列表" aria-rowcount={songs.length + 1} aria-multiselectable="true">
      <div className="song-head song-grid" role="row" aria-rowindex={1}>
        <span className="song-cell song-check" role="columnheader">
          <HeaderCheckbox checked={allSelected} indeterminate={!allSelected && selected.size > 0} onChange={onToggleAll} />
        </span>
        <span className="song-cell song-index" role="columnheader">
          #
        </span>
        <span className="song-cell" role="columnheader">
          标题
        </span>
        <span className="song-cell" role="columnheader">
          歌手
        </span>
        <span className="song-cell song-album" role="columnheader">
          专辑
        </span>
        <span className="song-cell song-duration" role="columnheader">
          时长
        </span>
        <span className="song-cell song-local" role="columnheader">
          本地状态
        </span>
        <span className="song-cell song-transfer" role="columnheader">
          下载状态
        </span>
        <span className="song-cell song-action" role="columnheader" aria-label="操作" />
      </div>
      <div
        ref={scrollerRef}
        className="song-scroller"
        role="rowgroup"
        tabIndex={0}
        aria-label="歌曲滚动区域"
        onScroll={onScroll}
      >
        <div className="song-spacer" style={{ height: totalHeight }}>
          {visible.map((song, offset) => {
            const index = range.start + offset
            return (
              <SongRow
                key={`${song.id}-${index}`}
                song={song}
                index={index}
                match={matches?.[index] ?? null}
                checked={selected.has(song.id)}
                transfer={transfers.get(song.id) ?? null}
                onShowLyrics={onShowLyrics}
                onEditFile={onEditFile}
                onToggle={onToggle}
              />
            )
          })}
        </div>
      </div>
    </div>
  )
}
