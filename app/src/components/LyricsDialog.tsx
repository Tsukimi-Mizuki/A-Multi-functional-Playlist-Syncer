import { useEffect, useRef, useState } from 'react'
import { getSongLyrics, toLyricsLoadError, type LyricsMode, type LyricsPreview } from '../api/lyrics'
import type { Song } from '../api/playlists'
import { artistText } from '../lib/format'
import { Segmented } from './controls'
import Modal from './Modal'

type LoadState =
  | { status: 'loading' }
  | { status: 'ready'; preview: LyricsPreview }
  | { status: 'error'; message: string }

const MODES: { mode: LyricsMode; label: string; hint: string }[] = [
  { mode: 'line', label: '逐行', hint: '普通 LRC，每行一个时间戳' },
  { mode: 'word', label: '逐字优先', hint: '有逐字歌词时显示增强 LRC，否则回退逐行' },
]

interface LyricsDialogProps {
  song: Song
  onClose: () => void
}

/** 歌词预览对话框：只读，可在「逐行 / 逐字优先」之间切换；已取回的结果在对话框内缓存，切回去不重新请求。 */
export default function LyricsDialog({ song, onClose }: LyricsDialogProps) {
  const [mode, setMode] = useState<LyricsMode>('line')
  const [state, setState] = useState<LoadState>({ status: 'loading' })
  /** 点「重试」时加一，重新请求当前形式。 */
  const [attempt, setAttempt] = useState(0)
  const cache = useRef(new Map<LyricsMode, LyricsPreview>())

  useEffect(() => {
    const cached = cache.current.get(mode)
    if (cached) {
      setState({ status: 'ready', preview: cached })
      return
    }

    let cancelled = false
    setState({ status: 'loading' })
    getSongLyrics(song, mode)
      .then((preview) => {
        if (cancelled) return
        cache.current.set(mode, preview)
        setState({ status: 'ready', preview })
      })
      .catch((error: unknown) => {
        if (!cancelled) setState({ status: 'error', message: toLyricsLoadError(error).message })
      })
    return () => {
      cancelled = true
    }
  }, [song, mode, attempt])

  const name = song.name.trim() === '' ? '未知歌曲' : song.name
  const preview = state.status === 'ready' ? state.preview : null

  return (
    <Modal
      title="歌词预览"
      className="modal-wide"
      onClose={onClose}
      footer={
        <button type="button" className="btn" aria-label="关闭歌词预览" onClick={onClose}>
          关闭
        </button>
      }
    >
      <p className="lyrics-song" title={`${name} — ${artistText(song)}`}>
        <span className="lyrics-song-name">{name}</span>
        <span className="lyrics-song-artist">{artistText(song)}</span>
      </p>

      <div className="lyrics-toolbar">
        <Segmented
          label="歌词形式"
          value={mode}
          options={MODES.map((item) => ({ value: item.mode, label: item.label, hint: item.hint }))}
          onChange={setMode}
        />
        <ul className="lyrics-meta" aria-label="歌词信息">
          <li>来源：{preview?.source ?? '按设置查找中'}</li>
          {preview && !preview.isEmpty && (
            <>
              <li>{preview.isWordLevel ? '逐字歌词' : '逐行歌词'}</li>
              <li>{preview.hasTranslation ? '含翻译' : '无翻译'}</li>
              <li>{preview.lineCount} 行</li>
            </>
          )}
        </ul>
      </div>

      {preview && mode === 'word' && !preview.isEmpty && !preview.hasWordData && (
        <p className="lyrics-note" role="status">
          这首歌没有逐字歌词，已显示逐行歌词。
        </p>
      )}

      <div className="lyrics-body" aria-busy={state.status === 'loading'}>
        {state.status === 'loading' && (
          <div className="lyrics-state" role="status" aria-live="polite">
            <span className="spinner" aria-hidden="true" />
            <p>正在获取歌词…</p>
          </div>
        )}
        {state.status === 'error' && (
          <div className="lyrics-state" role="alert">
            <p className="lyrics-error">{state.message}</p>
            <button type="button" className="btn" aria-label="重新获取歌词" onClick={() => setAttempt((n) => n + 1)}>
              重试
            </button>
          </div>
        )}
        {preview?.isEmpty && (
          <div className="lyrics-state" role="status">
            <p>这首歌暂无歌词</p>
            <p className="lyrics-hint">可能是纯音乐，或所选来源（{preview.source}）都没有收录这首歌的歌词。</p>
          </div>
        )}
        {preview && !preview.isEmpty && (
          <pre className="lyrics-text" tabIndex={0} aria-label="歌词内容（只读）">
            {preview.text}
          </pre>
        )}
      </div>
    </Modal>
  )
}
