import { useCallback, useEffect, useState } from 'react'
import { logout, type AccountSummary } from './api/auth'
import { getAppInfo, getAppState, type AppInfo, type AppState } from './api/app'
import AccountButton from './components/AccountButton'
import ConfirmDialog from './components/ConfirmDialog'
import LogPanel from './components/LogPanel'
import QrLoginDialog from './components/QrLoginDialog'
import TitleBar from './components/TitleBar'
import { useAuth } from './state/useAuth'
import { useLogs } from './state/useLogs'

type LoadState =
  | { status: 'loading' }
  | { status: 'ready'; info: AppInfo; app: AppState }
  | { status: 'error'; message: string }

type DialogState = 'none' | 'login' | 'logout'

export default function App() {
  const [state, setState] = useState<LoadState>({ status: 'loading' })
  const [dialog, setDialog] = useState<DialogState>('none')
  const { account } = useAuth()
  const logs = useLogs()

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

  const closeDialog = useCallback(() => setDialog('none'), [])

  const confirmLogout = useCallback(() => {
    setDialog('none')
    // 退出结果由后端的账号事件 + 日志反映，这里不需要处理返回值。
    void logout().catch(() => undefined)
  }, [])

  return (
    <div className="app-shell">
      <TitleBar
        accountSlot={
          <AccountButton
            account={account}
            onLogin={() => setDialog('login')}
            onLogout={() => setDialog('logout')}
          />
        }
      />
      <main className="app-content">
        <section className="card" aria-label="项目状态">
          <h1 className="card-title">Lunatic Princess</h1>
          <p className="card-text">登录模块已就绪，其余功能将按模块逐步重写。</p>
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
          {state.status === 'ready' && (
            <StatusDetails info={state.info} app={state.app} account={account} />
          )}
        </section>
      </main>
      <LogPanel logs={logs} />
      {dialog === 'login' && <QrLoginDialog onClose={closeDialog} />}
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

function StatusDetails({
  info,
  app,
  account,
}: {
  info: AppInfo
  app: AppState
  account: AccountSummary
}) {
  const login = account.loggedIn ? `是（${account.nickname ?? '网易云用户'}）` : '否'
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
      {app.session.loadError && (
        <p className="card-error" role="alert">
          登录信息读取失败：{app.session.loadError}
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
