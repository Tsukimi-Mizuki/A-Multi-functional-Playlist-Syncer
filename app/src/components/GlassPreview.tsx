import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from 'react'
import type { AppSettings } from '../api/settings'
import { gradientBackground, resolveBackground, type ResolvedBackground } from '../glass/background'
import { glassTint, isLightTheme, tintAlpha } from '../glass/params'
import { GlassRenderer, type GlassShape } from '../glass/renderer'

/** 玻璃块的尺寸（CSS 像素）。 */
const LENS = { width: 136, height: 84 }
/** 方向键每次移动的距离。 */
const KEY_STEP = 12

interface GlassPreviewProps {
  /** 设置面板里还没保存的草稿：背景、玻璃浓度与液态玻璃参数都按它画。 */
  draft: AppSettings
  /** 当前选中歌单的封面（背景选「歌单封面」时用）。 */
  coverUrl: string | null
}

/**
 * 「高级设置」里的即时预览：一个缩小的主界面（侧栏、卡片、按钮）和一块可以拖动的玻璃。
 * 内置背景下面板也由 WebGL 画成折射的玻璃，和主窗口一样；系统材质下面板是 CSS 近似，背景用渐变代替桌面。
 */
export default function GlassPreview({ draft, coverUrl }: GlassPreviewProps) {
  const stageRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const renderer = useRef<GlassRenderer | null>(null)
  const frame = useRef(0)
  const [unsupported, setUnsupported] = useState(false)
  const [resolved, setResolved] = useState<ResolvedBackground | null>(null)
  // 玻璃块左上角相对预览框的位置；null 表示还没量过尺寸，等第一次布局后放到右下方。
  const [lens, setLens] = useState<{ x: number; y: number } | null>(null)
  const drag = useRef<{ pointer: number; dx: number; dy: number } | null>(null)

  const builtin = draft.background !== 'System'
  const light = isLightTheme()

  useEffect(() => {
    let cancelled = false
    // 系统材质下没有可画的背景：用渐变光斑代替窗口后面的桌面。
    const request = builtin
      ? resolveBackground({
          source: draft.background,
          preset: draft.gradientPreset,
          imagePath: draft.backgroundImage,
          coverUrl,
          light,
        })
      : Promise.resolve({ background: gradientBackground(draft.gradientPreset, light), note: null })
    void request.then((next) => {
      if (!cancelled) setResolved(next)
    })
    return () => {
      cancelled = true
    }
  }, [builtin, draft.background, draft.gradientPreset, draft.backgroundImage, coverUrl, light])

  const clampLens = useCallback((x: number, y: number) => {
    const stage = stageRef.current
    if (!stage) return { x, y }
    return {
      x: Math.min(Math.max(x, 0), Math.max(0, stage.clientWidth - LENS.width)),
      y: Math.min(Math.max(y, 0), Math.max(0, stage.clientHeight - LENS.height)),
    }
  }, [])

  const latest = useRef({ draft, resolved, lens, builtin })
  useEffect(() => {
    latest.current = { draft, resolved, lens, builtin }
  })

  const draw = useCallback(() => {
    frame.current = 0
    const stage = stageRef.current
    const active = renderer.current
    const { draft: settings, resolved: bg, lens: position, builtin: drawPanels } = latest.current
    if (!stage || !active || !bg || !position) return
    const origin = stage.getBoundingClientRect()
    const glass = settings.liquidGlass
    const shapes: GlassShape[] = []
    if (drawPanels) {
      for (const element of stage.querySelectorAll<HTMLElement>('.lg-preview-panel')) {
        const rect = element.getBoundingClientRect()
        shapes.push({
          x: rect.left - origin.left,
          y: rect.top - origin.top,
          width: rect.width,
          height: rect.height,
          radius: glass.cornerRadius * 0.75,
          tintAlpha: tintAlpha('panel', settings.glassOpacity),
        })
      }
    }
    // 玻璃块几乎不着色，折射与高光看得最清楚。
    shapes.push({
      ...position,
      ...LENS,
      radius: Math.min(LENS.height / 2, glass.cornerRadius * 1.6),
      tintAlpha: settings.glassOpacity * 0.2,
    })
    active.setSize(origin.width, origin.height, window.devicePixelRatio || 1)
    const background = bg.background.kind === 'gradient' ? { ...bg.background, pattern: true } : bg.background
    active.render({ background, shapes, glass, tint: glassTint(glass, isLightTheme()) })
  }, [])

  const schedule = useCallback(() => {
    if (frame.current === 0) frame.current = requestAnimationFrame(draw)
  }, [draw])

  useEffect(() => {
    const canvas = canvasRef.current
    const stage = stageRef.current
    if (!canvas || !stage) return
    const created = GlassRenderer.create(canvas)
    if (!created) {
      setUnsupported(true)
      return
    }
    renderer.current = created
    const resize = new ResizeObserver(() => {
      setLens((current) =>
        current
          ? clampLens(current.x, current.y)
          : clampLens(stage.clientWidth * 0.62 - LENS.width / 2, stage.clientHeight - LENS.height - 18),
      )
      schedule()
    })
    resize.observe(stage)
    return () => {
      resize.disconnect()
      cancelAnimationFrame(frame.current)
      frame.current = 0
      created.dispose()
      renderer.current = null
    }
  }, [clampLens, schedule])

  useEffect(() => {
    schedule()
  }, [draft, resolved, lens, builtin, light, schedule])

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (!lens) return
    event.currentTarget.setPointerCapture(event.pointerId)
    drag.current = { pointer: event.pointerId, dx: event.clientX - lens.x, dy: event.clientY - lens.y }
  }
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current
    if (!current || current.pointer !== event.pointerId) return
    setLens(clampLens(event.clientX - current.dx, event.clientY - current.dy))
  }
  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointer === event.pointerId) drag.current = null
  }
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!lens) return
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-KEY_STEP, 0],
      ArrowRight: [KEY_STEP, 0],
      ArrowUp: [0, -KEY_STEP],
      ArrowDown: [0, KEY_STEP],
    }
    const move = moves[event.key]
    if (!move) return
    event.preventDefault()
    setLens(clampLens(lens.x + move[0], lens.y + move[1]))
  }

  const note = unsupported
    ? '当前环境不支持 WebGL2：内置背景无法显示，界面使用 CSS 近似效果。'
    : builtin
      ? resolved?.note ?? null
      : '系统材质下主界面透出的是窗口后面的桌面，面板与按钮为 CSS 近似；预览里用渐变光斑代替桌面。'

  return (
    <div className="lg-preview-wrap">
      <div ref={stageRef} className={unsupported ? 'lg-preview lg-preview-fallback' : 'lg-preview'} aria-label="液态玻璃即时预览">
        <canvas ref={canvasRef} className="lg-preview-canvas" aria-hidden="true" />
        <div className="lg-preview-panel lg-preview-side" aria-hidden="true">
          <span className="lg-preview-heading">我的歌单</span>
          <span className="lg-preview-row lg-preview-row-active">我喜欢的音乐</span>
          <span className="lg-preview-row">夜间循环</span>
          <span className="lg-preview-row">通勤路上</span>
        </div>
        <div className="lg-preview-panel lg-preview-card" aria-hidden="true">
          <span className="lg-preview-title">我喜欢的音乐</span>
          <span className="lg-preview-text">共 128 首 · 本地已存在 96 首</span>
          <div className="lg-preview-actions">
            <button type="button" className="btn btn-small" tabIndex={-1}>
              下载缺失
            </button>
            <button type="button" className="btn btn-small btn-primary" tabIndex={-1}>
              一键同步
            </button>
          </div>
        </div>
        {lens && (
          <div
            className="lg-preview-lens"
            role="button"
            tabIndex={0}
            aria-label="可拖动的玻璃块：拖动或用方向键移动，查看折射效果"
            style={{ left: lens.x, top: lens.y, width: LENS.width, height: LENS.height }}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            onKeyDown={onKeyDown}
          />
        )}
      </div>
      {note && <p className="lg-preview-note">{note}</p>}
    </div>
  )
}
