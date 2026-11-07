import type { PendingTransfer } from '../state/useTransferConfirm'
import Modal from './Modal'

interface SyncConfirmDialogProps {
  pending: PendingTransfer
  /** 当前下载音质的显示名。 */
  qualityLabel: string
  onConfirm: () => void
  onCancel: () => void
}

/** 「一键同步」的二次确认：按本地对比结果说明会跳过、解密、下载各多少首，以及音质与下载目录。 */
export default function SyncConfirmDialog({ pending, qualityLabel, onConfirm, onCancel }: SyncConfirmDialogProps) {
  const { plan } = pending
  const duplicates = pending.rowCount - plan.total
  const rows: Array<{ label: string; count: number }> = [
    {
      label: pending.overwriteExisting ? '本地已存在，重新下载（设置里开了「覆盖已存在文件」）' : '本地已存在，跳过',
      count: plan.available,
    },
    { label: 'NCM 解密', count: plan.ncm },
    { label: '需要下载', count: plan.missing.length },
  ]

  return (
    <Modal
      title="一键同步"
      className="transfer-confirm"
      onClose={onCancel}
      footer={
        <>
          <button type="button" className="btn btn-primary" aria-label="开始同步整个歌单" onClick={onConfirm}>
            开始同步
          </button>
          <button type="button" className="btn" aria-label="取消同步" onClick={onCancel}>
            取消
          </button>
        </>
      }
    >
      <p className="confirm-message">
        歌单「{pending.playlistName}」共 {plan.total} 首：
      </p>
      <ul className="transfer-confirm-stats" aria-label="同步计划">
        {rows.map((row) => (
          <li key={row.label} className="transfer-confirm-stat">
            <span>{row.label}</span>
            <span className="transfer-confirm-stat-count">{row.count} 首</span>
          </li>
        ))}
      </ul>
      {duplicates > 0 && <p className="transfer-confirm-note">歌单里有 {duplicates} 首重复的歌曲，只处理一次。</p>}
      {pending.libraryNote && (
        <p className="transfer-confirm-warn" role="note">
          {pending.libraryNote}
        </p>
      )}
      <dl className="transfer-confirm-meta">
        <dt>音质</dt>
        <dd>{qualityLabel}</dd>
        {pending.downloadDirectory && (
          <>
            <dt>下载到</dt>
            <dd className="transfer-confirm-path" title={pending.downloadDirectory}>
              {pending.downloadDirectory}
            </dd>
          </>
        )}
      </dl>
    </Modal>
  )
}
