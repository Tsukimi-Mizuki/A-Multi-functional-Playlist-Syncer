import { memo } from 'react'
import type { Song } from '../api/playlists'
import { artistText, formatDuration } from '../lib/format'
import { useVirtualRows } from '../lib/virtual'

/** 固定行高（px）：虚拟滚动依赖它，样式里的 `--song-row-height` 必须与此一致。 */
export const SONG_ROW_HEIGHT = 48

const SongRow = memo(function SongRow({
  song,
  index,
  onShowLyrics,
}: {
  song: Song
  index: number
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
      {/* 本地状态：模块 8（本地曲库与对比）接入后在这里显示「本地已存在 / 需下载 / NCM 待解密」。 */}
      <span className="song-cell song-local" role="cell" aria-label="本地状态：尚未对比">
        —
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
  onShowLyrics,
}: {
  songs: Song[]
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
              onShowLyrics={onShowLyrics}
            />
          ))}
        </div>
      </div>
    </div>
  )
}
