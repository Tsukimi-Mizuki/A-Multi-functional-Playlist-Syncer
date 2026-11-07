import { invoke } from '@tauri-apps/api/core'
import type { QualityOption } from './app'

export type ThemeMode = 'Dark' | 'Light' | 'System'
export type WindowMaterial = 'Acrylic' | 'Mica' | 'None'
export type LyricsWriteMode = 'None' | 'Line' | 'Word'
export type SourcePriority = 'NeteaseFirst' | 'QQMusicFirst' | 'NeteaseOnly' | 'QQMusicOnly'
export type LyricsSourceKind = 'Netease' | 'QQMusic' | 'Kugou' | 'Lrclib'
/** 主界面背景：内置背景（渐变光斑 / 自选图片 / 歌单封面）由界面用 WebGL 画，玻璃能真正折射；`System` 透出桌面。 */
export type BackgroundSource = 'System' | 'Gradient' | 'Image' | 'PlaylistCover'
export type GradientPreset = 'Aurora' | 'Ocean' | 'Sunset' | 'Forest'

/** 与 Rust 端 `LiquidGlass`（camelCase）一一对应：液态玻璃的高级参数，范围见 `glass/params.ts`。 */
export interface LiquidGlass {
  refThickness: number
  refDistance: number
  refFactor: number
  refDispersion: number
  fresnelRange: number
  fresnelHardness: number
  fresnelFactor: number
  glareRange: number
  glareHardness: number
  glareFactor: number
  glareConvergence: number
  glareOppositeFactor: number
  glareAngle: number
  blurRadius: number
  shadowExpand: number
  shadowFactor: number
  shadowOffsetX: number
  shadowOffsetY: number
  cornerRadius: number
  superellipse: number
  blurEdge: boolean
  /** `#rrggbb`；空字符串表示跟随主题。 */
  tintColor: string
}

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
  /** 背景雾度（0–1）：对话框打开时下面那层界面虚化与遮暗的程度。 */
  backdropHaze: number
  reduceMotion: boolean
  /** 启动时自动检查 GitHub 上的新版本。 */
  autoCheckUpdates: boolean
  /** 「忽略本次更新」记住的版本（空表示没有），由更新提醒写入，设置窗口不改它。 */
  ignoredUpdateVersion: string
  background: BackgroundSource
  gradientPreset: GradientPreset
  /** 自选背景图片的路径（`background` 为 `Image` 时使用）。 */
  backgroundImage: string
  liquidGlass: LiquidGlass
}

/** 实际要铺的系统材质（与 Rust 端 `effective_window_material` 一致）：内置背景盖满窗口，下面不铺材质。 */
export function effectiveMaterial(settings: Pick<AppSettings, 'background' | 'windowMaterial'>): WindowMaterial {
  return settings.background === 'System' ? settings.windowMaterial : 'None'
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
