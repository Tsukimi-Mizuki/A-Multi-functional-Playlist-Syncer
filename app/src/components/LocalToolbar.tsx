import type { MatchOutcome } from '../api/library'
import type { LocalBusy, LocalError } from '../state/useLocalMatch'

interface LocalToolbarProps {
  /** 歌单歌曲数（还没有对比结果时用来显示「共 N 首」）。 */
  songCount: number
  outcome: MatchOutcome | null
  busy: LocalBusy | null
  error: LocalError | null
  notice: string | null
  onCompare: () => void
  onRescan: () => void
  onCancel: () => void
}

function busyText(busy: LocalBusy): string {
  const progress = busy.progress
  if (progress === null || progress.total === 0) return '正在对比本地曲库…'
  return `正在扫描本地曲库 ${progress.scanned}/${progress.total}`
}

/** 歌单头部下方的本地对比工具条：统计、「对比本地」「重新扫描」、扫描进度与取消。 */
export default function LocalToolbar({ songCount, outcome, busy, error, notice, onCompare, onRescan, onCancel }: LocalToolbarProps) {
  const percent =
    busy?.progress && busy.progress.total > 0 ? Math.min(100, Math.round((busy.progress.scanned / busy.progress.total) * 100)) : null

  return (
    <div className="local-toolbar" role="group" aria-label="本地曲库对比">
      <div className="local-summary" role="status" aria-live="polite">
        {busy ? (
          <>
            <span className="spinner spinner-small" aria-hidden="true" />
            <span className="local-busy-text">{busyText(busy)}</span>
            {percent !== null && (
              <progress className="local-progress" max={100} value={percent} aria-label={`本地曲库扫描进度 ${percent}%`} />
            )}
            {busy.progress && busy.progress.fileName !== '' && (
              <span className="local-file" title={busy.progress.fileName}>
                {busy.progress.fileName}
              </span>
            )}
          </>
        ) : outcome ? (
          <>
            <span className="local-chip local-chip-available" aria-label={`本地已存在 ${outcome.stats.available} 首`}>
              本地已存在 {outcome.stats.available}
            </span>
            <span className="local-chip local-chip-ncm" aria-label={`NCM 待解密 ${outcome.stats.ncm} 首`}>
              NCM 待解密 {outcome.stats.ncm}
            </span>
            <span className="local-chip local-chip-missing" aria-label={`需下载 ${outcome.stats.missing} 首`}>
              需下载 {outcome.stats.missing}
            </span>
            <span
              className="local-library"
              title={`曲库目录：${outcome.libraryDirectory}`}
              aria-label={`曲库共 ${outcome.totalFiles} 个文件，其中 NCM ${outcome.ncmFiles} 个`}
            >
              曲库 {outcome.totalFiles} 个文件{outcome.ncmFiles > 0 ? `（NCM ${outcome.ncmFiles}）` : ''}
            </span>
          </>
        ) : error ? (
          <span className="local-error" role="alert">
            {error.message}
          </span>
        ) : (
          <span className="local-idle">尚未对比本地曲库（共 {songCount} 首）</span>
        )}
        {!busy && notice && <span className="local-notice">{notice}</span>}
        {!busy && outcome && error && (
          <span className="local-error" role="alert">
            {error.message}
          </span>
        )}
      </div>
      <div className="local-actions">
        {busy ? (
          <button type="button" className="btn btn-small" aria-label="取消扫描本地曲库" onClick={onCancel}>
            取消
          </button>
        ) : (
          <>
            <button type="button" className="btn btn-small" aria-label="对比本地曲库" title="扫描本地曲库（已扫描过则直接复用）并与歌单对比" onClick={onCompare}>
              对比本地
            </button>
            <button type="button" className="btn btn-small" aria-label="重新扫描本地曲库" title="重新扫描曲库目录，然后对比" onClick={onRescan}>
              重新扫描
            </button>
          </>
        )}
      </div>
    </div>
  )
}
