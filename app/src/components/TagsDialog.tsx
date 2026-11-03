import { useEffect, useState } from 'react'
import { readTags, toTagLoadError, type TagView } from '../api/tags'
import { formatDuration, formatFileSize, formatPosition } from '../lib/format'
import Modal from './Modal'

type LoadState =
  | { status: 'loading' }
  | { status: 'ready'; tags: TagView }
  | { status: 'unreadable' }
  | { status: 'error'; message: string }

interface TagsDialogProps {
  /** 要查看的音频文件完整路径。 */
  path: string
  onClose: () => void
  /** 在对话框里重新选一个文件。 */
  onPickAnother: () => void
}

function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path
}

function Field({ label, value }: { label: string; value: string | number | null | undefined }) {
  const text = value == null || String(value).trim() === '' ? null : String(value)
  return (
    <>
      <dt>{label}</dt>
      <dd className={text == null ? 'tags-empty' : undefined} aria-label={`${label}：${text ?? '无'}`}>
        {text ?? '—'}
      </dd>
    </>
  )
}

function CoverBox({ tags }: { tags: TagView }) {
  if (tags.coverDataUrl) {
    return (
      <div className="tags-cover">
        <img src={tags.coverDataUrl} alt="封面" draggable={false} />
      </div>
    )
  }
  return (
    <div className="tags-cover tags-cover-empty" role="img" aria-label={tags.hasCover ? '封面过大，未预览' : '没有封面'}>
      {tags.hasCover ? '封面过大\n未预览' : '无封面'}
    </div>
  )
}

/** 本地音频标签查看对话框：只读，显示全部字段、封面、歌词、文件信息与网易云 ID。完整编辑器在模块 10。 */
export default function TagsDialog({ path, onClose, onPickAnother }: TagsDialogProps) {
  const [state, setState] = useState<LoadState>({ status: 'loading' })
  /** 点「重试」时加一，重新读取。 */
  const [attempt, setAttempt] = useState(0)

  useEffect(() => {
    let cancelled = false
    setState({ status: 'loading' })
    readTags(path)
      .then((tags) => {
        if (cancelled) return
        setState(tags ? { status: 'ready', tags } : { status: 'unreadable' })
      })
      .catch((error: unknown) => {
        if (!cancelled) setState({ status: 'error', message: toTagLoadError(error).message })
      })
    return () => {
      cancelled = true
    }
  }, [path, attempt])

  const tags = state.status === 'ready' ? state.tags : null
  const name = fileNameOf(path)

  return (
    <Modal
      title="音频标签"
      className="modal-wide"
      onClose={onClose}
      footer={
        <>
          <button type="button" className="btn" aria-label="选择其他音频文件" onClick={onPickAnother}>
            选择其他文件…
          </button>
          <button type="button" className="btn btn-primary" aria-label="关闭标签查看" onClick={onClose}>
            关闭
          </button>
        </>
      }
    >
      <p className="lyrics-song" title={path}>
        <span className="lyrics-song-name">{name}</span>
        {tags?.fileFormat && <span className="lyrics-song-artist">{tags.fileFormat}</span>}
      </p>

      <div className="tags-scroll" aria-busy={state.status === 'loading'}>
        {state.status === 'loading' && (
          <div className="lyrics-state" role="status" aria-live="polite">
            <span className="spinner" aria-hidden="true" />
            <p>正在读取标签…</p>
          </div>
        )}
        {state.status === 'unreadable' && (
          <div className="lyrics-state" role="alert">
            <p className="lyrics-error">无法读取该文件：格式不支持或文件已损坏。</p>
            <p className="lyrics-hint">支持 MP3 / FLAC / M4A / MP4 / OGG / OPUS / WAV / AAC。</p>
          </div>
        )}
        {state.status === 'error' && (
          <div className="lyrics-state" role="alert">
            <p className="lyrics-error">{state.message}</p>
            <button type="button" className="btn" aria-label="重新读取标签" onClick={() => setAttempt((n) => n + 1)}>
              重试
            </button>
          </div>
        )}
        {tags && (
          <>
            <div className="tags-top">
              <CoverBox tags={tags} />
              <dl className="tags-fields" aria-label="标签字段">
                <Field label="标题" value={tags.title} />
                <Field label="艺术家" value={tags.artist} />
                <Field label="专辑" value={tags.album} />
                <Field label="专辑艺术家" value={tags.albumArtist} />
                <Field label="年份" value={tags.year} />
                <Field label="音轨" value={formatPosition(tags.trackNumber, tags.trackTotal)} />
                <Field label="碟号" value={formatPosition(tags.discNumber, tags.discTotal)} />
                <Field label="流派" value={tags.genre} />
                <Field label="作曲家" value={tags.composer} />
                <Field label="备注" value={tags.comment} />
                <Field label="网易云 ID" value={tags.neteaseId} />
              </dl>
            </div>

            <h3 className="tags-heading">歌词（只读）</h3>
            {tags.lyrics ? (
              <pre className="lyrics-text tags-lyrics" tabIndex={0} aria-label="歌词内容（只读）">
                {tags.lyrics}
              </pre>
            ) : (
              <p className="tags-empty tags-no-lyrics">文件里没有歌词</p>
            )}

            <h3 className="tags-heading">文件信息</h3>
            <dl className="tags-fields tags-fileinfo" aria-label="文件信息">
              <Field label="格式" value={tags.fileFormat} />
              <Field label="时长" value={tags.durationMs > 0 ? formatDuration(tags.durationMs) : null} />
              <Field label="码率" value={tags.bitrate > 0 ? `${tags.bitrate} kbps` : null} />
              <Field label="大小" value={formatFileSize(tags.fileSize)} />
              <Field
                label="封面"
                value={
                  tags.hasCover
                    ? `${tags.coverMimeType ?? '图片'}${tags.coverSize != null ? ` · ${formatFileSize(tags.coverSize)}` : ''}`
                    : null
                }
              />
              <Field label="路径" value={tags.path} />
            </dl>
          </>
        )}
      </div>
    </Modal>
  )
}
