import { memo } from 'react'
import type { Song } from '../api/playlists'
import { artistText, formatDuration } from '../lib/format'
import { useVirtualRows } from '../lib/virtual'
import { useSelection } from '../state/useSelection'
import type { PendingTransfer } from '../state/useTransferConfirm'
import Modal from './Modal'
import { HeaderCheckbox } from './SongTable'

/** 固定行高（px）：虚拟滚动依赖它，样式里的 `--missing-row-height` 必须与此一致。 */
const MISSING_ROW_HEIGHT = 36

const MissingRow = memo(function MissingRow({
  song,
  index,
  checked,
  onToggle,
}: {
  song: Song
  index: number
  checked: boolean
  /** 第二个参数是点击时是否按着 Shift。 */
  onToggle: (index: number, extend: boolean) => void
}) {
  const name = song.name.trim() === '' ? '未知歌曲' : song.name
  const artists = artistText(song)
  const album = song.album.name.trim() === '' ? '—' : song.album.name
  return (
    <div
      className={checked ? 'missing-row missing-grid missing-row-checked' : 'missing-row missing-grid'}
      role="row"
      aria-rowindex={index + 2}
      aria-selected={checked}
      style={{ transform: `translateY(${index * MISSING_ROW_HEIGHT}px)` }}
      onClick={(event) => onToggle(index, event.shiftKey)}
    >
      <span className="missing-cell missing-check" role="cell">
        <input
          type="checkbox"
          className="song-checkbox"
          aria-label={`下载：${name}`}
          checked={checked}
          onChange={() => undefined}
          onClick={(event) => {
            // 行本身也能点：别让这次点击再冒泡到行上切换第二次。
            event.stopPropagation()
            onToggle(index, event.shiftKey)
          }}
        />
      </span>
      <span className="missing-cell missing-index" role="cell">
        {index + 1}
      </span>
      <span className="missing-cell missing-title" role="cell" title={song.alias ? `${name}（${song.alias}）` : name}>
        {name}
      </span>
      <span className="missing-cell missing-secondary" role="cell" title={artists}>
        {artists}
      </span>
      <span className="missing-cell missing-secondary" role="cell" title={album}>
        {album}
      </span>
      <span className="missing-cell missing-duration" role="cell">
        {formatDuration(song.durationMs)}
      </span>
    </div>
  )
})

interface DownloadMissingDialogProps {
  pending: PendingTransfer
  /** 当前下载音质的显示名。 */
  qualityLabel: string
  /** 确认下载：仍勾选的歌曲 ID（按歌单顺序）。 */
  onConfirm: (ids: number[]) => void
  onCancel: () => void
}

/**
 * 「下载缺失」的二次确认：列出本地曲库里没有的歌曲（默认全选，固定行高 + 虚拟滚动），
 * 取消勾选的这次不下载；点行或复选框切换，按住 Shift 连选一段。
 */
export default function DownloadMissingDialog({ pending, qualityLabel, onConfirm, onCancel }: DownloadMissingDialogProps) {
  const songs = pending.plan.missing
  const selection = useSelection(songs, true)
  const { scrollerRef, onScroll, range, totalHeight } = useVirtualRows(songs.length, MISSING_ROW_HEIGHT)
  const visible = songs.slice(range.start, range.end)
  const count = selection.count

  return (
    <Modal
      title="下载缺失的歌曲"
      className="transfer-confirm transfer-confirm-wide"
      onClose={onCancel}
      footer={
        <>
          <span className="transfer-confirm-count">
            已选 {count}/{songs.length} 首
          </span>
          <button
            type="button"
            className="btn btn-primary"
            aria-label={`下载勾选的 ${count} 首歌曲`}
            disabled={count === 0}
            onClick={() => onConfirm(songs.filter((song) => selection.selected.has(song.id)).map((song) => song.id))}
          >
            下载 {count} 首
          </button>
          <button type="button" className="btn" aria-label="取消下载缺失的歌曲" onClick={onCancel}>
            取消
          </button>
        </>
      }
    >
      <p className="confirm-message">
        歌单「{pending.playlistName}」里有 {songs.length} 首歌曲本地曲库中没有，勾选的歌曲将以「{qualityLabel}」音质下载
        {pending.downloadDirectory ? '到：' : '。'}
      </p>
      {pending.downloadDirectory && (
        <p className="transfer-confirm-path" title={pending.downloadDirectory}>
          {pending.downloadDirectory}
        </p>
      )}
      {pending.libraryNote && (
        <p className="transfer-confirm-warn" role="note">
          {pending.libraryNote}
        </p>
      )}
      <div className="missing-table" role="table" aria-label="缺失的歌曲" aria-rowcount={songs.length + 1} aria-multiselectable="true">
        <div className="missing-head missing-grid" role="row" aria-rowindex={1}>
          <span className="missing-cell missing-check" role="columnheader">
            <HeaderCheckbox
              checked={selection.allSelected}
              indeterminate={!selection.allSelected && count > 0}
              onChange={selection.toggleAll}
            />
          </span>
          <span className="missing-cell missing-index" role="columnheader">
            #
          </span>
          <span className="missing-cell" role="columnheader">
            标题
          </span>
          <span className="missing-cell" role="columnheader">
            歌手
          </span>
          <span className="missing-cell" role="columnheader">
            专辑
          </span>
          <span className="missing-cell missing-duration" role="columnheader">
            时长
          </span>
        </div>
        <div
          ref={scrollerRef}
          className="missing-scroller"
          role="rowgroup"
          tabIndex={0}
          aria-label="缺失歌曲滚动区域"
          onScroll={onScroll}
        >
          <div className="missing-spacer" style={{ height: totalHeight }}>
            {visible.map((song, offset) => {
              const index = range.start + offset
              return (
                <MissingRow
                  key={song.id}
                  song={song}
                  index={index}
                  checked={selection.selected.has(song.id)}
                  onToggle={selection.toggle}
                />
              )
            })}
          </div>
        </div>
      </div>
      <p className="transfer-confirm-note">
        点行或复选框切换，按住 Shift 可以连选一段。
        {pending.plan.ncm > 0 && `另有 ${pending.plan.ncm} 首是待解密的 NCM，不在此列，可以用「一键同步」解密。`}
      </p>
    </Modal>
  )
}
