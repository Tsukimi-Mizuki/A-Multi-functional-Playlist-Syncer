import { useEffect, useRef, useState } from 'react'
import { resolveBackground, type ResolvedBackground } from '../glass/background'
import { glassTint, isLightTheme, tintAlpha } from '../glass/params'
import { GlassRenderer, type GlassShape } from '../glass/renderer'
import { currentAppearance, subscribeAppearance, type Appearance } from '../state/appearance'

/** 贴边长条伸出窗口外的距离：只让朝里的那条边折射，贴着窗口边缘的几条边不要出现假的折射。 */
const BLEED = 240

/**
 * 主界面 `[data-glass]` 元素的位置：`panel` 是浮起的面板（侧栏、歌曲区），`chrome` 是贴边的长条（标题栏、日志栏）。
 */
export function measureShapes(appearance: Appearance, width: number, height: number): GlassShape[] {
  const shapes: GlassShape[] = []
  for (const element of document.querySelectorAll<HTMLElement>('[data-glass]')) {
    const rect = element.getBoundingClientRect()
    if (rect.width <= 0 || rect.height <= 0) continue
    const chrome = element.dataset.glass === 'chrome'
    let { left, top, right, bottom } = rect
    if (chrome) {
      if (left <= 0.5) left -= BLEED
      if (top <= 0.5) top -= BLEED
      if (right >= width - 0.5) right += BLEED
      if (bottom >= height - 0.5) bottom += BLEED
    }
    shapes.push({
      x: left,
      y: top,
      width: right - left,
      height: bottom - top,
      radius: chrome ? 0 : appearance.liquidGlass.cornerRadius,
      tintAlpha: tintAlpha(chrome ? 'chrome' : 'panel', appearance.glassOpacity),
    })
  }
  return shapes
}

interface GlassBackdropProps {
  /** 当前选中歌单的封面（背景选「歌单封面」时用）。 */
  coverUrl: string | null
  /** WebGL2 不可用：主界面退回系统材质的样式，由调用方把窗口材质铺回来。 */
  onUnavailable: () => void
}

/**
 * 内置背景：铺满窗口的 WebGL 画布，画背景并把标题栏、侧栏、歌曲区、日志栏画成会折射的液态玻璃。
 * 背景设为「系统材质」时不挂载。只在尺寸、面板位置、外观参数或背景图变化时重画一帧。
 */
export default function GlassBackdrop({ coverUrl, onUnavailable }: GlassBackdropProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [appearance, setAppearance] = useState<Appearance | null>(currentAppearance)
  const [resolved, setResolved] = useState<ResolvedBackground | null>(null)
  const onUnavailableRef = useRef(onUnavailable)
  useEffect(() => {
    onUnavailableRef.current = onUnavailable
  })

  // 跟随系统主题切换深浅色时会原样再写一遍同一份外观：拷贝一份，保证重新换算光斑配色并重画。
  useEffect(() => subscribeAppearance((next) => setAppearance({ ...next })), [])

  // 背景源变化（或深浅色切换改变光斑配色）时重新解析；图片有缓存，拖动其他参数不会重复读取。
  const light = appearance ? isLightTheme() : false
  const source = appearance?.background
  const preset = appearance?.gradientPreset
  const imagePath = appearance?.backgroundImage
  useEffect(() => {
    if (!source || !preset || imagePath === undefined) return
    let cancelled = false
    void resolveBackground({ source, preset, imagePath, coverUrl, light }).then((next) => {
      if (!cancelled) setResolved(next)
    })
    return () => {
      cancelled = true
    }
  }, [source, preset, imagePath, coverUrl, light])

  const latest = useRef({ appearance, resolved })
  useEffect(() => {
    latest.current = { appearance, resolved }
  })
  const renderer = useRef<GlassRenderer | null>(null)
  const frame = useRef(0)
  const schedule = useRef<() => void>(() => undefined)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const created = GlassRenderer.create(canvas)
    if (!created) {
      document.documentElement.dataset.webgl = 'off'
      onUnavailableRef.current()
      return
    }
    renderer.current = created
    const draw = () => {
      frame.current = 0
      const { appearance: look, resolved: bg } = latest.current
      const active = renderer.current
      if (!look || !bg || !active || active.lost) return
      const width = window.innerWidth
      const height = window.innerHeight
      active.setSize(width, height, window.devicePixelRatio || 1)
      active.render({
        background: bg.background,
        shapes: measureShapes(look, width, height),
        glass: look.liquidGlass,
        tint: glassTint(look.liquidGlass, isLightTheme()),
      })
    }
    schedule.current = () => {
      if (frame.current === 0) frame.current = requestAnimationFrame(draw)
    }

    // 面板尺寸变了（窗口缩放、侧栏收起的过渡、日志栏展开、顶部提示条出现）就重画。
    const resize = new ResizeObserver(() => schedule.current())
    const observe = () => {
      resize.disconnect()
      resize.observe(document.documentElement)
      for (const element of document.querySelectorAll('[data-glass]')) resize.observe(element)
    }
    observe()
    const shell = canvas.parentElement
    const mutations = new MutationObserver(() => {
      observe()
      schedule.current()
    })
    if (shell) mutations.observe(shell, { childList: true })
    const onResize = () => schedule.current()
    window.addEventListener('resize', onResize)
    const onLost = (event: Event) => event.preventDefault()
    const onRestored = () => {
      renderer.current?.dispose()
      renderer.current = GlassRenderer.create(canvas)
      schedule.current()
    }
    canvas.addEventListener('webglcontextlost', onLost)
    canvas.addEventListener('webglcontextrestored', onRestored)
    schedule.current()
    return () => {
      resize.disconnect()
      mutations.disconnect()
      window.removeEventListener('resize', onResize)
      canvas.removeEventListener('webglcontextlost', onLost)
      canvas.removeEventListener('webglcontextrestored', onRestored)
      cancelAnimationFrame(frame.current)
      frame.current = 0
      renderer.current?.dispose()
      renderer.current = null
      schedule.current = () => undefined
    }
  }, [])

  // 参数或背景变化：重画一帧。
  useEffect(() => {
    schedule.current()
  }, [appearance, resolved])

  return <canvas ref={canvasRef} className="glass-backdrop" aria-hidden="true" />
}
