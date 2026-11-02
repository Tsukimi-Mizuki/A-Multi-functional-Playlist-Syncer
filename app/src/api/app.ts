import { invoke } from '@tauri-apps/api/core'

/** 与 Rust 端 `AppInfoDto`（camelCase）一一对应。 */
export interface AppInfo {
  name: string
  version: string
}

/** 读取应用名称与版本，用来验证前后端 IPC 通路。 */
export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('app_info')
}
