import { useState, type ReactNode } from 'react'
import type { PlaylistDetail, PlaylistSummary, Song } from '../api/playlists'
import { imageSrc } from '../lib/format'
import type { QualityOption } from '../api/app'
import type { useLocalMatch } from '../state/useLocalMatch'
import type { useSelection } from '../state/useSelection'
import type { useTransfer } from '../state/useTransfer'
import type { DetailState, ListState } from '../state/usePlaylists'
import LocalToolbar from './LocalToolbar'
import SongTable from './SongTable'
import TransferToolbar from './TransferToolbar'

interface PlaylistViewProps {
  loggedIn: boolean
  list: ListState
  selected: PlaylistSummary | null
  detail: DetailState
  onLogin: () => void
  onRetryList: () => void
  onRetryDetail: () => void
  onShowLyrics: (song: Song) => void
  /** 当前歌单与本地曲库的对比状态与操作。 */
  local: ReturnType<typeof useLocalMatch>
  /** 歌曲表格的多选状态。 */
  selection: ReturnType<typeof useSelection>
  /** 下载 / 同步任务的状态与操作。 */
  transfer: ReturnType<typeof useTransfer>
  /** 当前下载音质（`AudioQuality` 成员名）与可选项。 */
  quality: string
  qualityOptions: QualityOption[]
  qualityError: string | null
  onQualityChange: (quality: string) => void
}

function BigCover({ url, alt }: { url: string | null; alt: string }) {
  const [failed, setFailed] = useState(false)
  const src = imageSrc(url, 224)
  return (
    <span className="pv-cover">
      <span className="pv-cover-fallback" aria-hidden="true">
        ♪
      </span>
      {src && !failed && (
        <img src={src} alt={alt} decoding="async" draggable={false} onError={() => setFailed(true)} />
      )}
    </span>
  )
}

function Header({
  title,
  coverUrl,
  creator,
  trackText,
  description,
}: {
  title: string
  coverUrl: string | null
  creator: string | null
  trackText: string
  description: string | null
}) {
  const parts = [creator ? `创建者：${creator}` : null, trackText].filter(Boolean)
  return (
    <header className="pv-header">
      {/* key 绑定封面地址：换歌单后重置“加载失败”状态。 */}
      <BigCover key={coverUrl ?? ''} url={coverUrl} alt={`歌单封面：${title}`} />
      <div className="pv-header-text">
        <h2 className="pv-title" title={title}>
          {title}
        </h2>
        <p className="pv-meta">{parts.join(' · ')}</p>
        {description && description.trim() !== '' && (
          <p className="pv-desc" title={description}>
            {description}
          </p>
        )}
      </div>
    </header>
  )
}

function Message({
  title,
  text,
  role = 'status',
  children,
}: {
  title: string
  text?: string
  role?: 'status' | 'alert'
  children?: ReactNode
}) {
  return (
    <div className="pv-message" role={role}>
      <h3 className="pv-message-title">{title}</h3>
      {text && <p className="pv-message-text">{text}</p>}
      {children}
    </div>
  )
}

function partialNote(detail: PlaylistDetail): string | null {
  if (!detail.isPartial) return null
  const missing = Math.max(0, detail.trackCount - detail.songs.length)
  return missing > 0
    ? `有 ${missing} 首歌曲没有取回详情（可能已下架或接口暂时异常），可点击「重新加载」重试。`
    : '部分歌曲没有取回详情（可能已下架或接口暂时异常），可点击「重新加载」重试。'
}

/** 右侧主区域：歌单头部 + 歌曲表格，以及各种空 / 加载 / 错误状态。 */
export default function PlaylistView({
  loggedIn,
  list,
  selected,
  detail,
  onLogin,
  onRetryList,
  onRetryDetail,
  onShowLyrics,
  local,
  selection,
  transfer,
  quality,
  qualityOptions,
  qualityError,
  onQualityChange,
}: PlaylistViewProps) {
  if (!loggedIn) {
    return (
      <Message title="登录网易云音乐" text="扫码登录后即可浏览你的歌单，对比本地曲库并一键同步下载。">
        <button type="button" className="btn btn-primary pv-action" aria-label="扫码登录" onClick={onLogin}>
          扫码登录
        </button>
      </Message>
    )
  }

  if (list.status === 'idle' || list.status === 'loading') {
    return (
      <Message title="正在加载歌单…" text="正在读取你的歌单列表，请稍候。">
        <span className="spinner" aria-hidden="true" />
      </Message>
    )
  }

  if (list.status === 'error') {
    return (
      <Message title="歌单加载失败" text={list.message} role="alert">
        <button type="button" className="btn pv-action" aria-label="重新加载歌单" onClick={onRetryList}>
          重试
        </button>
      </Message>
    )
  }

  if (list.items.length === 0) {
    return <Message title="账号下暂无歌单" text="在网易云音乐里创建或收藏歌单后，点击侧栏的「刷新」即可看到。" />
  }

  if (!selected) {
    return <Message title="请选择左侧歌单" text="选择歌单后将显示其中的全部歌曲。" />
  }

  const name = selected.name.trim() === '' ? '未命名歌单' : selected.name

  if (detail.status === 'loading' || detail.status === 'idle') {
    const progress = detail.status === 'loading' ? detail.progress : null
    const percent = progress && progress.total > 0 ? Math.round((progress.loaded / progress.total) * 100) : null
    return (
      <>
        <Header
          title={name}
          coverUrl={selected.coverUrl}
          creator={selected.creatorNickname}
          trackText={`共 ${selected.trackCount} 首`}
          description={selected.description}
        />
        <div className="pv-loading" role="status" aria-live="polite">
          <span className="spinner" aria-hidden="true" />
          <p className="pv-message-text">
            {progress
              ? `正在加载歌单「${name}」… 已加载 ${progress.loaded}/${progress.total} 首`
              : `正在加载歌单「${name}」…`}
          </p>
          {percent !== null && (
            <progress
              className="pv-progress"
              max={100}
              value={percent}
              aria-label={`歌单加载进度 ${percent}%`}
            />
          )}
        </div>
      </>
    )
  }

  if (detail.status === 'error') {
    return (
      <>
        <Header
          title={name}
          coverUrl={selected.coverUrl}
          creator={selected.creatorNickname}
          trackText={`共 ${selected.trackCount} 首`}
          description={selected.description}
        />
        <Message title="歌单加载失败" text={detail.message} role="alert">
          <button type="button" className="btn pv-action" aria-label="重新加载歌单曲目" onClick={onRetryDetail}>
            重新加载
          </button>
        </Message>
      </>
    )
  }

  const { detail: loaded } = detail
  const note = partialNote(loaded)
  return (
    <>
      <Header
        title={loaded.name.trim() === '' ? name : loaded.name}
        coverUrl={loaded.coverUrl ?? selected.coverUrl}
        creator={loaded.creatorNickname ?? selected.creatorNickname}
        trackText={`共 ${loaded.songs.length} 首`}
        description={loaded.description ?? selected.description}
      />
      {note && (
        <div className="pv-banner" role="alert">
          <span>{note}</span>
          <button type="button" className="btn btn-small" aria-label="重新加载歌单曲目" onClick={onRetryDetail}>
            重新加载
          </button>
        </div>
      )}
      {loaded.songs.length === 0 ? (
        <Message title="歌单暂无歌曲" text="可以换一个歌单，或稍后点击「重新加载」。">
          <button type="button" className="btn pv-action" aria-label="重新加载歌单曲目" onClick={onRetryDetail}>
            重新加载
          </button>
        </Message>
      ) : (
        <>
          <LocalToolbar
            songCount={loaded.songs.length}
            outcome={local.outcome}
            busy={local.busy}
            error={local.error}
            notice={local.notice}
            onCompare={local.compare}
            onRescan={local.rescan}
            onCancel={local.cancel}
          />
          <TransferToolbar
            selectedCount={selection.count}
            onClearSelection={selection.clear}
            running={transfer.running}
            summary={transfer.summary}
            notice={transfer.notice}
            error={transfer.error}
            missingCount={local.outcome ? local.outcome.stats.missing : null}
            localBusy={local.busy !== null}
            quality={quality}
            qualityOptions={qualityOptions}
            qualityError={qualityError}
            onQualityChange={onQualityChange}
            onDownloadSelected={() =>
              transfer.downloadSelected(loaded.songs.filter((song) => selection.selected.has(song.id)).map((song) => song.id))
            }
            onDownloadMissing={transfer.downloadMissing}
            onSync={transfer.sync}
            onCancel={transfer.cancel}
            onDismiss={transfer.dismiss}
          />
          <SongTable
            songs={loaded.songs}
            matches={local.matches}
            selected={selection.selected}
            allSelected={selection.allSelected}
            transfers={transfer.rows}
            onShowLyrics={onShowLyrics}
            onToggle={selection.toggle}
            onToggleAll={selection.toggleAll}
          />
        </>
      )}
    </>
  )
}
