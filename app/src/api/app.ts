import { invoke } from '@tauri-apps/api/core'
import type { AccountSummary } from './auth'

/** 与 Rust 端 `AppInfoDto`（camelCase）一一对应。 */
export interface AppInfo {
  name: string
  version: string
}

/** 设置摘要：占位页只展示这几项，完整设置在模块 11 接入。 */
export interface SettingsSummary {
  downloadDirectory: string
  libraryDirectory: string
  /** 下载音质：`AudioQuality` 的成员名（如 `ExHigh`）。 */
  quality: string
  theme: string
  maxConcurrency: number
  matchThreshold: number
  /** 加载歌单后是否自动对比本地曲库。 */
  autoMatchLocal: boolean
}

/** 登录态摘要：账号信息与读取失败原因，后端不会把 Cookie 交给前端。 */
export interface SessionSummary {
  loggedIn: boolean
  nickname: string | null
  /** 登录文件读取 / 解密失败时的中文原因。 */
  loadError: string | null
  /** 完整账号摘要（头像、会员类型等），不含 Cookie。 */
  account: AccountSummary
}

/** 一档下载音质：`value` 写进设置，`label` 给界面显示。 */
export interface QualityOption {
  value: string
  label: string
}

/** 与 Rust 端 `AppStateDto` 一一对应。 */
export interface AppState {
  dataDir: string
  isPortable: boolean
  defaultDownloadDir: string
  settings: SettingsSummary
  /** 设置文件读取失败时的中文原因（此时使用默认设置）。 */
  settingsWarning: string | null
  session: SessionSummary
  /** 可选的下载音质，按从低到高排列。 */
  qualityOptions: QualityOption[]
}

/** 读取应用名称与版本，用来验证前后端 IPC 通路。 */
export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('app_info')
}

/** 读取数据目录、便携模式、设置摘要与登录态摘要。 */
export function getAppState(): Promise<AppState> {
  return invoke<AppState>('get_app_state')
}
