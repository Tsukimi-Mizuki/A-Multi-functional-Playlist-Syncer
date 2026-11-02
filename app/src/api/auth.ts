import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/** 与 Rust 端 `AccountSummary`（camelCase）一一对应。后端不会把 Cookie 交给前端。 */
export interface AccountSummary {
  loggedIn: boolean
  nickname: string | null
  userId: number
  vipType: number
  avatarUrl: string | null
}

export const LOGGED_OUT: AccountSummary = {
  loggedIn: false,
  nickname: null,
  userId: 0,
  vipType: 0,
  avatarUrl: null,
}

export type ValidationStatus = 'notLoggedIn' | 'valid' | 'unverified' | 'loggedOut'

export interface ValidationResult {
  status: ValidationStatus
  message: string | null
  account: AccountSummary
}

export interface QrStart {
  key: string
  /** 白底黑码的 SVG 文本。 */
  svg: string
  lifetimeSecs: number
}

export type QrPollState = 'waiting' | 'scanned' | 'expired' | 'failed' | 'success' | 'error'

export interface QrPoll {
  state: QrPollState
  message: string | null
  /** state 为 error 时，是否是暂时性的网络问题（可继续重试几次）。 */
  transient: boolean
  account: AccountSummary | null
}

/** 启动后的联网校验：读取账号并续期；网络类错误保留登录，明确 301 才清除。 */
export function validateLogin(): Promise<ValidationResult> {
  return invoke<ValidationResult>('validate_login')
}

/** 当前账号摘要（不联网）。 */
export function loginStatus(): Promise<AccountSummary> {
  return invoke<AccountSummary>('login_status')
}

/** 申请登录二维码。 */
export function qrStart(): Promise<QrStart> {
  return invoke<QrStart>('qr_start')
}

/** 轮询一次扫码状态。 */
export function qrPoll(key: string): Promise<QrPoll> {
  return invoke<QrPoll>('qr_poll', { key })
}

/** 退出登录：清内存会话、排空自动落盘、删除 session.dat。 */
export function logout(): Promise<void> {
  return invoke<void>('logout')
}

/** 账号状态变化（登录成功、退出、登录失效、资料刷新）。 */
export function onAccountChanged(handler: (account: AccountSummary) => void): Promise<UnlistenFn> {
  return listen<AccountSummary>('auth:state', (event) => handler(event.payload))
}
