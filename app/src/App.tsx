import { useCallback, useEffect, useRef, useState } from 'react'
import { logout } from './api/auth'
import { getAppState, type AppState } from './api/app'
import { getSettings, previewAppearance, type SettingsState } from './api/settings'
import { setQuality, type TransferSummary } from './api/transfer'
import { directoryOf, errorMessage, resolveDrop, takeStartupFile, type DropResolution } from './api/editor'
import { pickAudioFile } from './api/tags'
import type { Song } from './api/playlists'
import AccountButton from './components/AccountButton'
import BatchLyricsDialog, { type BatchDrop } from './components/BatchLyricsDialog'
import ConfirmDialog from './components/ConfirmDialog'
import DownloadMissingDialog from './components/DownloadMissingDialog'
import DropOverlay from './components/DropOverlay'
import GlassBackdrop from './components/GlassBackdrop'
import LyricsEditorDialog, { type EditorRequest } from './components/LyricsEditorDialog'
import LyricsDialog from './components/LyricsDialog'
import NcmImportDialog from './components/NcmImportDialog'
import LogPanel from './components/LogPanel'
import TagsDialog from './components/TagsDialog'
import PlaylistSidebar from './components/PlaylistSidebar'
import PlaylistView from './components/PlaylistView'
import QrLoginDialog from './components/QrLoginDialog'
import SettingsDialog from './components/SettingsDialog'
import SyncConfirmDialog from './components/SyncConfirmDialog'
import TitleBar from './components/TitleBar'
import UpdateDialog from './components/UpdateDialog'
import { installUnderGlass } from './glass/underGlass'
import { appearanceOf, applyAppearance, useAppearance } from './state/appearance'
import { useAuth } from './state/useAuth'
import { useFileDrop } from './state/useFileDrop'
import { useLogs } from './state/useLogs'
import { useLocalMatch } from './state/useLocalMatch'
import { usePlaylists } from './state/usePlaylists'
import { useSelection } from './state/useSelection'
import { useTransfer } from './state/useTransfer'
import { useTransferConfirm } from './state/useTransferConfirm'
import { useUpdates } from './state/useUpdates'

type DialogState = 'none' | 'login' | 'logout' | 'ncm' | 'settings'

const SIDEBAR_KEY = 'lp.sidebarCollapsed'
const NO_SONGS: Song[] = []

/** 拖入结果里能交给批量获取的文件：多首直接用；只有一首也可以（批量窗口开着时就换成这一首）。 */
function droppedFiles(result: DropResolution): BatchDrop['files'] | null {
  if (result.kind === 'batch') return result.files
  if (result.kind === 'audio' && result.path) {
    return [{ path: result.path, name: result.fileName ?? result.path, size: 0 }]
  }
  return null
}

/** 侧栏收起状态只是每个用户自己的界面偏好：存取失败（隐私模式等）时静默回退。 */
function readSidebarCollapsed(): boolean {
  try {
    return window.localStorage.getItem(SIDEBAR_KEY) === '1'
  } catch {
    return false
  }
}

function writeSidebarCollapsed(collapsed: boolean): void {
  try {
    window.localStorage.setItem(SIDEBAR_KEY, collapsed ? '1' : '0')
  } catch {
    // 偏好存不下来不影响使用。
  }
}

export default function App() {
  const [appState, setAppState] = useState<AppState | null>(null)
  const [dialog, setDialog] = useState<DialogState>('none')
  const [sidebarCollapsed, setSidebarCollapsed] = useState(readSidebarCollapsed)
  // 歌词预览是只读的临时对话框，与登录 / 退出确认互不影响。
  const [lyricsSong, setLyricsSong] = useState<Song | null>(null)
  // 标签查看同样是只读的临时对话框：保存的是用户选中的本地音频路径。
  const [tagsPath, setTagsPath] = useState<string | null>(null)
  // 歌词与标签编辑器（大型模态面板）：`initialPath` 是打开时直接载入的文件，`request` 是打开期间拖进来的文件。
  const [editor, setEditor] = useState<{ key: number; initialPath: string | null } | null>(null)
  const [editorRequest, setEditorRequest] = useState<EditorRequest | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  // 主窗口拖入多首歌时打开的批量获取（编辑器里拖入时由编辑器自己的批量窗口处理）。
  const [batch, setBatch] = useState<{ key: number; folder: string; drop: BatchDrop } | null>(null)
  const editorKey = useRef(0)
  const requestSeq = useRef(0)
  const { account, validated } = useAuth()
  const logs = useLogs()
  // 启动时按设置自动检查一次更新，发现新版本就弹窗；设置里的「立即检查」也经它弹窗。
  const updates = useUpdates()
  const playlists = usePlaylists(account, validated)
  // 歌曲加载完成后与本地曲库对比；设置还没读到（null）时先不动。
  const local = useLocalMatch(
    playlists.detail.status === 'ready' ? playlists.detail.detail : null,
    appState ? appState.settings.autoMatchLocal : null,
  )
  const loadedDetail = playlists.detail.status === 'ready' ? playlists.detail.detail : null
  const selection = useSelection(loadedDetail?.songs ?? NO_SONGS)
  // 下载 / 同步结束后：清掉勾选，重新对比本地曲库（后端已经重新扫描过，这里直接复用扫描结果）。
  const clearSelection = selection.clear
  const compareLocal = local.compare
  const onTransferFinished = useCallback(
    (summary: TransferSummary) => {
      if (!summary.cancelled) clearSelection()
      compareLocal()
    },
    [clearSelection, compareLocal],
  )
  const transfer = useTransfer(loadedDetail?.id ?? null, onTransferFinished)
  const transferConfirm = useTransferConfirm(loadedDetail, local.ensure, transfer)
  // 下载音质：启动时取设置里的值，工具栏改动后立即生效并写回设置。
  const [quality, setQualityState] = useState<string | null>(null)
  const [qualityError, setQualityError] = useState<string | null>(null)
  const onQualityChange = useCallback((next: string) => {
    setQualityState(next)
    setQualityError(null)
    setQuality(next).catch((raised: unknown) => {
      setQualityError(typeof raised === 'string' ? raised : '保存下载音质失败，请稍后重试')
    })
  }, [])

  useEffect(() => {
    let cancelled = false
    getAppState()
      .then((app) => {
        if (cancelled) return
        setAppState(app)
        setQualityState((current) => current ?? app.settings.quality)
      })
      .catch(() => undefined) // 非 Tauri 环境（纯浏览器预览）没有后端。
    // 外观（主题、玻璃浓度、减少动效）以后端的设置为准；首帧已经用上次缓存的外观铺过了。
    getSettings()
      .then((loaded) => {
        if (!cancelled) applyAppearance(appearanceOf(loaded.settings))
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [])

  // 液态玻璃：内置背景由 WebGL 画布负责；对话框折射它下面的主界面。
  const appearance = useAppearance()
  const [webglAvailable, setWebglAvailable] = useState(true)
  useEffect(() => installUnderGlass(), [])
  // WebGL2 不可用时内置背景画不出来：界面退回系统材质的样式，并把窗口材质铺回去，免得只剩一个透明窗口。
  const onWebglUnavailable = useCallback(() => {
    setWebglAvailable(false)
    getSettings()
      .then((loaded) => previewAppearance(loaded.settings.windowMaterial, loaded.settings.theme))
      .catch(() => undefined)
  }, [])

  // 设置保存后：刷新主界面用到的设置摘要（自动对比、目录等）与工具栏的下载音质。
  const onSettingsSaved = useCallback((saved: SettingsState) => {
    setQualityState(saved.settings.quality)
    setQualityError(null)
    getAppState()
      .then(setAppState)
      .catch(() => undefined)
  }, [])

  const toggleSidebar = useCallback(() => {
    setSidebarCollapsed((previous) => {
      writeSidebarCollapsed(!previous)
      return !previous
    })
  }, [])

  // Ctrl+B 切换歌单侧栏（与 C# 版一致）。
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 'b') {
        event.preventDefault()
        toggleSidebar()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [toggleSidebar])

  /** 弹出系统文件选择器，选中后打开标签查看；取消则保持原状。 */
  const pickTagsFile = useCallback(() => {
    pickAudioFile()
      .then((path) => {
        if (path) setTagsPath(path)
      })
      .catch(() => undefined) // 非 Tauri 环境（纯浏览器预览）没有系统对话框。
  }, [])

  const openEditor = useCallback((path: string | null) => {
    editorKey.current += 1
    setEditorRequest(null)
    setEditor({ key: editorKey.current, initialPath: path })
  }, [])
  const closeEditor = useCallback(() => {
    setEditor(null)
    setEditorRequest(null)
  }, [])

  // `--lyrics/--edit <文件>` 启动参数：启动后直接打开编辑器（后端只交出一次）。
  useEffect(() => {
    let cancelled = false
    takeStartupFile()
      .then((path) => {
        if (!cancelled && path) openEditor(path)
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [openEditor])

  // 提示条 8 秒后自动消失。
  useEffect(() => {
    if (!notice) return
    const timer = window.setTimeout(() => setNotice(null), 8000)
    return () => window.clearTimeout(timer)
  }, [notice])

  // 拖放：编辑器打开时把文件交给编辑器；否则解析后直接用编辑器打开，.ncm 提示改用「导入 NCM」。
  const editorOpen = editor !== null
  const batchOpen = batch !== null
  const openRef = useRef({ editorOpen, batchOpen })
  useEffect(() => {
    openRef.current = { editorOpen, batchOpen }
  })
  // 拖入一首：用编辑器打开；多首（或含多首的文件夹）：交给批量获取。编辑器开着时交给编辑器决定，
  // 批量窗口开着时换成新拖入的这批。
  const onDrop = useCallback(
    (paths: string[]) => {
      const { editorOpen: inEditor, batchOpen: inBatch } = openRef.current
      resolveDrop(paths, !inEditor)
        .then((result) => {
          if (inEditor) {
            requestSeq.current += 1
            setEditorRequest({
              seq: requestSeq.current,
              path: result.kind === 'audio' ? result.path : null,
              files: result.kind === 'batch' ? result.files : null,
              message: result.kind === 'audio' || result.kind === 'batch' ? null : result.message,
            })
            return
          }
          const files = droppedFiles(result)
          if (inBatch) {
            requestSeq.current += 1
            const seq = requestSeq.current
            if (files) setBatch((current) => (current ? { ...current, drop: { seq, files } } : current))
            else setNotice(result.message)
          } else if (result.kind === 'batch') {
            setNotice(null)
            requestSeq.current += 1
            editorKey.current += 1
            setBatch({
              key: editorKey.current,
              folder: directoryOf(result.files[0]?.path ?? ''),
              drop: { seq: requestSeq.current, files: result.files },
            })
          } else if (result.kind === 'audio' && result.path) {
            setNotice(null)
            openEditor(result.path)
          } else {
            setNotice(result.message)
          }
        })
        .catch((raised: unknown) => {
          const message = `载入拖入的文件失败：${errorMessage(raised, '请重试')}`
          if (inEditor) {
            requestSeq.current += 1
            setEditorRequest({ seq: requestSeq.current, path: null, files: null, message })
          } else {
            setNotice(message)
          }
        })
    },
    [openEditor],
  )
  // 登录 / NCM 导入 / 歌词预览 / 更新提醒 / 下载确认等对话框占着界面时不接收拖放（编辑器与批量窗口自己接收）。
  const dropEnabled =
    editorOpen ||
    batchOpen ||
    (dialog === 'none' && !lyricsSong && !tagsPath && !updates.prompt && !transferConfirm.pending)
  const { dragging } = useFileDrop(dropEnabled, onDrop)

  const openLogin = useCallback(() => setDialog('login'), [])
  const openNcm = useCallback(() => setDialog('ncm'), [])
  const closeDialog = useCallback(() => setDialog('none'), [])

  const confirmLogout = useCallback(() => {
    setDialog('none')
    // 退出结果由后端的账号事件 + 日志反映，这里不需要处理返回值。
    void logout().catch(() => undefined)
  }, [])

  const selected =
    playlists.list.status === 'ready'
      ? (playlists.list.items.find((item) => item.id === playlists.selectedId) ?? null)
      : null
  const currentQuality = quality ?? appState?.settings.quality ?? 'ExHigh'
  const qualityOptions = appState?.qualityOptions ?? []
  const qualityLabel = qualityOptions.find((option) => option.value === currentQuality)?.label ?? currentQuality
  // 背景选「歌单封面」时用当前歌单的封面。
  const coverUrl = loadedDetail?.coverUrl ?? selected?.coverUrl ?? null

  return (
    <div className="app-shell">
      {appearance && appearance.background !== 'System' && webglAvailable && (
        <GlassBackdrop coverUrl={coverUrl} onUnavailable={onWebglUnavailable} />
      )}
      <TitleBar
        toolsSlot={
          <>
            <button type="button" className="btn" aria-label="打开歌词与标签编辑器" onClick={() => openEditor(null)}>
              歌词编辑
            </button>
            <button type="button" className="btn" aria-label="导入并解密 NCM 文件" onClick={openNcm}>
              导入 NCM
            </button>
            <button type="button" className="btn" aria-label="查看本地音频文件的标签" onClick={pickTagsFile}>
              查看标签
            </button>
            <button
              type="button"
              className="btn btn-icon"
              aria-label="打开设置"
              title="设置"
              onClick={() => setDialog('settings')}
            >
              <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
                <path
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.7"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M12 15.2a3.2 3.2 0 1 0 0-6.4 3.2 3.2 0 0 0 0 6.4Zm7.4-3.2c0-.5 0-1-.1-1.4l2-1.6-2-3.4-2.4 1a7.6 7.6 0 0 0-2.4-1.4L14.2 2.6h-4l-.4 2.6c-.9.3-1.7.8-2.4 1.4l-2.4-1-2 3.4 2 1.6a7.4 7.4 0 0 0 0 2.8l-2 1.6 2 3.4 2.4-1c.7.6 1.5 1.1 2.4 1.4l.4 2.6h4l.4-2.6c.9-.3 1.7-.8 2.4-1.4l2.4 1 2-3.4-2-1.6c.1-.4.1-.9.1-1.4Z"
                />
              </svg>
            </button>
          </>
        }
        accountSlot={<AccountButton account={account} onLogin={openLogin} onLogout={() => setDialog('logout')} />}
      />
      {appState?.session.loadError && (
        <p className="app-banner" role="alert">
          登录信息读取失败：{appState.session.loadError}
        </p>
      )}
      {appState?.settingsWarning && (
        <p className="app-banner" role="alert">
          设置读取失败，已使用默认设置：{appState.settingsWarning}
        </p>
      )}
      {notice && (
        <p className="app-banner app-banner-info" role="status">
          {notice}
        </p>
      )}
      <main className="workspace">
        <PlaylistSidebar
          collapsed={sidebarCollapsed}
          onToggle={toggleSidebar}
          loggedIn={account.loggedIn}
          list={playlists.list}
          selectedId={playlists.selectedId}
          onSelect={playlists.select}
          onRefresh={playlists.reloadList}
          onLogin={openLogin}
        />
        <section className="main-pane" aria-label="歌曲" data-glass="panel">
          <PlaylistView
            loggedIn={account.loggedIn}
            list={playlists.list}
            selected={selected}
            detail={playlists.detail}
            onLogin={openLogin}
            onRetryList={playlists.reloadList}
            onRetryDetail={playlists.reloadDetail}
            onShowLyrics={setLyricsSong}
            onEditFile={openEditor}
            local={local}
            selection={selection}
            transfer={transfer}
            transferConfirm={transferConfirm}
            quality={currentQuality}
            qualityOptions={qualityOptions}
            qualityError={qualityError}
            onQualityChange={onQualityChange}
          />
        </section>
      </main>
      <LogPanel logs={logs} />
      {dialog === 'login' && <QrLoginDialog onClose={closeDialog} />}
      {dialog === 'ncm' && <NcmImportDialog onClose={closeDialog} />}
      {dialog === 'settings' && (
        <SettingsDialog
          coverUrl={coverUrl}
          webglAvailable={webglAvailable}
          onClose={closeDialog}
          onSaved={onSettingsSaved}
          onUpdateFound={updates.show}
        />
      )}
      {editor && <LyricsEditorDialog key={editor.key} initialPath={editor.initialPath} request={editorRequest} onClose={closeEditor} />}
      {batch && (
        <BatchLyricsDialog key={batch.key} initialFolder={batch.folder} drop={batch.drop} onClose={() => setBatch(null)} />
      )}
      {dragging && <DropOverlay target={editorOpen ? 'editor' : batchOpen ? 'batch' : 'main'} />}
      {tagsPath && <TagsDialog path={tagsPath} onClose={() => setTagsPath(null)} onPickAnother={pickTagsFile} />}
      {lyricsSong && <LyricsDialog key={lyricsSong.id} song={lyricsSong} onClose={() => setLyricsSong(null)} />}
      {transferConfirm.pending?.kind === 'missing' && (
        <DownloadMissingDialog
          pending={transferConfirm.pending}
          qualityLabel={qualityLabel}
          onConfirm={transferConfirm.confirmMissing}
          onCancel={transferConfirm.dismiss}
        />
      )}
      {transferConfirm.pending?.kind === 'sync' && (
        <SyncConfirmDialog
          pending={transferConfirm.pending}
          qualityLabel={qualityLabel}
          onConfirm={transferConfirm.confirmSync}
          onCancel={transferConfirm.dismiss}
        />
      )}
      {dialog === 'logout' && (
        <ConfirmDialog
          title="退出登录"
          message={`确定要退出「${account.nickname ?? '网易云用户'}」吗？已下载的本地文件不会被删除。`}
          confirmLabel="退出登录"
          danger
          onConfirm={confirmLogout}
          onCancel={closeDialog}
        />
      )}
      {updates.prompt && (
        <UpdateDialog
          key={updates.prompt.release.version}
          check={updates.prompt}
          error={updates.openError}
          onDownload={updates.openDownload}
          onIgnore={updates.ignore}
          onLater={updates.remindLater}
        />
      )}
    </div>
  )
}
