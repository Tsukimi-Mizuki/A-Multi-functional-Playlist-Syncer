import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from 'react'
import type { FileContext, LoadedTags } from '../api/online'
import { useOnlineTags, type CoverSizeState, type OnlineTagRow, type TagParts } from '../state/useOnlineTags'
import Modal from './Modal'

interface OnlineTagsDialogProps {
  /** 这首歌现在的标题 / 艺术家 / 专辑 / 时长（给候选打分用）。 */
  context: FileContext
  /** 打开时自动搜索的关键词。 */
  initialKeyword: string
  /** 用户选好一条并按了「确定」：把取回的标签 / 封面交给编辑器（`parts` 是勾选要套用的部分）。 */
  onApply: (loaded: LoadedTags, parts: TagParts) => void
  onClose: () => void
}

/** 封面缩略图；没有地址或加载失败时显示占位。 */
function Thumbnail({ url }: { url: string | null }) {
  const [failed, setFailed] = useState(false)
  return (
    <span className="otags-cover" aria-hidden="true">
      {url && !failed ? (
        <img src={url} alt="" loading="lazy" draggable={false} referrerPolicy="no-referrer" onError={() => setFailed(true)} />
      ) : (
        '♪'
      )}
    </span>
  )
}

/** 封面尺寸一栏的文字。 */
function sizeText(hasCover: boolean, size: CoverSizeState | undefined): string {
  if (!hasCover) return '无封面'
  if (size === undefined || size === 'pending') return '读取尺寸…'
  if (size === 'unknown') return '尺寸未知'
  return `${size.width}×${size.height}`
}

/** 一行候选：封面、来源一栏（来源 / 封面尺寸 / 年份 / 音轨 / 风格）、标题、艺术家、专辑、时长与匹配度。 */
function CandidateRow({
  row,
  size,
  selected,
  onSelect,
  onConfirm,
}: {
  row: OnlineTagRow
  size: CoverSizeState | undefined
  selected: boolean
  onSelect: () => void
  onConfirm: () => void
}) {
  const { song, score, belowThreshold } = row.candidate
  const cover = sizeText(song.coverUrl != null, size)
  const facts = [
    cover,
    song.year != null ? String(song.year) : null,
    song.trackNumber != null ? `音轨 ${song.trackNumber}` : null,
    song.discNumber != null && song.discNumber > 1 ? `碟 ${song.discNumber}` : null,
    song.genre?.trim() || null,
  ].filter((fact): fact is string => fact != null)
  const match = score ? `${Math.round(score.total * 100)}%` : ''
  const artist = song.artist.trim() === '' ? '未知歌手' : song.artist
  const album = song.album.trim() === '' ? '未知专辑' : song.album
  return (
    <div
      role="option"
      id={`otags-${row.key}`}
      aria-selected={selected}
      aria-label={`${song.sourceLabel}：${song.displayTitle}，${artist}，${album}，封面${cover}${match ? `，匹配度 ${match}` : ''}`}
      className={selected ? 'otags-row otags-row-selected' : 'otags-row'}
      onClick={onSelect}
      onDoubleClick={onConfirm}
    >
      <Thumbnail url={row.candidate.thumbnailUrl} />
      <span className="otags-source">
        <span className="otags-source-name">{song.sourceLabel}</span>
        {facts.map((fact) => (
          <span key={fact}>{fact}</span>
        ))}
      </span>
      <span className="otags-text otags-title">{song.displayTitle}</span>
      <span className="otags-text">{artist}</span>
      <span className="otags-text">{album}</span>
      <span className="otags-match">
        <span>{song.durationText}</span>
        {match !== '' && (
          <span
            className={belowThreshold ? 'otags-low' : undefined}
            title={belowThreshold ? '匹配度低于设置里的阈值，可能不是同一首歌' : undefined}
          >
            匹配 {match}
          </span>
        )}
      </span>
    </div>
  )
}

/**
 * 在线标签（歌词与标签编辑器「标签信息」右上角的按钮打开）：同一个关键词在网易云与 QQ 音乐各搜一次，
 * 一行一条候选，列出封面与标签；选一条按「确定」把标签 / 封面填进编辑器的表单。只找标签与封面，不取歌词。
 */
export default function OnlineTagsDialog({ context, initialKeyword, onApply, onClose }: OnlineTagsDialogProps) {
  const tags = useOnlineTags(context, initialKeyword)
  const { rows, selectedKey, setSelectedKey, searching, loading } = tags
  const [parts, setParts] = useState<TagParts>({ tags: true, cover: true })
  const listRef = useRef<HTMLDivElement>(null)

  const selectedIndex = rows.findIndex((row) => row.key === selectedKey)
  const canConfirm = selectedIndex >= 0 && !searching && !loading && (parts.tags || parts.cover)

  const confirm = useCallback(async () => {
    if (!canConfirm) return
    const loaded = await tags.load(parts)
    if (!loaded) return
    onApply(loaded, parts)
    onClose()
  }, [canConfirm, tags, parts, onApply, onClose])

  // 键盘换行后把选中的那一行滚进可视范围。
  useEffect(() => {
    if (selectedKey == null) return
    listRef.current?.querySelector('.otags-row-selected')?.scrollIntoView({ block: 'nearest' })
  }, [selectedKey])

  const onListKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (rows.length === 0) return
    if (event.key === 'Enter') {
      event.preventDefault()
      void confirm()
      return
    }
    const last = rows.length - 1
    const from = selectedIndex < 0 ? 0 : selectedIndex
    const next =
      event.key === 'ArrowDown'
        ? Math.min(last, selectedIndex < 0 ? 0 : from + 1)
        : event.key === 'ArrowUp'
          ? Math.max(0, from - 1)
          : event.key === 'Home'
            ? 0
            : event.key === 'End'
              ? last
              : null
    if (next == null) return
    event.preventDefault()
    setSelectedKey(rows[next]?.key ?? null)
  }

  return (
    <Modal
      title="在线标签"
      className="otags-dialog"
      onClose={onClose}
      footer={
        <>
          <div className="otags-parts" role="group" aria-label="要套用的内容">
            <span className="otags-parts-title">套用</span>
            <label className="ncm-check">
              <input
                type="checkbox"
                aria-label="套用标签"
                checked={parts.tags}
                disabled={loading}
                onChange={(event) => setParts((previous) => ({ ...previous, tags: event.target.checked }))}
              />
              标签
            </label>
            <label className="ncm-check">
              <input
                type="checkbox"
                aria-label="套用封面"
                checked={parts.cover}
                disabled={loading}
                onChange={(event) => setParts((previous) => ({ ...previous, cover: event.target.checked }))}
              />
              封面
            </label>
            <span className="otags-parts-note">歌词不会改动；确定后填进编辑器，保存才写入文件</span>
          </div>
          <button
            type="button"
            className="btn btn-primary"
            aria-label="套用选中的在线标签"
            disabled={!canConfirm}
            onClick={() => void confirm()}
          >
            {loading ? '获取中…' : '确定'}
          </button>
          <button type="button" className="btn" aria-label="关闭在线标签" onClick={onClose}>
            取消
          </button>
        </>
      }
    >
      <div className="otags-search">
        <input
          type="text"
          className="editor-input otags-keyword"
          aria-label="在线标签搜索关键词"
          spellCheck={false}
          placeholder="歌曲、歌手或专辑"
          value={tags.keyword}
          onChange={(event) => tags.setKeyword(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !searching && !loading) void tags.search(tags.keyword)
          }}
        />
        <button
          type="button"
          className="btn btn-primary"
          aria-label="搜索在线标签"
          disabled={searching || loading}
          onClick={() => void tags.search(tags.keyword)}
        >
          {searching ? '搜索中…' : '搜索'}
        </button>
      </div>
      <p className="otags-status" role="status" aria-live="polite">
        {tags.status}
        {tags.failures.length > 0 && <span className="otags-warn"> 部分来源没有搜到：{tags.failures.join('；')}</span>}
      </p>
      <div
        ref={listRef}
        className="otags-table"
        role="listbox"
        aria-label="在线标签候选"
        aria-activedescendant={selectedKey != null ? `otags-${selectedKey}` : undefined}
        tabIndex={0}
        onKeyDown={onListKeyDown}
      >
        <div className="otags-head" aria-hidden="true">
          <span />
          <span className="otags-head-source">源</span>
          <span>标题</span>
          <span>艺术家</span>
          <span>专辑</span>
          <span className="otags-head-match">时长 / 匹配</span>
        </div>
        {rows.map((row) => (
          <CandidateRow
            key={row.key}
            row={row}
            size={tags.sizes[row.key]}
            selected={row.key === selectedKey}
            onSelect={() => setSelectedKey(row.key)}
            onConfirm={() => void confirm()}
          />
        ))}
        {rows.length === 0 && (
          <p className="otags-empty">{searching ? '正在搜索…' : '没有候选。改一下上面的关键词再搜索。'}</p>
        )}
      </div>
    </Modal>
  )
}
