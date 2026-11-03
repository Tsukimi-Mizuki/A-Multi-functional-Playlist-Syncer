import type { QualityOption } from '../api/app'
import type { TransferKind, TransferSummary } from '../api/transfer'
import type { RunningTransfer } from '../state/useTransfer'

interface TransferToolbarProps {
  /** 勾选的歌曲数。 */
  selectedCount: number
  onClearSelection: () => void
  /** 正在运行的任务；没有为 `null`。 */
  running: RunningTransfer | null
  /** 上一个任务的汇总。 */
  summary: TransferSummary | null
  /** 没有勾选 / 没有缺失之类不算错误的提示。 */
  notice: string | null
  error: string | null
  /** 本地曲库对比里「需下载」的数量；还没有对比为 `null`。 */
  missingCount: number | null
  /** 正在扫描 / 对比本地曲库：这时不开始新任务。 */
  localBusy: boolean
  quality: string
  qualityOptions: QualityOption[]
  qualityError: string | null
  onQualityChange: (quality: string) => void
  onDownloadSelected: () => void
  onDownloadMissing: () => void
  onSync: () => void
  onCancel: () => void
  onDismiss: () => void
}

function verb(kind: TransferKind): string {
  return kind === 'sync' ? '同步' : '下载'
}

/** 运行中的一行说明：`正在同步 3/120`；总数还不知道时只说在准备。 */
export function runningText(running: RunningTransfer): string {
  if (running.cancelling) return '正在取消…（等当前歌曲收尾，已下载的文件会保留）'
  if (running.total === 0) return `正在准备${verb(running.kind)}…`
  return `正在${verb(running.kind)} ${running.completed}/${running.total}`
}

/** 任务结束后的汇总文案（与桌面版日志一致）。 */
export function summaryText(summary: TransferSummary): string {
  const done = summary.results.length
  if (summary.kind === 'sync') {
    const body = `成功 ${summary.succeeded + summary.skipped} 首（下载 ${summary.downloaded}、NCM 解密 ${summary.decrypted}、跳过 ${summary.skipped}），失败 ${summary.failed} 首`
    return summary.cancelled ? `同步已取消：已处理 ${done}/${summary.total}，${body}` : `同步完成：${body}`
  }
  const body = `成功 ${summary.succeeded} 首，跳过 ${summary.skipped} 首${summary.failed > 0 ? `，失败 ${summary.failed} 首` : ''}`
  return summary.cancelled ? `下载已取消：已处理 ${done}/${summary.total}，${body}` : `下载完成：${body}`
}

/** 歌单头部下方的下载 / 同步工具条：音质、三个任务按钮、取消、整体进度与结果汇总。 */
export default function TransferToolbar({
  selectedCount,
  onClearSelection,
  running,
  summary,
  notice,
  error,
  missingCount,
  localBusy,
  quality,
  qualityOptions,
  qualityError,
  onQualityChange,
  onDownloadSelected,
  onDownloadMissing,
  onSync,
  onCancel,
  onDismiss,
}: TransferToolbarProps) {
  const busy = running !== null
  const percent = running && running.total > 0 ? Math.min(100, Math.round((running.completed / running.total) * 100)) : null
  const noMissing = missingCount === 0
  const failedTone = summary !== null && summary.failed > 0

  return (
    <div className="transfer-toolbar" role="group" aria-label="下载与同步">
      <div className="transfer-row">
        <label className="transfer-quality">
          <span className="transfer-quality-label">音质</span>
          <select
            className="transfer-select"
            aria-label="下载音质"
            title="下载使用的音质；服务端没有对应权限时会自动降级，并在列表里提示实际音质"
            value={quality}
            disabled={busy}
            onChange={(event) => onQualityChange(event.target.value)}
          >
            {qualityOptions.length === 0 && <option value={quality}>{quality}</option>}
            {qualityOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <button
          type="button"
          className="btn btn-small"
          aria-label="下载选中的歌曲"
          title={selectedCount === 0 ? '先在列表里勾选歌曲（按住 Shift 可以连选一段）' : `下载勾选的 ${selectedCount} 首歌曲`}
          disabled={busy || localBusy || selectedCount === 0}
          onClick={onDownloadSelected}
        >
          下载选中{selectedCount > 0 ? `（${selectedCount}）` : ''}
        </button>
        <button
          type="button"
          className="btn btn-small"
          aria-label="下载本地缺失的歌曲"
          title={noMissing ? '本地曲库里没有缺失的歌曲' : '下载本地曲库里没有的歌曲（待解密的 NCM 不算缺失，用「一键同步」解密）'}
          disabled={busy || localBusy || noMissing}
          onClick={onDownloadMissing}
        >
          下载缺失
        </button>
        <button
          type="button"
          className="btn btn-small btn-primary"
          aria-label="一键同步整个歌单"
          title="已存在的跳过，NCM 解密，缺失的下载，并写入标签、封面与歌词"
          disabled={busy || localBusy}
          onClick={onSync}
        >
          一键同步
        </button>
        {busy && (
          <button
            type="button"
            className="btn btn-small"
            aria-label="取消下载或同步"
            disabled={running.cancelling}
            onClick={onCancel}
          >
            取消
          </button>
        )}
        {selectedCount > 0 && !busy && (
          <span className="transfer-selected">
            已选 {selectedCount} 首
            <button type="button" className="transfer-link" aria-label="清除选择" onClick={onClearSelection}>
              清除
            </button>
          </span>
        )}
      </div>

      {running && (
        <div className="transfer-row transfer-progress-row" role="status" aria-live="polite">
          <span className="spinner spinner-small" aria-hidden="true" />
          <span className="transfer-running-text">{runningText(running)}</span>
          {percent !== null && !running.cancelling && (
            <progress className="transfer-progress" max={100} value={percent} aria-label={`${verb(running.kind)}进度 ${percent}%`} />
          )}
        </div>
      )}

      {!running && summary && (
        <div className="transfer-row" role="status" aria-live="polite">
          <span className={failedTone ? 'transfer-summary transfer-summary-warn' : 'transfer-summary'}>{summaryText(summary)}</span>
          {summary.onlineMatchSkipped && (
            <span className="transfer-note">在线补全标签与歌词将在后续版本提供，本次已跳过。</span>
          )}
          <button type="button" className="transfer-link" aria-label="关闭结果提示" onClick={onDismiss}>
            知道了
          </button>
        </div>
      )}
      {!running && notice && (
        <div className="transfer-row" role="status">
          <span className="transfer-note">{notice}</span>
        </div>
      )}
      {!running && error && (
        <div className="transfer-row" role="alert">
          <span className="transfer-error">{error}</span>
          <button type="button" className="transfer-link" aria-label="关闭错误提示" onClick={onDismiss}>
            知道了
          </button>
        </div>
      )}
      {qualityError && (
        <div className="transfer-row" role="alert">
          <span className="transfer-error">{qualityError}</span>
        </div>
      )}
    </div>
  )
}
