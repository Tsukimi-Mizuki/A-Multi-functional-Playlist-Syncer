import type { AppSettings, ThemeMode } from '../api/settings'

/** 只在前端生效的外观部分：主题、玻璃浓度、背景雾度与减少动效（窗口材质由后端铺在网页下面）。 */
export interface Appearance {
  theme: ThemeMode
  glassOpacity: number
  backdropHaze: number
  reduceMotion: boolean
}

/** 背景雾度的默认值，与后端 `DEFAULT_BACKDROP_HAZE` 一致。 */
const DEFAULT_BACKDROP_HAZE = 0.3

const CACHE_KEY = 'lp.appearance'
let appliedTheme: ThemeMode | null = null

export function appearanceOf(settings: AppSettings): Appearance {
  return {
    theme: settings.theme,
    glassOpacity: settings.glassOpacity,
    backdropHaze: settings.backdropHaze,
    reduceMotion: settings.reduceMotion,
  }
}

/** 是否应该减少动效：用户在设置里打开，或系统开启了“减少动画”。 */
export function motionReduced(): boolean {
  return (
    document.documentElement.hasAttribute('data-reduce-motion') ||
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  )
}

function write(next: Appearance): void {
  const root = document.documentElement
  if (next.theme === 'System') root.removeAttribute('data-theme')
  else root.dataset.theme = next.theme === 'Light' ? 'light' : 'dark'
  root.style.setProperty('--glass-strength', String(next.glassOpacity))
  root.style.setProperty('--backdrop-haze', String(next.backdropHaze))
  root.toggleAttribute('data-reduce-motion', next.reduceMotion)
  appliedTheme = next.theme
}

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
    })
  } catch {
    // 缓存损坏就用默认外观。
  }
}
