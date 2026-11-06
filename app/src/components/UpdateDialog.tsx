import type { UpdateCheck } from '../api/update'
import Modal from './Modal'

interface UpdateDialogProps {
  check: UpdateCheck
  /** 打开发布页失败的原因。 */
  error: string | null
  onDownload: () => void
  onIgnore: () => void
  /** 「稍后提醒」，Esc 与点遮罩也算。 */
  onLater: () => void
}

/** 发现新版本的提醒：版本、发布日期与更新说明，以及跳转下载 / 忽略 / 稍后提醒三个选择。 */
export default function UpdateDialog({ check, error, onDownload, onIgnore, onLater }: UpdateDialogProps) {
  const { release } = check
  return (
    <Modal
      title="发现新版本"
      className="update-dialog"
      onClose={onLater}
      footer={
        <>
          <button type="button" className="btn btn-primary" aria-label="跳转至下载页面" onClick={onDownload}>
            跳转至下载页面
          </button>
          <button type="button" className="btn" aria-label={`忽略版本 ${release.version}`} onClick={onIgnore}>
            忽略本次更新
          </button>
          <button type="button" className="btn" aria-label="稍后提醒" onClick={onLater}>
            稍后提醒
          </button>
        </>
      }
    >
      <div className="update-heading">
        <span className="update-version">Lunatic Princess {release.version}</span>
        <span className="update-meta">
          当前 {check.currentVersion}
          {release.publishedOn && ` · ${release.publishedOn} 发布`}
        </span>
      </div>
      {check.status === 'ignored' && (
        <p className="update-note" role="status">
          你之前选择过忽略这个版本。
        </p>
      )}
      {release.notes ? (
        <div className="update-notes" tabIndex={0} aria-label="更新说明">
          {release.notes}
        </div>
      ) : (
        <p className="update-notes-empty">这个版本没有填写更新说明，详情见下载页面。</p>
      )}
      <p className="update-hint">忽略后不再提醒这个版本，出了更新的版本照常提醒；稍后提醒会在下次启动时再提醒。</p>
      {error && (
        <p className="update-error" role="alert">
          {error}
        </p>
      )}
    </Modal>
  )
}
