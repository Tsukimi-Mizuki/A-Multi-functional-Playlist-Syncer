import { useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import type { PlaylistSummary } from '../api/playlists'
import { imageSrc } from '../lib/format'
import type { ListState } from '../state/usePlaylists'

interface PlaylistSidebarProps {
  collapsed: boolean
  onToggle: () => void
  loggedIn: boolean
  list: ListState
  selectedId: number | null
  onSelect: (id: number) => void
  onRefresh: () => void
  onLogin: () => void
}

function ToggleIcon({ collapsed }: { collapsed: boolean }) {
  // 展开时是歌单图标，收起时是“展开”箭头，与 C# 版一致。
  return collapsed ? (
    <svg width="16" height="16" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M9 6l6 6-6 6z" fill="currentColor" />
    </svg>
  ) : (
    <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M15 6H3v2h12V6zm0 4H3v2h12v-2zM3 16h8v-2H3v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z"
        fill="currentColor"
      />
    </svg>
  )
}

function Cover({ playlist }: { playlist: PlaylistSummary }) {
  const [failed, setFailed] = useState(false)
  const src = imageSrc(playlist.coverUrl, 96)
  return (
    <span className="pl-cover" aria-hidden="true">
      <span className="pl-cover-fallback">♪</span>
      {src && !failed && (
        <img src={src} alt="" loading="lazy" decoding="async" draggable={false} onError={() => setFailed(true)} />
      )}
    </span>
  )
}

/** 左侧歌单侧栏：展开 272px，收起 88px 只显示封面；宽度过渡动画，列表上下边缘渐隐。 */
export default function PlaylistSidebar({
  collapsed,
  onToggle,
  loggedIn,
  list,
  selectedId,
  onSelect,
  onRefresh,
  onLogin,
}: PlaylistSidebarProps) {
  const listRef = useRef<HTMLUListElement>(null)
  const items = list.status === 'ready' ? list.items : []
  const refreshing = list.status === 'loading' || (list.status === 'ready' && list.refreshing)
  const toggleLabel = collapsed ? '展开歌单栏（Ctrl+B）' : '收起歌单栏（Ctrl+B）'

  // 上下方向键在歌单之间移动焦点，不必逐个 Tab。
  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return
    const buttons = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>('button.pl-item') ?? [])
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
    if (index < 0) return
    const next = buttons[index + (event.key === 'ArrowDown' ? 1 : -1)]
    if (next) {
      event.preventDefault()
      next.focus()
    }
  }

  let placeholder: ReactNode = null
  if (!loggedIn) {
    placeholder = (
      <div className="sidebar-empty" role="status">
        <span className="sidebar-empty-icon" aria-hidden="true">
          ♪
        </span>
        <p>登录后在此显示你的歌单</p>
        <button type="button" className="btn btn-primary" aria-label="扫码登录" onClick={onLogin}>
          扫码登录
        </button>
      </div>
    )
  } else if (list.status === 'loading' || list.status === 'idle') {
    placeholder = (
      <div className="sidebar-empty" role="status" aria-label="正在加载歌单">
        <span className="spinner" aria-hidden="true" />
        <p>正在加载歌单…</p>
      </div>
    )
  } else if (list.status === 'error') {
    placeholder = (
      <div className="sidebar-empty" role="alert">
        <p className="sidebar-error">{list.message}</p>
        <button type="button" className="btn" aria-label="重新加载歌单" onClick={onRefresh}>
          重试
        </button>
      </div>
    )
  } else if (items.length === 0) {
    placeholder = (
      <div className="sidebar-empty" role="status">
        <span className="sidebar-empty-icon" aria-hidden="true">
          ♪
        </span>
        <p>账号下暂无歌单，点击「刷新」重试</p>
      </div>
    )
  }

  return (
    <aside className={`sidebar${collapsed ? ' sidebar-collapsed' : ''}`} aria-label="歌单侧栏" data-glass="panel">
      <header className="sidebar-head">
        <button
          type="button"
          className="btn btn-ghost sidebar-toggle"
          aria-label={toggleLabel}
          aria-expanded={!collapsed}
          title={toggleLabel}
          onClick={onToggle}
        >
          <ToggleIcon collapsed={collapsed} />
        </button>
        <span className="sidebar-title">
          我的歌单
          {items.length > 0 && <span className="sidebar-count">{items.length}</span>}
        </span>
        {loggedIn && (
          <button
            type="button"
            className="btn btn-ghost btn-small sidebar-refresh"
            aria-label="刷新歌单"
            disabled={refreshing}
            tabIndex={collapsed ? -1 : 0}
            onClick={onRefresh}
          >
            {refreshing ? '刷新中…' : '刷新'}
          </button>
        )}
      </header>

      {loggedIn && list.status === 'ready' && list.error && !collapsed && (
        <p className="sidebar-note" role="alert">
          {list.error}
        </p>
      )}

      <div className="sidebar-body">
        {items.length > 0 && (
          <ul ref={listRef} className="playlist-list" aria-label="歌单列表" onKeyDown={onKeyDown}>
            {items.map((playlist) => {
              const name = playlist.name.trim() === '' ? '未命名歌单' : playlist.name
              const selected = playlist.id === selectedId
              return (
                <li key={playlist.id}>
                  <button
                    type="button"
                    className={`pl-item${selected ? ' pl-item-selected' : ''}`}
                    aria-label={`歌单：${name}，${playlist.trackCount} 首`}
                    aria-current={selected ? 'true' : undefined}
                    title={collapsed ? `${name}（${playlist.trackCount} 首）` : name}
                    onClick={() => onSelect(playlist.id)}
                  >
                    <Cover playlist={playlist} />
                    <span className="pl-text">
                      <span className="pl-name">{name}</span>
                      <span className="pl-count">{playlist.trackCount} 首</span>
                    </span>
                  </button>
                </li>
              )
            })}
          </ul>
        )}
        {!collapsed && placeholder}
      </div>
    </aside>
  )
}
