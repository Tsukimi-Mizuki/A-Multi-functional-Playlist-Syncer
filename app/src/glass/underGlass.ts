import type { LiquidGlass } from '../api/settings'
import { currentAppearance, subscribeAppearance } from '../state/appearance'

/**
 * 对话框折射它下面的界面：给下一层（`#root`）套一个 SVG 位移滤镜，位移图按对话框的形状与折射参数生成——
 * 边缘像透镜一样把内容往里「拉」，并带色散彩边。（早期 WebView2 的透明窗口里没有 backdrop-filter，
 * 现在有了也不用：backdrop-filter 会在折射之后再模糊一遍，把折射抹掉。）
 *
 * - 只处理只有一个对话框的情况；叠放时下面那层已经被虚化，折射看不出来，不做。
 * - 滤镜放在 `#root`，背景雾度的模糊放在它里面的 `.app-shell`：先模糊再折射（与参考实现的顺序一致），
 *   而且 url() 滤镜不能插值，和可过渡的模糊放在同一个元素上会让模糊的淡入淡出失效。
 * - 对话框进场动画结束后才套上滤镜（动画期间对话框在缩放，位移图对不齐）。
 */

const FILTER_ID = 'lp-under-glass'
/** 位移图按 CSS 像素的一半生成，滤镜再拉伸回原尺寸：位移本来就是平滑的，省一半计算。 */
const MAP_SCALE = 0.5
const NS = 'http://www.w3.org/2000/svg'

/** 圆角矩形（四角是超椭圆）的有向距离：p 以矩形中心为原点，负数在内部。 */
function roundedSdf(px: number, py: number, hw: number, hh: number, r: number, n: number): number {
  const dx = Math.abs(px) - hw
  const dy = Math.abs(py) - hh
  if (r > 0 && dx > -r && dy > -r) {
    const cx = Math.abs(px) - (hw - r)
    const cy = Math.abs(py) - (hh - r)
    return (Math.abs(cx) ** n + Math.abs(cy) ** n) ** (1 / n) - r
  }
  return Math.min(Math.max(dx, dy), 0) + Math.hypot(Math.max(dx, 0), Math.max(dy, 0))
}

/** 斯涅尔定律算出的边缘偏折量（与 shaders.ts 一致）：深度 0 处最大，超过折射厚度为 0。 */
export function edgeFactor(depth: number, thickness: number, factor: number): number {
  if (depth >= thickness) return 0
  const x = 1 - depth / thickness
  const thetaI = Math.asin(Math.min(1, x * x))
  const thetaT = Math.asin(Math.min(1, Math.sin(thetaI) / Math.max(factor, 1)))
  return Math.max(0, -Math.tan(thetaT - thetaI))
}

export interface DisplacementMap {
  width: number
  height: number
  /** RGBA：R / G 是 x / y 位移，128 为不动。 */
  pixels: Uint8ClampedArray
  /** feDisplacementMap 的 scale（CSS 像素）。 */
  scale: number
}

/** 按对话框的尺寸（CSS 像素）与折射参数生成位移图；完全不折射时返回 null。 */
export function displacementMap(width: number, height: number, radius: number, glass: LiquidGlass): DisplacementMap | null {
  const strength = glass.refDistance * 1414
  const maxEdge = edgeFactor(0, glass.refThickness, glass.refFactor)
  const maxShift = strength * maxEdge
  if (maxShift < 0.25 || width < 2 || height < 2) return null
  const mapWidth = Math.max(1, Math.ceil(width * MAP_SCALE))
  const mapHeight = Math.max(1, Math.ceil(height * MAP_SCALE))
  const pixels = new Uint8ClampedArray(mapWidth * mapHeight * 4)
  const hw = width / 2
  const hh = height / 2
  const r = Math.min(radius, hw, hh)
  const n = Math.max(2, glass.superellipse)
  const scale = maxShift * 2
  const h = 0.5
  for (let y = 0; y < mapHeight; y += 1) {
    for (let x = 0; x < mapWidth; x += 1) {
      const i = (y * mapWidth + x) * 4
      pixels[i] = 128
      pixels[i + 1] = 128
      pixels[i + 3] = 255
      const px = (x + 0.5) / MAP_SCALE - hw
      const py = (y + 0.5) / MAP_SCALE - hh
      const d = roundedSdf(px, py, hw, hh, r, n)
      const depth = -d
      if (depth <= 0 || depth >= glass.refThickness) continue
      const gx = roundedSdf(px + h, py, hw, hh, r, n) - roundedSdf(px - h, py, hw, hh, r, n)
      const gy = roundedSdf(px, py + h, hw, hh, r, n) - roundedSdf(px, py - h, hw, hh, r, n)
      const length = Math.hypot(gx, gy)
      if (length < 1e-6) continue
      const edge = edgeFactor(depth, glass.refThickness, glass.refFactor)
      // 往内侧取样（法线朝外，所以取负）。
      const shiftX = (-gx / length) * edge * strength
      const shiftY = (-gy / length) * edge * strength
      pixels[i] = 128 + (shiftX / scale) * 255
      pixels[i + 1] = 128 + (shiftY / scale) * 255
    }
  }
  return { width: mapWidth, height: mapHeight, pixels, scale }
}

function toDataUrl(map: DisplacementMap): string | null {
  const canvas = document.createElement('canvas')
  canvas.width = map.width
  canvas.height = map.height
  const context = canvas.getContext('2d')
  if (!context) return null
  context.putImageData(new ImageData(map.pixels as Uint8ClampedArray<ArrayBuffer>, map.width, map.height), 0, 0)
  return canvas.toDataURL('image/png')
}

/** 只保留一个颜色通道、透明度取三分之一（三层相加后再乘回 3，见 filterMarkup）。 */
const CHANNEL = {
  R: '1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 0.33333 0',
  G: '0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 0.33333 0',
  B: '0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 0.33333 0',
}

function filterMarkup(
  url: string,
  rect: { x: number; y: number; width: number; height: number },
  region: { width: number; height: number },
  scale: number,
  dispersion: number,
): string {
  const map = `<feFlood flood-color="rgb(128,128,128)" result="neutral"/>` +
    `<feImage href="${url}" x="${rect.x}" y="${rect.y}" width="${rect.width}" height="${rect.height}" preserveAspectRatio="none" result="patch"/>` +
    `<feMerge result="map"><feMergeNode in="neutral"/><feMergeNode in="patch"/></feMerge>`
  const displace = (s: number, result: string) =>
    `<feDisplacementMap in="SourceGraphic" in2="map" scale="${s.toFixed(2)}" xChannelSelector="R" yChannelSelector="G" result="${result}"/>`
  let body: string
  if (dispersion <= 0) {
    body = displace(scale, 'out')
  } else {
    // 色散：红 / 绿 / 蓝各按不同的位移量取样，再把三个通道拼回去（与着色器里的 1 ± 0.02 × 色散增益一致）。
    const spread = dispersion * 0.02
    body =
      displace(scale * (1 + spread), 'dr') +
      `<feColorMatrix in="dr" type="matrix" values="${CHANNEL.R}" result="r"/>` +
      displace(scale, 'dg') +
      `<feColorMatrix in="dg" type="matrix" values="${CHANNEL.G}" result="g"/>` +
      displace(scale * Math.max(0, 1 - spread), 'db') +
      `<feColorMatrix in="db" type="matrix" values="${CHANNEL.B}" result="b"/>` +
      `<feComposite in="r" in2="g" operator="arithmetic" k2="1" k3="1" result="rg"/>` +
      `<feComposite in="rg" in2="b" operator="arithmetic" k2="1" k3="1" result="rgb"/>` +
      `<feComponentTransfer in="rgb"><feFuncR type="linear" slope="3"/><feFuncG type="linear" slope="3"/><feFuncB type="linear" slope="3"/></feComponentTransfer>`
  }
  return (
    `<filter id="${FILTER_ID}" x="0" y="0" width="${region.width}" height="${region.height}" ` +
    `filterUnits="userSpaceOnUse" primitiveUnits="userSpaceOnUse" color-interpolation-filters="sRGB">${map}${body}</filter>`
  )
}

let svg: SVGSVGElement | null = null
let frame = 0
let observed: HTMLElement | null = null
let lastKey = ''

function schedule(): void {
  if (frame === 0) frame = requestAnimationFrame(update)
}

const resize = typeof ResizeObserver === 'function' ? new ResizeObserver(() => schedule()) : null

function clear(shell: HTMLElement | null): void {
  shell?.style.removeProperty('filter')
  lastKey = ''
}

function update(): void {
  frame = 0
  const shell = document.getElementById('root')
  const modals = [...document.querySelectorAll<HTMLElement>('body > .modal-backdrop:not(.modal-leaving) > .modal')]
  const top = modals.length === 1 ? (modals[0] ?? null) : null
  if (observed !== top) {
    resize?.disconnect()
    if (top) resize?.observe(top)
    observed = top
  }
  const look = currentAppearance()
  if (!shell || !svg || !top || !look) {
    clear(shell)
    return
  }
  // 进场动画还在缩放对话框：等 animationend 再算。
  if (top.getAnimations().some((animation) => animation.playState === 'running')) {
    clear(shell)
    return
  }
  const box = top.getBoundingClientRect()
  const origin = shell.getBoundingClientRect()
  const radius = Number.parseFloat(getComputedStyle(top).borderTopLeftRadius) || 0
  const glass = look.liquidGlass
  const rect = { x: box.left - origin.left, y: box.top - origin.top, width: box.width, height: box.height }
  const key = JSON.stringify([rect, radius, origin.width, origin.height, glass.refDistance, glass.refThickness, glass.refFactor, glass.refDispersion, glass.superellipse])
  if (key === lastKey) return
  const map = displacementMap(box.width, box.height, radius, glass)
  const url = map ? toDataUrl(map) : null
  if (!map || !url) {
    clear(shell)
    return
  }
  svg.innerHTML = filterMarkup(url, rect, { width: origin.width, height: origin.height }, map.scale, glass.refDispersion)
  shell.style.filter = `url(#${FILTER_ID})`
  lastKey = key
}

function onAnimationEnd(event: AnimationEvent): void {
  if (event.target instanceof Element && event.target.classList.contains('modal')) schedule()
}

/** 启动对话框折射（主界面挂载时调用一次）；返回卸载函数。 */
export function installUnderGlass(): () => void {
  svg = document.createElementNS(NS, 'svg')
  svg.setAttribute('aria-hidden', 'true')
  svg.setAttribute('width', '0')
  svg.setAttribute('height', '0')
  // 不能 display: none，否则里面的滤镜失效。
  svg.style.position = 'absolute'
  svg.style.width = '0'
  svg.style.height = '0'
  svg.style.pointerEvents = 'none'
  document.body.appendChild(svg)
  // 对话框都挂在 body 下（createPortal），开关对话框就是 body 的子节点变化。
  const mutations = new MutationObserver(() => schedule())
  mutations.observe(document.body, { childList: true })
  document.addEventListener('animationend', onAnimationEnd, true)
  window.addEventListener('resize', schedule)
  const unsubscribe = subscribeAppearance(() => schedule())
  schedule()
  return () => {
    mutations.disconnect()
    document.removeEventListener('animationend', onAnimationEnd, true)
    window.removeEventListener('resize', schedule)
    unsubscribe()
    resize?.disconnect()
    observed = null
    cancelAnimationFrame(frame)
    frame = 0
    clear(document.getElementById('root'))
    svg?.remove()
    svg = null
  }
}
