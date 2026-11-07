import type { GradientPreset, LiquidGlass } from '../api/settings'

/**
 * 液态玻璃参数的界面描述与换算。参数含义沿用 liquid-glass-studio（MIT，见 THIRD_PARTY_NOTICES.md）。
 *
 * 范围必须与 Rust 端 `settings::liquid_glass` 的表一致（后端会严格校验）。
 * 内置背景下这些参数交给 WebGL（`renderer.ts`）；系统材质下与按钮、对话框一样，由 `glassCssVars` 换算成 CSS 近似。
 */

export type NumberKey = Exclude<keyof LiquidGlass, 'blurEdge' | 'tintColor'>

export interface ParamSpec {
  key: NumberKey
  label: string
  min: number
  max: number
  step: number
  format: (value: number) => string
}

export interface ParamGroup {
  title: string
  params: ParamSpec[]
}

const plain = (value: number) => (Number.isInteger(value) ? String(value) : value.toFixed(2).replace(/0+$/, '').replace(/\.$/, ''))
const px = (value: number) => `${plain(value)} px`
const deg = (value: number) => `${plain(value)}°`

/** 设置窗口「高级设置」里的分组（模糊边缘开关与色调另外放在「模糊、色调与阴影」组里）。 */
export const PARAM_GROUPS: ParamGroup[] = [
  {
    title: '折射与色散',
    params: [
      { key: 'refThickness', label: '折射厚度', min: 1, max: 80, step: 0.5, format: px },
      { key: 'refDistance', label: '背景距离', min: 0, max: 0.2, step: 0.005, format: (v) => v.toFixed(3) },
      { key: 'refFactor', label: '折射系数', min: 1, max: 4, step: 0.05, format: (v) => v.toFixed(2) },
      { key: 'refDispersion', label: '色散增益', min: 0, max: 50, step: 0.5, format: plain },
    ],
  },
  {
    title: '菲涅尔与高光',
    params: [
      { key: 'fresnelRange', label: '菲涅尔反射范围', min: 0, max: 100, step: 1, format: plain },
      { key: 'fresnelHardness', label: '菲涅尔反射硬度', min: 0, max: 100, step: 1, format: plain },
      { key: 'fresnelFactor', label: '菲涅尔反射强度', min: 0, max: 100, step: 1, format: plain },
      { key: 'glareRange', label: '高光范围', min: 0, max: 100, step: 1, format: plain },
      { key: 'glareHardness', label: '高光硬度', min: 0, max: 100, step: 1, format: plain },
      { key: 'glareFactor', label: '高光强度', min: 0, max: 120, step: 1, format: plain },
      { key: 'glareConvergence', label: '高光聚拢度', min: 0, max: 100, step: 1, format: plain },
      { key: 'glareOppositeFactor', label: '高光对侧强度', min: 0, max: 100, step: 1, format: plain },
      { key: 'glareAngle', label: '高光角度', min: -180, max: 180, step: 1, format: deg },
    ],
  },
  {
    title: '模糊、色调与阴影',
    params: [
      { key: 'blurRadius', label: '模糊半径', min: 1, max: 200, step: 1, format: px },
      { key: 'shadowExpand', label: '阴影扩散', min: 2, max: 100, step: 1, format: px },
      { key: 'shadowFactor', label: '阴影强度', min: 0, max: 100, step: 1, format: plain },
      { key: 'shadowOffsetX', label: '阴影水平位置', min: -20, max: 20, step: 0.5, format: px },
      { key: 'shadowOffsetY', label: '阴影垂直位置', min: -20, max: 20, step: 0.5, format: px },
    ],
  },
  {
    title: '形状',
    params: [
      { key: 'cornerRadius', label: '圆角', min: 0, max: 40, step: 1, format: px },
      { key: 'superellipse', label: '超椭圆系数', min: 2, max: 7, step: 0.1, format: (v) => v.toFixed(1) },
    ],
  },
]

/** 默认参数，与 Rust 端 `LiquidGlass::default()` 一致（「重置为默认」与浏览器假后端用）。 */
export const DEFAULT_LIQUID_GLASS: LiquidGlass = {
  refThickness: 20,
  refDistance: 0.05,
  refFactor: 1.4,
  refDispersion: 7,
  fresnelRange: 30,
  fresnelHardness: 20,
  fresnelFactor: 20,
  glareRange: 30,
  glareHardness: 20,
  glareFactor: 90,
  glareConvergence: 50,
  glareOppositeFactor: 80,
  glareAngle: -45,
  blurRadius: 24,
  shadowExpand: 25,
  shadowFactor: 15,
  shadowOffsetX: 0,
  shadowOffsetY: 10,
  cornerRadius: 16,
  superellipse: 5,
  blurEdge: false,
  tintColor: '',
}

export type Rgb = [number, number, number]

/** `#rrggbb` → 0–1 的 RGB；格式不对返回 null。 */
export function hexToRgb(hex: string): Rgb | null {
  const match = /^#([0-9a-f]{6})$/i.exec(hex.trim())
  if (!match) return null
  const value = Number.parseInt(match[1] ?? '', 16)
  return [((value >> 16) & 255) / 255, ((value >> 8) & 255) / 255, (value & 255) / 255]
}

/** 当前实际生效的是不是浅色（跟随系统时看系统）。 */
export function isLightTheme(): boolean {
  const theme = document.documentElement.dataset.theme
  if (theme === 'light') return true
  if (theme === 'dark') return false
  return window.matchMedia('(prefers-color-scheme: light)').matches
}

/** 深色 / 浅色主题的玻璃底色（与 theme.css 的 `--tint-rgb` 一致）。 */
export function themeTint(light: boolean): Rgb {
  return light ? [1, 1, 1] : [14 / 255, 18 / 255, 27 / 255]
}

/** 玻璃色调：自定义颜色优先，否则跟随主题。 */
export function glassTint(glass: LiquidGlass, light: boolean): Rgb {
  return hexToRgb(glass.tintColor) ?? themeTint(light)
}

/** 面板与贴边长条（标题栏、日志栏）的色调浓度，与 theme.css 的 `--panel` / `--chrome-glass` 公式一致。 */
export function tintAlpha(kind: 'panel' | 'chrome', glassOpacity: number): number {
  return kind === 'panel' ? 0.1 + glassOpacity * 0.62 : 0.12 + glassOpacity * 0.6
}

/** CSS `corner-shape: superellipse(K)` 的 K 与超椭圆指数 n 的关系：n = 2^K（K = 1 是普通圆角）。 */
export function cornerShapeK(superellipse: number): number {
  return Math.log2(Math.max(2, superellipse))
}

export interface GradientPalette {
  base: Rgb
  /** 四团光斑：颜色、中心（相对窗口 0–1）与半径（相对窗口对角线）。 */
  blobs: { color: Rgb; x: number; y: number; radius: number }[]
}

const rgb = (hex: string): Rgb => hexToRgb(hex) ?? [0, 0, 0]

const PALETTES: Record<GradientPreset, { base: string; colors: [string, string, string, string] }> = {
  Aurora: { base: '#0b0f1c', colors: ['#ff6b7d', '#7b5cff', '#2ec4b6', '#3a86ff'] },
  Ocean: { base: '#06121f', colors: ['#1e6fff', '#00b4d8', '#48cae4', '#5a3fd0'] },
  Sunset: { base: '#1a0b12', colors: ['#ff7b54', '#ffb26b', '#ff4d6d', '#8f2d56'] },
  Forest: { base: '#07140f', colors: ['#2d6a4f', '#52b788', '#95d5b2', '#1b4332'] },
}

const BLOB_LAYOUT: readonly { x: number; y: number; radius: number }[] = [
  { x: 0.12, y: 0.18, radius: 0.42 },
  { x: 0.88, y: 0.22, radius: 0.38 },
  { x: 0.72, y: 0.9, radius: 0.46 },
  { x: 0.18, y: 0.86, radius: 0.36 },
]

/** 内置渐变光斑的配色：深色主题是深底亮斑，浅色主题把底色与光斑都往白色推，避免压过文字。 */
export function gradientPalette(preset: GradientPreset, light: boolean): GradientPalette {
  const palette = PALETTES[preset] ?? PALETTES.Aurora
  const toward = (color: Rgb, target: Rgb, amount: number): Rgb => [
    color[0] + (target[0] - color[0]) * amount,
    color[1] + (target[1] - color[1]) * amount,
    color[2] + (target[2] - color[2]) * amount,
  ]
  const white: Rgb = [1, 1, 1]
  const base = light ? toward(rgb(palette.base), [0.93, 0.95, 0.98], 0.94) : rgb(palette.base)
  return {
    base,
    blobs: palette.colors.map((color, index) => ({
      ...(BLOB_LAYOUT[index] ?? { x: 0.5, y: 0.5, radius: 0.4 }),
      color: light ? toward(rgb(color), white, 0.42) : rgb(color),
    })),
  }
}

const clamp01 = (value: number) => Math.min(1, Math.max(0, value))
const round = (value: number, digits = 3) => Number(value.toFixed(digits))

/**
 * 系统材质、按钮与对话框用的 CSS 近似：网页读不到窗口下面的桌面，没法真的折射，
 * 就用同一组参数画边缘的菲涅尔亮边、按角度分布的高光、色散彩边与投影。
 */
export function glassCssVars(glass: LiquidGlass, light: boolean): Record<string, string> {
  const angle = glass.glareAngle
  // 高光峰宽：聚拢度越高越窄（参考实现里聚拢度让 sin 波形的峰更尖）。
  const width = 70 - glass.glareConvergence * 0.55
  const peak = clamp01((glass.glareFactor / 100) * 0.9)
  const opposite = clamp01(peak * (glass.glareOppositeFactor / 100))
  // 浅色主题下白色亮边落在白色玻璃上看不见：非高光部分改用很淡的深色描边，高光仍是白色。
  const base = clamp01(light ? 0.1 + (glass.fresnelFactor / 100) * 0.3 : (glass.fresnelFactor / 100) * 0.55)
  // 高光范围越大，峰两侧的过渡越宽；硬度越高过渡越陡。
  const soften = (1 - glass.glareHardness / 100) * (glass.glareRange / 100) * 40
  const white = (alpha: number) => `rgb(255 255 255 / ${round(alpha)})`
  const rim = (alpha: number) => (light ? `rgb(24 34 54 / ${round(alpha)})` : white(alpha))
  const stops = [
    `${white(peak)} 0deg`,
    `${rim(base)} ${round(width + soften, 1)}deg`,
    `${rim(base)} ${round(180 - width - soften, 1)}deg`,
    `${white(opposite)} 180deg`,
    `${rim(base)} ${round(180 + width + soften, 1)}deg`,
    `${rim(base)} ${round(360 - width - soften, 1)}deg`,
    `${white(peak)} 360deg`,
  ]

  // 菲涅尔：沿边缘向内的一圈亮光，范围决定宽度，硬度决定是一条线还是一片柔光。
  const fresnelWidth = 0.5 + glass.fresnelRange * 0.1
  const fresnelSpread = fresnelWidth * (glass.fresnelHardness / 100)
  const fresnelBlur = fresnelWidth - fresnelSpread + 0.5
  const fresnelAlpha = clamp01((glass.fresnelFactor / 100) * (light ? 0.75 : 0.45))
  // 折射厚度与系数：越厚、折射越强，边缘那圈「透镜」越宽越亮。
  const lensWidth = glass.refThickness * 0.45
  const lensAlpha = clamp01((glass.refFactor - 1) * 0.05 + (light ? 0.05 : 0.025))
  // 色散：迎光一侧偏红、背光一侧偏蓝的细彩边。
  const rad = (angle * Math.PI) / 180
  const dx = round(Math.sin(rad), 2)
  const dy = round(-Math.cos(rad), 2)
  const fringe = clamp01((glass.refDispersion / 50) * 0.55)

  const shadowAlpha = clamp01((glass.shadowFactor / 100) * (light ? 1.1 : 2.2))
  return {
    '--lg-radius': `${glass.cornerRadius}px`,
    '--lg-corner-shape': `superellipse(${round(cornerShapeK(glass.superellipse))})`,
    '--lg-rim': `conic-gradient(from ${round(angle, 1)}deg, ${stops.join(', ')})`,
    '--lg-edge':
      `inset 0 0 ${round(fresnelBlur, 2)}px ${round(fresnelSpread, 2)}px ${white(fresnelAlpha)}, ` +
      `inset 0 0 ${round(lensWidth, 2)}px ${white(lensAlpha)}`,
    '--lg-fringe':
      fringe > 0
        ? `inset ${dx}px ${dy}px 0 -0.5px rgb(255 70 110 / ${round(fringe)}), inset ${-dx}px ${-dy}px 0 -0.5px rgb(70 150 255 / ${round(fringe)})`
        : '0 0 transparent',
    '--lg-shadow': `${glass.shadowOffsetX}px ${glass.shadowOffsetY}px ${glass.shadowExpand}px rgb(0 0 0 / ${round(shadowAlpha)})`,
    '--glass-blur': `${Math.min(60, Math.max(1, glass.blurRadius))}px`,
  }
}
