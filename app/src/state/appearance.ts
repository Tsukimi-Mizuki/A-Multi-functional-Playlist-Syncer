import { useSyncExternalStore } from 'react'
import type { AppSettings, BackgroundSource, GradientPreset, LiquidGlass, ThemeMode } from '../api/settings'
import { DEFAULT_LIQUID_GLASS, glassCssVars, hexToRgb, isLightTheme } from '../glass/params'

/**
 * 只在前端生效的外观部分：主题、玻璃浓度、背景雾度、减少动效、主界面背景与液态玻璃参数
 * （窗口材质由后端铺在网页下面）。
 */
export interface Appearance {
  theme: ThemeMode
  glassOpacity: number
  backdropHaze: number
  reduceMotion: boolean
  background: BackgroundSource
  gradientPreset: GradientPreset
  backgroundImage: string
  liquidGlass: LiquidGlass
}

/** 背景雾度的默认值，与后端 `DEFAULT_BACKDROP_HAZE` 一致。 */
const DEFAULT_BACKDROP_HAZE = 0.3

const CACHE_KEY = 'lp.appearance'
let appliedTheme: ThemeMode | null = null
let current: Appearance | null = null
const listeners = new Set<(appearance: Appearance) => void>()

export function appearanceOf(settings: AppSettings): Appearance {
  return {
    theme: settings.theme,
    glassOpacity: settings.glassOpacity,
    backdropHaze: settings.backdropHaze,
    reduceMotion: settings.reduceMotion,
    background: settings.background,
    gradientPreset: settings.gradientPreset,
    backgroundImage: settings.backgroundImage,
    liquidGlass: settings.liquidGlass,
  }
}

/** 当前写在 `<html>` 上的外观（设置面板预览中也是预览的那份）；还没写过为 null。 */
export function currentAppearance(): Appearance | null {
  return current
}

/** 订阅外观变化（WebGL 背景、设置里的预览框据此重画）；返回取消订阅的函数。 */
export function subscribeAppearance(listener: (appearance: Appearance) => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** 当前外观（含设置面板里还没保存的预览），变化时重新渲染。 */
export function useAppearance(): Appearance | null {
  return useSyncExternalStore(subscribeAppearance, currentAppearance)
}

/** 是否应该减少动效：用户在设置里打开，或系统开启了“减少动画”。 */
export function motionReduced(): boolean {
  return (
    document.documentElement.hasAttribute('data-reduce-motion') ||
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  )
}

const rgbTriple = (rgb: [number, number, number]) => rgb.map((channel) => Math.round(channel * 255)).join(' ')

function write(next: Appearance): void {
  const root = document.documentElement
  if (next.theme === 'System') root.removeAttribute('data-theme')
  else root.dataset.theme = next.theme === 'Light' ? 'light' : 'dark'
  root.style.setProperty('--glass-strength', String(next.glassOpacity))
  root.style.setProperty('--backdrop-haze', String(next.backdropHaze))
  root.toggleAttribute('data-reduce-motion', next.reduceMotion)
  // 内置背景时主面板由 WebGL 画成玻璃，CSS 只负责按钮、对话框等其余部分。
  root.dataset.backdrop = next.background === 'System' ? 'system' : 'builtin'
  for (const [name, value] of Object.entries(glassCssVars(next.liquidGlass, isLightTheme()))) {
    root.style.setProperty(name, value)
  }
  // 自定义色调覆盖主题底色；留空就交还给主题。
  const tint = hexToRgb(next.liquidGlass.tintColor)
  if (tint) {
    root.style.setProperty('--tint-rgb', rgbTriple(tint))
    root.style.setProperty('--tint-2-rgb', rgbTriple(tint))
  } else {
    root.style.removeProperty('--tint-rgb')
    root.style.removeProperty('--tint-2-rgb')
  }
  appliedTheme = next.theme
  current = next
  for (const listener of listeners) listener(next)
}

// 跟随系统主题时，系统切换深浅色要重新换算高光（浅色主题下亮边更强）并通知 WebGL 背景重画。
window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => {
  if (current && current.theme === 'System') write(current)
})

interface ApplyOptions {
  /** 主题变化时整窗交叉淡化（用户在设置里切换时）；启动时同步设置不需要。 */
  animate?: boolean
  /** 记下来供下次启动铺首帧；设置面板里的预览还没保存，不记。 */
  persist?: boolean
}

/**
 * 把外观写到 `<html>` 上。用户切换主题时用 View Transition 做整窗交叉淡化，
 * 颜色不会一块一块地跳；减少动效或窗口不可见（无法截取画面）时直接切换。
 */
export function applyAppearance(next: Appearance, { animate = false, persist = true }: ApplyOptions = {}): void {
  const themeChanged = appliedTheme !== null && appliedTheme !== next.theme
  const canAnimate =
    animate &&
    themeChanged &&
    typeof document.startViewTransition === 'function' &&
    document.visibilityState === 'visible'
  if (canAnimate && !next.reduceMotion && !motionReduced()) {
    document.startViewTransition(() => write(next))
  } else {
    write(next)
  }
  if (!persist) return
  try {
    window.localStorage.setItem(CACHE_KEY, JSON.stringify(next))
  } catch {
    // 只是为了下次启动首帧不闪，存不下来没关系。
  }
}

const BACKGROUNDS: readonly BackgroundSource[] = ['System', 'Gradient', 'Image', 'PlaylistCover']
const PRESETS: readonly GradientPreset[] = ['Aurora', 'Ocean', 'Sunset', 'Forest']

/** 启动时、还没拿到后端设置之前，先用上次的外观铺首帧，避免主题闪一下。 */
export function restoreCachedAppearance(): void {
  try {
    const raw = window.localStorage.getItem(CACHE_KEY)
    if (!raw) return
    const cached = JSON.parse(raw) as Partial<Appearance>
    if (cached.theme !== 'Dark' && cached.theme !== 'Light' && cached.theme !== 'System') return
    write({
      theme: cached.theme,
      glassOpacity: typeof cached.glassOpacity === 'number' ? cached.glassOpacity : 0.45,
      backdropHaze: typeof cached.backdropHaze === 'number' ? cached.backdropHaze : DEFAULT_BACKDROP_HAZE,
      reduceMotion: cached.reduceMotion === true,
      background: BACKGROUNDS.includes(cached.background as BackgroundSource) ? (cached.background as BackgroundSource) : 'Gradient',
      gradientPreset: PRESETS.includes(cached.gradientPreset as GradientPreset)
        ? (cached.gradientPreset as GradientPreset)
        : 'Aurora',
      backgroundImage: typeof cached.backgroundImage === 'string' ? cached.backgroundImage : '',
      // 缓存只为首帧不闪：缺的参数用默认值补齐，真正的值稍后由后端设置覆盖。
      liquidGlass: { ...DEFAULT_LIQUID_GLASS, ...(typeof cached.liquidGlass === 'object' ? cached.liquidGlass : {}) },
    })
  } catch {
    // 缓存损坏就用默认外观。
  }
}
