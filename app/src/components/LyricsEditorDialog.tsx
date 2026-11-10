import { useCallback, useEffect, useRef, useState } from 'react'
import { fileNameOf, directoryOf } from '../api/editor'
import { LYRICS_SOURCE_LABELS, type BatchFile, type FileContext, type LyricsSourceKind } from '../api/online'
import {
  describeEntry,
  fileInfoText,
  onlineTagsQuery,
  useLyricsEditor,
  type CoverMeta,
  type EditorFields,
} from '../state/useLyricsEditor'
import BatchTagsDialog, { type BatchDrop } from './BatchTagsDialog'
import ConfirmDialog from './ConfirmDialog'
import Modal from './Modal'
import OnlineTagsDialog from './OnlineTagsDialog'

/** 外部（拖放）要求编辑器载入文件，或提示无法载入的原因。`seq` 每次加一，相同路径也会重新触发。 */
export interface EditorRequest {
  seq: number
  /** 拖入了一首：要载入的文件。 */
  path: string | null
  /** 拖入了多首（文件夹已展开）：交给批量修改标签。 */
  files: BatchFile[] | null
  /** 没有可载入的文件时给用户看的原因（NCM、不支持的格式等）。 */
  message: string | null
}

interface LyricsEditorDialogProps {
  /** 打开时直接载入的文件（工具栏选择、歌曲行的「编辑」、`--edit` 启动参数）。 */
  initialPath: string | null
  /** 编辑器打开期间到来的拖放请求。 */
  request: EditorRequest | null
  /** 从批量修改标签窗口里打开（只编辑这一首）：不显示「打开音频文件」与「批量修改」。 */
  embedded?: boolean
  onClose: () => void
}

const SOURCES: LyricsSourceKind[] = ['Netease', 'QQMusic', 'Kugou', 'Lrclib']

/** 封面旁的参数：格式、像素尺寸、大小、图片类型，一行一项；拿不到的项不显示。 */
function CoverMetaList({ meta }: { meta: CoverMeta }) {
  const items = [
    { label: '格式', value: meta.mimeType },
    { label: '尺寸', value: meta.dimensions },
    { label: '大小', value: meta.size },
    { label: '类型', value: meta.pictureType },
  ].filter((item): item is { label: string; value: string } => item.value != null)
  return (
    <ul className="editor-cover-meta" aria-label="封面参数">
      {items.map((item) => (
        <li key={item.label} title={item.label} aria-label={`封面${item.label}：${item.value}`}>
          {item.value}
        </li>
      ))}
    </ul>
  )
}

/** 带标签的文本框；`short` 用于年份 / 音轨 / 碟号。 */
function TextField({
  label,
  name,
  value,
  onChange,
  placeholder,
}: {
  label: string
  name: keyof EditorFields
  value: string
  onChange: (name: keyof EditorFields, value: string) => void
  placeholder?: string
}) {
  return (
    <label className="editor-field">
      <span className="editor-field-label">{label}</span>
      <input
        type="text"
        className="editor-input"
        aria-label={label}
        spellCheck={false}
        placeholder={placeholder}
        value={value}
        onChange={(event) => onChange(name, event.target.value)}
      />
    </label>
  )
}

/**
 * 歌词与标签编辑器（主窗口内的大型模态面板，对应 C# 的 `LyricsEditorWindow`）。
 * 选用模态面板而不是独立 Tauri 窗口：拖放事件、IPC、主题与未保存确认都在同一个 WebView 里，
 * 不需要额外的 capabilities、无边框标题栏与窗口材质处理。
 */
export default function LyricsEditorDialog({ initialPath, request, embedded = false, onClose }: LyricsEditorDialogProps) {
  const editor = useLyricsEditor()
  const { config, file, fields, busy, dirty } = editor
  const [batchOpen, setBatchOpen] = useState(false)
  // 拖入多首时交给批量窗口的清单（从「批量修改」按钮打开时为 null，按目录扫描）。
  const [batchDrop, setBatchDrop] = useState<BatchDrop | null>(null)
  // 「在线标签」窗口：打开那一刻的关键词与打分用的歌曲信息（之后表单再变也不影响这一次）。
  const [onlineTags, setOnlineTags] = useState<{ keyword: string; context: FileContext } | null>(null)
  const [confirm, setConfirm] = useState<{ message: string; confirmLabel: string; run: () => void } | null>(null)
  const dirtyRef = useRef(dirty)
  const busyRef = useRef(busy)
  const batchOpenRef = useRef(batchOpen)
  useEffect(() => {
    dirtyRef.current = dirty
    busyRef.current = busy
    batchOpenRef.current = batchOpen
  })

  /** 有未保存的修改时先确认，再执行 `action`。 */
  const guard = useCallback((message: string, confirmLabel: string, action: () => void) => {
    if (dirtyRef.current) setConfirm({ message, confirmLabel, run: action })
    else action()
  }, [])

  // 打开时载入指定文件（等编辑器配置读到，自动搜索才会用上次的来源）。
  const initialHandled = useRef(false)
  const ready = config !== null
  const { loadFile } = editor
  useEffect(() => {
    if (initialHandled.current || !initialPath || !ready) return
    initialHandled.current = true
    void loadFile(initialPath)
  }, [initialPath, ready, loadFile])

  // 拖放请求：同一个 seq 只处理一次（严格模式会把副作用跑两遍）。
  // 多首交给批量修改标签（叠在编辑器上）；批量窗口开着时，拖入的一首也交给它（它在最上面）。
  const handledSeq = useRef(0)
  const setStatus = editor.setStatus
  useEffect(() => {
    if (!request || request.seq === handledSeq.current) return
    handledSeq.current = request.seq
    const { path, files, message } = request
    const forBatch = files ?? (batchOpenRef.current && path ? [{ path, name: fileNameOf(path), size: 0 }] : null)
    if (forBatch && forBatch.length > 0) {
      setBatchDrop({ seq: request.seq, files: forBatch })
      setBatchOpen(true)
      return
    }
    if (!path) {
      if (message) setStatus(message)
      return
    }
    if (busyRef.current) {
      setStatus('正在处理，请稍候再拖入文件。')
      return
    }
    guard('当前修改尚未保存，载入新文件会丢弃这些修改。', '放弃修改并载入', () => {
      // 换了一首歌：给上一首找的在线标签不能再套用。
      setOnlineTags(null)
      void loadFile(path)
    })
  }, [request, guard, loadFile, setStatus])

  const requestClose = useCallback(() => {
    if (batchOpen || onlineTags) return
    guard('当前修改尚未保存，关闭后会丢失这些修改。', '放弃修改并关闭', onClose)
  }, [batchOpen, onlineTags, guard, onClose])

  const openFile = useCallback(async () => {
    const path = await editor.pickFile()
    if (!path) return
    guard('当前修改尚未保存，载入新文件会丢弃这些修改。', '放弃修改并载入', () => void loadFile(path))
  }, [editor, guard, loadFile])

  const info = file?.info ?? null
  const hasFile = file !== null
  const canSave = hasFile && !busy
  const cover = editor.coverView
  const batchFolder = file ? directoryOf(file.path) : ''

  return (
    <>
      <Modal
        title="歌词与标签编辑器"
        className="editor-dialog"
        onClose={requestClose}
        footer={
          <>
            <div className="editor-foot-status">
              <span className="editor-foot-info" title={file?.path}>
                {info ? fileInfoText(info) : '支持 MP3 / FLAC / M4A / OGG / WAV 等格式'}
              </span>
              <span className="editor-foot-text" role="status" aria-live="polite">
                {dirty && <span className="editor-dirty">● 有未保存的修改 · </span>}
                {editor.status}
              </span>
            </div>
            <button
              type="button"
              className="btn btn-primary"
              aria-label="保存标签与歌词"
              disabled={!canSave}
              onClick={() => void editor.save()}
            >
              保存标签与歌词
            </button>
            <button type="button" className="btn" aria-label="关闭歌词编辑器" onClick={requestClose}>
              关闭
            </button>
          </>
        }
      >
        <div className="editor-head">
          <div className="editor-head-info">
            <p className="editor-file" title={file?.path}>
              {file ? fileNameOf(file.path) : '尚未选择音频文件'}
            </p>
            <p className="editor-hint">也可以把音频文件或文件夹直接拖进窗口</p>
            {config?.isElevated && (
              <p className="editor-hint editor-hint-warn" role="note">
                {config.dragDropBlockedHint}
              </p>
            )}
          </div>
          {!embedded && (
            <button
              type="button"
              className="btn"
              aria-label="打开音频文件"
              disabled={busy}
              onClick={() => void openFile()}
            >
              打开音频文件
            </button>
          )}
          <button
            type="button"
            className="btn"
            aria-label="重新读取标签"
            disabled={!hasFile || busy}
            onClick={() =>
              guard('当前修改尚未保存，重新读取会丢弃这些修改。', '放弃修改并重新读取', () => void editor.reload())
            }
          >
            重新读取
          </button>
          {!embedded && (
            <button
              type="button"
              className="btn"
              aria-label="批量修改标签"
              title="列出一个目录里的歌曲，批量修改标签，或在线获取标签、封面与歌词"
              onClick={() => {
                setBatchDrop(null)
                setBatchOpen(true)
              }}
            >
              批量修改
            </button>
          )}
        </div>

        <section className="editor-card editor-search" aria-label="在线搜索面板">
          <div className="editor-search-row">
            <select
              className="editor-select editor-select-source"
              aria-label="在线搜索来源"
              value={editor.source}
              onChange={(event) => editor.setSource(event.target.value as LyricsSourceKind)}
            >
              {SOURCES.map((kind) => (
                <option key={kind} value={kind}>
                  {LYRICS_SOURCE_LABELS[kind]}
                </option>
              ))}
            </select>
            <input
              type="text"
              className="editor-input editor-keyword"
              aria-label="在线搜索关键词"
              spellCheck={false}
              placeholder="搜索歌曲、歌手或专辑，例如：夜空中最亮的星"
              value={editor.keyword}
              onChange={(event) => editor.setKeyword(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') void editor.search()
              }}
            />
            <select
              className="editor-select editor-select-mode"
              aria-label="在线歌词格式"
              title="选择从网络获取逐行或逐字歌词"
              value={editor.preferWord ? 'word' : 'line'}
              onChange={(event) => editor.setPreferWord(event.target.value === 'word')}
            >
              <option value="line">逐行歌词</option>
              <option value="word">逐字歌词（有则优先）</option>
            </select>
            <button
              type="button"
              className="btn btn-primary"
              aria-label="在线搜索"
              disabled={editor.searching || busy}
              onClick={() => void editor.search()}
            >
              {editor.searching ? '搜索中…' : '搜索'}
            </button>
          </div>
          {editor.searchStatus !== '' && (
            <p className="editor-search-status" role="status" aria-live="polite">
              {editor.searchStatus}
            </p>
          )}
          {editor.results.length > 0 && (
            <ul className="editor-results" role="listbox" aria-label="在线搜索结果">
              {editor.results.map((entry) => {
                const view = describeEntry(entry)
                return (
                  <li key={entry.key} role="presentation">
                    <button
                      type="button"
                      role="option"
                      aria-selected={entry.key === editor.selectedKey}
                      aria-label={`载入：${view.title}，${view.subtitle}${view.matchText ? `，${view.matchText}` : ''}`}
                      className={
                        entry.key === editor.selectedKey ? 'editor-result editor-result-selected' : 'editor-result'
                      }
                      onClick={() => editor.selectEntry(entry)}
                    >
                      <span className="editor-result-main">
                        <span className="editor-result-title">{view.title}</span>
                        <span className="editor-result-sub">{view.subtitle}</span>
                      </span>
                      {view.matchText !== '' && (
                        <span
                          className={view.below ? 'editor-result-match editor-result-low' : 'editor-result-match'}
                          title={view.scoreTitle}
                        >
                          {view.matchText}
                        </span>
                      )}
                      <span className="editor-result-duration">{view.durationText}</span>
                    </button>
                  </li>
                )
              })}
            </ul>
          )}
          <p className="editor-threshold">匹配度阈值 {editor.thresholdText}（在设置的「在线匹配度」里调整）</p>
        </section>

        <div className="editor-main">
          <section className="editor-card editor-tags" aria-label="标签信息">
            <div className="editor-card-head">
              <h3 className="editor-card-title">标签信息</h3>
              <button
                type="button"
                className="btn btn-small"
                aria-label="在线标签"
                title="在网易云音乐与 QQ 音乐里查找这首歌的标签与封面（不获取歌词）"
                disabled={!file || busy}
                onClick={() => file && setOnlineTags(onlineTagsQuery(file, fields))}
              >
                在线标签…
              </button>
            </div>
            <div className="editor-cover-row">
              <div className="editor-cover" aria-label={cover.src ? '封面预览' : '没有封面预览'} role="img">
                {cover.src ? <img src={cover.src} alt="封面" draggable={false} /> : <span aria-hidden="true">♪</span>}
              </div>
              <div className="editor-cover-side">
                {cover.meta && <CoverMetaList meta={cover.meta} />}
                {(!hasFile || cover.caption) && (
                  <p className="editor-cover-caption">{hasFile ? cover.caption : '打开文件后可更换封面'}</p>
                )}
                <div className="editor-cover-actions">
                  <button
                    type="button"
                    className="btn btn-small"
                    aria-label="更换封面"
                    disabled={!hasFile || busy}
                    onClick={() => void editor.chooseCover()}
                  >
                    更换封面…
                  </button>
                  <button
                    type="button"
                    className="btn btn-small"
                    aria-label="移除封面"
                    disabled={!hasFile || busy || editor.cover.kind === 'removed' || (editor.cover.kind === 'file' && !info?.hasCover)}
                    onClick={editor.removeCover}
                  >
                    移除封面
                  </button>
                  {editor.cover.kind !== 'file' && (
                    <button type="button" className="btn btn-small btn-ghost" aria-label="还原封面" onClick={editor.restoreCover}>
                      还原
                    </button>
                  )}
                </div>
              </div>
            </div>
            <div className="editor-tags-scroll">
              <TextField label="标题" name="title" value={fields.title} onChange={editor.setField} />
              <TextField label="艺术家（多个用 / 分隔）" name="artist" value={fields.artist} onChange={editor.setField} />
              <TextField label="专辑" name="album" value={fields.album} onChange={editor.setField} />
              <TextField label="专辑艺术家" name="albumArtist" value={fields.albumArtist} onChange={editor.setField} />
              <div className="editor-field-row">
                <TextField label="年份" name="year" value={fields.year} onChange={editor.setField} />
                <TextField label="音轨号" name="trackNumber" value={fields.trackNumber} onChange={editor.setField} />
                <TextField label="碟号" name="discNumber" value={fields.discNumber} onChange={editor.setField} />
              </div>
              <TextField label="风格" name="genre" value={fields.genre} onChange={editor.setField} />
              <TextField label="作曲家（多个用 / 分隔）" name="composer" value={fields.composer} onChange={editor.setField} />
              <TextField label="作词家（多个用 / 分隔）" name="lyricist" value={fields.lyricist} onChange={editor.setField} />
              <TextField label="注释" name="comment" value={fields.comment} onChange={editor.setField} />
            </div>
          </section>

          <section className="editor-card editor-lyrics" aria-label="歌词">
            <div className="editor-lyrics-head">
              <div>
                <h3 className="editor-card-title editor-accent">歌词</h3>
                <p className="editor-hint">直接编辑并写入音频标签；支持 LRC / 逐字歌词文本。</p>
              </div>
              <span className="editor-hint">{fields.lyrics.length.toLocaleString('zh-CN')} 字符</span>
            </div>
            <textarea
              className="editor-textarea"
              aria-label="歌词内容"
              spellCheck={false}
              placeholder="在此输入或粘贴歌词；保存后会写入音频文件的歌词标签。"
              value={fields.lyrics}
              onChange={(event) => editor.setField('lyrics', event.target.value)}
            />
          </section>
        </div>
      </Modal>
      {batchOpen && (
        <BatchTagsDialog
          initialFolder={batchDrop ? directoryOf(batchDrop.files[0]?.path ?? '') || batchFolder : batchFolder}
          drop={batchDrop}
          renderEditor={(path, close) => <LyricsEditorDialog initialPath={path} request={null} embedded onClose={close} />}
          onClose={() => {
            setBatchOpen(false)
            setBatchDrop(null)
            // 批量窗口里可能改过正在编辑的这首歌：表单没动过就重新读一遍，免得之后保存时把旧内容写回去。
            if (file && !dirtyRef.current && !busyRef.current) void loadFile(file.path, false)
          }}
        />
      )}
      {onlineTags && (
        <OnlineTagsDialog
          context={onlineTags.context}
          initialKeyword={onlineTags.keyword}
          onApply={editor.applyOnlineTags}
          onClose={() => setOnlineTags(null)}
        />
      )}
      {confirm && (
        <ConfirmDialog
          title="有未保存的修改"
          message={confirm.message}
          confirmLabel={confirm.confirmLabel}
          cancelLabel="继续编辑"
          danger
          onConfirm={() => {
            const run = confirm.run
            setConfirm(null)
            run()
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </>
  )
}
