import { readCoverFile } from '../api/editor'
import type { BackgroundSource, GradientPreset } from '../api/settings'
import { imageSrc } from '../lib/format'
import { gradientPalette } from './params'
import type { GlassBackground } from './renderer'

/** 解析好的背景，以及要提示给用户的原因（图片读不出来、没有歌单封面而改用渐变时）。 */
export interface ResolvedBackground {
  background: GlassBackground
  note: string | null
}

export interface BackgroundRequest {
  source: BackgroundSource
  preset: GradientPreset
  imagePath: string
  /** 当前选中歌单的封面地址；没有为 null。 */
  coverUrl: string | null
  light: boolean
}

/** 光斑的浓度：浅色主题下文字是深色，光斑可以亮一些。 */
const gradientStrength = (light: boolean) => (light ? 0.75 : 0.62)

export function gradientBackground(preset: GradientPreset, light: boolean): GlassBackground {
  return { kind: 'gradient', palette: gradientPalette(preset, light), strength: gradientStrength(light) }
}

// 同一张图只解码一次：拖动参数滑块时背景不变，不必反复读文件、下载。
const images = new Map<string, Promise<HTMLImageElement>>()
const MAX_CACHED_IMAGES = 4

function decode(src: string, crossOrigin: boolean): Promise<HTMLImageElement> {
  const image = new Image()
  // 歌单封面来自网易云图床（返回 Access-Control-Allow-Origin: *），WebGL 要求跨域图片带 CORS 才能当纹理。
  if (crossOrigin) image.crossOrigin = 'anonymous'
  image.decoding = 'async'
  image.src = src
  return image.decode().then(() => image)
}

function cached(key: string, load: () => Promise<HTMLImageElement>): Promise<HTMLImageElement> {
  const hit = images.get(key)
  if (hit) return hit
  const pending = load()
  pending.catch(() => images.delete(key))
  images.set(key, pending)
  if (images.size > MAX_CACHED_IMAGES) {
    const oldest = images.keys().next().value
    if (oldest !== undefined) images.delete(oldest)
  }
  return pending
}

function asImage(image: HTMLImageElement): GlassBackground {
  return { kind: 'image', image, width: image.naturalWidth, height: image.naturalHeight }
}

function reason(error: unknown): string {
  return typeof error === 'string' ? error : error instanceof Error ? error.message : '无法读取'
}

/** 把背景设置变成可渲染的背景；任何失败都退回渐变光斑并给出原因，不抛错。 */
export async function resolveBackground(request: BackgroundRequest): Promise<ResolvedBackground> {
  const fallback = gradientBackground(request.preset, request.light)
  if (request.source === 'Image') {
    const path = request.imagePath.trim()
    if (path === '') return { background: fallback, note: '还没有选择背景图片，暂时使用渐变光斑。' }
    try {
      const image = await cached(`file:${path}`, async () => {
        const file = await readCoverFile(path)
        return decode(`data:${file.mimeType};base64,${file.base64}`, false)
      })
      return { background: asImage(image), note: null }
    } catch (error) {
      return { background: fallback, note: `背景图片读取失败（${reason(error)}），暂时使用渐变光斑。` }
    }
  }
  if (request.source === 'PlaylistCover') {
    const src = imageSrc(request.coverUrl, 800)
    if (!src) return { background: fallback, note: '当前歌单没有封面，暂时使用渐变光斑。' }
    try {
      const image = await cached(`cover:${src}`, () => decode(src, !src.startsWith('data:')))
      return { background: asImage(image), note: null }
    } catch {
      return { background: fallback, note: '歌单封面加载失败，暂时使用渐变光斑。' }
    }
  }
  return { background: fallback, note: null }
}
