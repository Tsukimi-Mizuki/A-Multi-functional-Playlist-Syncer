import { invoke } from '@tauri-apps/api/core'

/** 与 Rust 端 `ReleaseInfo` 一一对应：GitHub 上的一个正式版。 */
export interface ReleaseInfo {
  /** 归一后的版本号（如 `2.2.0`），「忽略本次更新」记住的就是它。 */
  version: string
  /** GitHub 上的 tag 名（如 `v2.2`）。 */
  tag: string
  /** 整理成纯文本的更新说明（可能为空）。 */
  notes: string
  /** 发布页地址（只会是本项目的 Releases 页面）。 */
  url: string
  /** 发布日期 `YYYY-MM-DD`。 */
  publishedOn: string | null
}

/** `available` 有新版本；`ignored` 有新版本但用户忽略过它；`upToDate` 已是最新。 */
export type UpdateStatus = 'upToDate' | 'available' | 'ignored'

/** 与 Rust 端 `UpdateCheck` 一一对应。 */
export interface UpdateCheck {
  currentVersion: string
  status: UpdateStatus
  release: ReleaseInfo
}

/**
 * 检查 GitHub 上的最新正式版。`manual` 为 false 是启动时的自动检查，设置里关掉了就返回 null（不联网）。
 * 失败时 reject 的是 `{ kind, message }`，message 是中文说明。
 */
export function checkForUpdates(manual: boolean): Promise<UpdateCheck | null> {
  return invoke<UpdateCheck | null>('check_for_updates', { manual })
}

/** 「忽略本次更新」：之后的自动检查不再为这个版本弹窗。 */
export function ignoreUpdate(version: string): Promise<void> {
  return invoke<void>('ignore_update', { version })
}

/** 用默认浏览器打开发布页；后端只接受本项目的 GitHub 发布页地址。 */
export function openReleasePage(url: string): Promise<void> {
  return invoke<void>('open_release_page', { url })
}
