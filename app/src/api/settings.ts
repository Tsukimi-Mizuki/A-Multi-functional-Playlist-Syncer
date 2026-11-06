import { invoke } from '@tauri-apps/api/core'
import type { QualityOption } from './app'

export type ThemeMode = 'Dark' | 'Light' | 'System'
export type WindowMaterial = 'Acrylic' | 'Mica' | 'None'
export type LyricsWriteMode = 'None' | 'Line' | 'Word'
export type SourcePriority = 'NeteaseFirst' | 'QQMusicFirst' | 'NeteaseOnly' | 'QQMusicOnly'
export type LyricsSourceKind = 'Netease' | 'QQMusic' | 'Kugou' | 'Lrclib'

/** 与 Rust 端 `AppSettings`（camelCase）一一对应，也就是 `settings.json` 的内容。 */
export interface AppSettings {
  downloadDirectory: string
  libraryDirectory: string
  quality: string
  lyricsMode: LyricsWriteMode
  writeCover: boolean
  overwriteExisting: boolean
  skipExisting: boolean
  fileNameTemplate: string
  maxConcurrency: number
  autoMatchLocal: boolean
  tagSourcePriority: SourcePriority
  lyricsSourcePriority: SourcePriority
  ncmImportFolders: string[]
  ncmImportOutputDirectory: string
  lyricsEditorSearchSource: LyricsSourceKind
  fallbackKugouLyrics: boolean
  fallbackLrclibLyrics: boolean
  lyricsEditorPreferWordLyrics: boolean
  matchThreshold: number
  theme: ThemeMode
  renderFrameRate: string
  windowMaterial: WindowMaterial
  /** 界面玻璃底色浓度（0.10–0.95）。 */
  glassOpacity: number
  reduceMotion: boolean
  /** 启动时自动检查 GitHub 上的新版本。 */
  autoCheckUpdates: boolean
  /** 「忽略本次更新」记住的版本（空表示没有），由更新提醒写入，设置窗口不改它。 */
  ignoredUpdateVersion: string
}

/** 与 Rust 端 `SettingsDto` 一一对应。 */
export interface SettingsState {
  settings: AppSettings
  dataDir: string
  settingsFile: string
  isPortable: boolean
  defaultDownloadDir: string
  /** 设置文件读取失败的原因（保存一次新设置后为空）。 */
  warning: string | null
  qualityOptions: QualityOption[]
}

/** 只含改动过的字段，后端逐项校验后写盘，其余字段保持原样。 */
export type SettingsPatch = Partial<AppSettings>

export function getSettings(): Promise<SettingsState> {
  return invoke<SettingsState>('get_settings')
}

/** 保存改动；失败时 reject 的是中文说明，设置保持原样。 */
export function saveSettings(patch: SettingsPatch): Promise<SettingsState> {
  return invoke<SettingsState>('save_settings', { patch })
}

/** 实时预览窗口主题与材质（不写盘），返回实际生效的材质。 */
export function previewAppearance(material: WindowMaterial, theme: ThemeMode): Promise<WindowMaterial> {
  return invoke<WindowMaterial>('preview_appearance', { material, theme })
}

/** 两份设置之间改动过的字段。 */
export function diffSettings(saved: AppSettings, draft: AppSettings): SettingsPatch {
  const patch: Record<string, unknown> = {}
  for (const key of Object.keys(draft) as (keyof AppSettings)[]) {
    if (JSON.stringify(draft[key]) !== JSON.stringify(saved[key])) patch[key] = draft[key]
  }
  return patch as SettingsPatch
}
