import { useEffect, useState } from 'react'
import { getAppInfo, type AppInfo } from './api/app'
import TitleBar from './components/TitleBar'

type InfoState =
  | { status: 'loading' }
  | { status: 'ready'; info: AppInfo }
  | { status: 'error'; message: string }

export default function App() {
  const [state, setState] = useState<InfoState>({ status: 'loading' })

  useEffect(() => {
    let cancelled = false
    getAppInfo()
      .then((info) => {
        if (!cancelled) setState({ status: 'ready', info })
      })
      .catch((error: unknown) => {
        if (!cancelled) setState({ status: 'error', message: String(error) })
      })
    return () => {
      cancelled = true
    }
  }, [])

  return (
    <div className="app-shell">
      <TitleBar />
      <main className="app-content">
        <section className="card" aria-label="项目状态">
          <h1 className="card-title">Lunatic Princess</h1>
          <p className="card-text">项目骨架已就绪，功能将按模块逐步重写。</p>
          <p className="card-meta" role="status">
            {state.status === 'loading' && '正在读取版本…'}
            {state.status === 'ready' && `版本 ${state.info.version}`}
            {state.status === 'error' && `读取版本失败：${state.message}`}
          </p>
        </section>
      </main>
    </div>
  )
}
