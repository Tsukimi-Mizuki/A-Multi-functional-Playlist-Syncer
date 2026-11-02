import { useEffect, useState } from 'react'
import { getAppInfo, getAppState, type AppInfo, type AppState } from './api/app'
import TitleBar from './components/TitleBar'

type LoadState =
  | { status: 'loading' }
  | { status: 'ready'; info: AppInfo; app: AppState }
  | { status: 'error'; message: string }

export default function App() {
  const [state, setState] = useState<LoadState>({ status: 'loading' })

  useEffect(() => {
    let cancelled = false
    Promise.all([getAppInfo(), getAppState()])
      .then(([info, app]) => {
        if (!cancelled) setState({ status: 'ready', info, app })
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
          <p className="card-text">基础层已就绪，功能将按模块逐步重写。</p>
          {state.status === 'loading' && (
            <p className="card-meta" role="status">
              正在读取应用状态…
            </p>
          )}
          {state.status === 'error' && (
            <p className="card-error" role="alert">
              读取应用状态失败：{state.message}
            </p>
          )}
          {state.status === 'ready' && <StatusDetails info={state.info} app={state.app} />}
        </section>
      </main>
    </div>
  )
}

function StatusDetails({ info, app }: { info: AppInfo; app: AppState }) {
  const { session } = app
  const login = session.loggedIn ? `是（${session.nickname ?? '网易云用户'}）` : '否'
  return (
    <>
      <p className="card-meta" role="status">
        版本 {info.version}
      </p>
      <dl className="info-list" aria-label="基础信息">
        <div className="info-row">
          <dt>数据目录</dt>
          <dd aria-label="数据目录">{app.dataDir}</dd>
        </div>
        <div className="info-row">
          <dt>便携模式</dt>
          <dd aria-label="便携模式">{app.isPortable ? '是' : '否'}</dd>
        </div>
        <div className="info-row">
          <dt>已保存登录</dt>
          <dd aria-label="已保存登录">{login}</dd>
        </div>
        <div className="info-row">
          <dt>下载目录</dt>
          <dd aria-label="下载目录">{app.settings.downloadDirectory}</dd>
        </div>
      </dl>
      {session.loadError && (
        <p className="card-error" role="alert">
          登录信息读取失败：{session.loadError}
        </p>
      )}
      {app.settingsWarning && (
        <p className="card-error" role="alert">
          设置读取失败，已使用默认设置：{app.settingsWarning}
        </p>
      )}
    </>
  )
}
