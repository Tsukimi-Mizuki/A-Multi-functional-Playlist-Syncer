import { memo } from 'react'
import { availabilityLabel, methodLabel, type LocalMatch } from '../api/library'
import type { Song } from '../api/playlists'
import { artistText, formatDuration } from '../lib/format'
import { useVirtualRows } from '../lib/virtual'

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

const SongRow = memo(function SongRow({
  song,
  index,
  match,
  onShowLyrics,
}: {
  song: Song
  index: number
  /** 本地匹配结果；`null` 表示还没有对比。 */
  match: LocalMatch | null
  onShowLyrics: (song: Song) => void
}) {
  const name = song.name.trim() === '' ? '未知歌曲' : song.name
  const artists = artistText(song)
  const album = song.album.name.trim() === '' ? '—' : song.album.name
  return (
    <div
      className="song-row song-grid"
      role="row"
      aria-rowindex={index + 2}
      style={{ transform: `translateY(${index * SONG_ROW_HEIGHT}px)` }}
      onDoubleClick={() => onShowLyrics(song)}
    >
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
      <span className="song-cell song-secondary" role="cell" title={album}>
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
      <span className="song-cell song-action" role="cell">
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

/** 歌曲表格：序号、标题、歌手、专辑、时长、本地状态；固定行高 + 虚拟滚动，几千首也不卡。 */
export default function SongTable({
  songs,
  matches,
  onShowLyrics,
}: {
  songs: Song[]
  /** 与 `songs` 一一对应的本地匹配结果；`null` 表示还没有对比。 */
  matches: LocalMatch[] | null
  /** 双击一行或点行尾的「词」按钮时打开歌词预览。 */
  onShowLyrics: (song: Song) => void
}) {
  const { scrollerRef, onScroll, range, totalHeight } = useVirtualRows(songs.length, SONG_ROW_HEIGHT)
  const visible = songs.slice(range.start, range.end)

  return (
    <div className="song-table" role="table" aria-label="歌曲列表" aria-rowcount={songs.length + 1}>
      <div className="song-head song-grid" role="row" aria-rowindex={1}>
        <span className="song-cell song-index" role="columnheader">
          #
        </span>
        <span className="song-cell" role="columnheader">
          标题
        </span>
        <span className="song-cell" role="columnheader">
          歌手
        </span>
        <span className="song-cell" role="columnheader">
          专辑
        </span>
        <span className="song-cell song-duration" role="columnheader">
          时长
        </span>
        <span className="song-cell song-local" role="columnheader">
          本地状态
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
          {visible.map((song, offset) => (
            <SongRow
              key={`${song.id}-${range.start + offset}`}
              song={song}
              index={range.start + offset}
              match={matches?.[range.start + offset] ?? null}
              onShowLyrics={onShowLyrics}
            />
          ))}
        </div>
      </div>
    </div>
  )
}
