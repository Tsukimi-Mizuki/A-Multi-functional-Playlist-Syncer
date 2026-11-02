import { useEffect, useState } from 'react'
import {
  LOGGED_OUT,
  loginStatus,
  onAccountChanged,
  validateLogin,
  type AccountSummary,
  type ValidationResult,
} from '../api/auth'

/** 整个应用生命周期内只做一次联网校验（开发模式下 StrictMode 会重复执行 effect）。 */
let validation: Promise<ValidationResult | null> | null = null

/**
 * 账号状态：先订阅后端的账号变化事件，再读取当前摘要，最后触发一次启动校验。
 * 校验结果（续期、登录失效、暂时无法验证）由后端写入运行日志，并通过事件更新这里的账号。
 */
export function useAuth(): { account: AccountSummary; ready: boolean } {
  const [account, setAccount] = useState<AccountSummary>(LOGGED_OUT)
  const [ready, setReady] = useState(false)

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    onAccountChanged((next) => {
      if (!disposed) setAccount(next)
    })
      .then((fn) => {
        if (disposed) fn()
        else unlisten = fn
        return loginStatus()
      })
      .then((current) => {
        if (!disposed) {
          setAccount(current)
          setReady(true)
        }
        validation ??= validateLogin().catch(() => null)
        return validation
      })
      .then((result) => {
        if (!disposed && result) setAccount(result.account)
      })
      .catch(() => {
        if (!disposed) setReady(true) // 非 Tauri 环境（纯浏览器预览）：保持未登录。
      })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  return { account, ready }
}
