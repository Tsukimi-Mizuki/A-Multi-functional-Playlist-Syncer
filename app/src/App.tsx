import { useCallback, useEffect, useState } from 'react'
import { logout } from './api/auth'
import { getAppState, type AppState } from './api/app'
import { pickAudioFile } from './api/tags'
import type { Song } from './api/playlists'
import AccountButton from './components/AccountButton'
import ConfirmDialog from './components/ConfirmDialog'
import LyricsDialog from './components/LyricsDialog'
import LogPanel from './components/LogPanel'
import TagsDialog from './components/TagsDialog'
import PlaylistSidebar from './components/PlaylistSidebar'
import PlaylistView from './components/PlaylistView'
import QrLoginDialog from './components/QrLoginDialog'
import TitleBar from './components/TitleBar'
import { useAuth } from './state/useAuth'
import { useLogs } from './state/useLogs'
import { usePlaylists } from './state/usePlaylists'

type DialogState = 'none' | 'login' | 'logout'

const SIDEBAR_KEY = 'lp.sidebarCollapsed'

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
  const { account, validated } = useAuth()
  const logs = useLogs()
  const playlists = usePlaylists(account, validated)

  useEffect(() => {
    let cancelled = false
    getAppState()
      .then((app) => {
        if (!cancelled) setAppState(app)
      })
      .catch(() => undefined) // 非 Tauri 环境（纯浏览器预览）没有后端。
    return () => {
      cancelled = true
    }
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

  const openLogin = useCallback(() => setDialog('login'), [])
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

  return (
    <div className="app-shell">
      <TitleBar
        toolsSlot={
          <button type="button" className="btn" aria-label="查看本地音频文件的标签" onClick={pickTagsFile}>
            查看标签
          </button>
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
        <section className="main-pane" aria-label="歌曲">
          <PlaylistView
            loggedIn={account.loggedIn}
            list={playlists.list}
            selected={selected}
            detail={playlists.detail}
            onLogin={openLogin}
            onRetryList={playlists.reloadList}
            onRetryDetail={playlists.reloadDetail}
            onShowLyrics={setLyricsSong}
          />
        </section>
      </main>
      <LogPanel logs={logs} />
      {dialog === 'login' && <QrLoginDialog onClose={closeDialog} />}
      {tagsPath && <TagsDialog path={tagsPath} onClose={() => setTagsPath(null)} onPickAnother={pickTagsFile} />}
      {lyricsSong && <LyricsDialog key={lyricsSong.id} song={lyricsSong} onClose={() => setLyricsSong(null)} />}
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
    </div>
  )
}
