import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './styles/theme.css'
import './styles/app.css'
import './styles/playlists.css'
import './styles/lyrics.css'
import './styles/tags.css'
import './styles/ncm.css'
import './styles/transfer.css'
import './styles/editor.css'

const container = document.getElementById('root')
if (!container) {
  throw new Error('找不到根节点 #root')
}
const root = createRoot(container)

async function bootstrap(): Promise<void> {
  // 仅开发模式、且在普通浏览器（没有 Tauri）里：装上假后端检查界面渲染。生产构建会摘除这一段。
  if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
    const { installMockBackend } = await import('./dev/mockBackend')
    installMockBackend(new URLSearchParams(window.location.search).get('mock'))
  }
  root.render(
    <StrictMode>
      <App />
    </StrictMode>,
  )
}

void bootstrap()
