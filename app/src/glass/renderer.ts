import type { LiquidGlass } from '../api/settings'
import type { GradientPalette, Rgb } from './params'
import { BACKGROUND, BLUR, GLASS, MAX_BLUR_TAPS, MAX_SHAPES, VERTEX } from './shaders'

/** 一块玻璃：CSS 像素的矩形（相对画布左上角）、圆角与色调浓度。 */
export interface GlassShape {
  x: number
  y: number
  width: number
  height: number
  radius: number
  tintAlpha: number
}

/** 背景：内置渐变光斑，或已经解码好的图片。 */
export type GlassBackground =
  | { kind: 'gradient'; palette: GradientPalette; strength: number; /** 叠一层细斜纹（预览框用，方便看清折射）。 */ pattern?: boolean }
  | { kind: 'image'; image: TexImageSource; width: number; height: number }

export interface GlassScene {
  background: GlassBackground
  shapes: GlassShape[]
  glass: LiquidGlass
  tint: Rgb
}

type Uniforms<K extends string> = Record<K, WebGLUniformLocation | null>

interface Program<K extends string> {
  program: WebGLProgram
  uniforms: Uniforms<K>
}

interface Target {
  texture: WebGLTexture
  framebuffer: WebGLFramebuffer
  width: number
  height: number
}

function compile(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type)
  if (!shader) throw new Error('无法创建着色器')
  gl.shaderSource(shader, source)
  gl.compileShader(shader)
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(shader) ?? ''
    gl.deleteShader(shader)
    throw new Error(`着色器编译失败：${log}`)
  }
  return shader
}

function link<K extends string>(gl: WebGL2RenderingContext, fragment: string, names: readonly K[]): Program<K> {
  const program = gl.createProgram()
  if (!program) throw new Error('无法创建着色器程序')
  const vs = compile(gl, gl.VERTEX_SHADER, VERTEX)
  const fs = compile(gl, gl.FRAGMENT_SHADER, fragment)
  gl.attachShader(program, vs)
  gl.attachShader(program, fs)
  gl.bindAttribLocation(program, 0, 'a_position')
  gl.linkProgram(program)
  gl.deleteShader(vs)
  gl.deleteShader(fs)
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(`着色器链接失败：${gl.getProgramInfoLog(program) ?? ''}`)
  }
  const uniforms = {} as Uniforms<K>
  for (const name of names) uniforms[name] = gl.getUniformLocation(program, name)
  return { program, uniforms }
}

const SHAPE_UNIFORMS = ['u_rects', 'u_radii', 'u_count', 'u_roundness'] as const
type ShapeUniform = (typeof SHAPE_UNIFORMS)[number]

const BACKGROUND_UNIFORMS = [
  'u_resolution',
  'u_dpr',
  'u_kind',
  'u_base',
  'u_blobColor',
  'u_blobPlace',
  'u_blobStrength',
  'u_pattern',
  'u_image',
  'u_imageAspect',
  'u_shadowOffset',
  'u_shadowExpand',
  'u_shadowFactor',
  ...SHAPE_UNIFORMS,
] as const
type BackgroundUniform = (typeof BACKGROUND_UNIFORMS)[number]

const BLUR_UNIFORMS = ['u_source', 'u_lod', 'u_step', 'u_radius', 'u_weights'] as const
type BlurUniform = (typeof BLUR_UNIFORMS)[number]

const GLASS_UNIFORMS = [
  'u_resolution',
  'u_dpr',
  'u_bg',
  'u_blur',
  'u_refThickness',
  'u_refFactor',
  'u_refDistance',
  'u_refDispersion',
  'u_fresnelRange',
  'u_fresnelHardness',
  'u_fresnelFactor',
  'u_glareRange',
  'u_glareHardness',
  'u_glareFactor',
  'u_glareConvergence',
  'u_glareOpposite',
  'u_glareAngle',
  'u_blurEdge',
  'u_tint',
  'u_tintAlpha',
  ...SHAPE_UNIFORMS,
] as const
type GlassUniform = (typeof GLASS_UNIFORMS)[number]

/** 归一化的一维高斯权重（sigma = 半径 / 3，与参考实现一致）。 */
export function gaussianWeights(radius: number): Float32Array {
  const weights = new Float32Array(MAX_BLUR_TAPS + 1)
  const sigma = Math.max(radius, 1) / 3
  let sum = 0
  for (let i = 0; i <= radius; i += 1) {
    const w = Math.exp(-0.5 * ((i * i) / (sigma * sigma)))
    weights[i] = w
    sum += i === 0 ? w : w * 2
  }
  for (let i = 0; i <= radius; i += 1) weights[i] = (weights[i] ?? 0) / sum
  return weights
}

/** 模糊半径（设备像素）太大时降到哪一级 mipmap，使单方向采样数不超过上限。 */
export function blurLevel(radiusDevicePx: number): { lod: number; taps: number } {
  const lod = Math.max(0, Math.ceil(Math.log2(Math.max(radiusDevicePx, 1) / MAX_BLUR_TAPS)))
  const taps = Math.min(MAX_BLUR_TAPS, Math.max(1, Math.ceil(radiusDevicePx / 2 ** lod)))
  return { lod, taps }
}

/**
 * WebGL2 液态玻璃渲染器：背景（渐变 / 图片 + 投影）→ 降采样 + 两遍高斯模糊 → 玻璃合成。
 * 只在 `render()` 时画一帧，不跑动画循环；调用方在尺寸、形状、参数或背景变化时再调用。
 */
export class GlassRenderer {
  private readonly gl: WebGL2RenderingContext
  private readonly background: Program<BackgroundUniform>
  private readonly blur: Program<BlurUniform>
  private readonly glass: Program<GlassUniform>
  private readonly vao: WebGLVertexArrayObject
  private readonly buffer: WebGLBuffer
  private bgTarget: Target | null = null
  private tmpTarget: Target | null = null
  private blurTarget: Target | null = null
  private imageTexture: WebGLTexture | null = null
  private imageSource: TexImageSource | null = null
  private cssWidth = 0
  private cssHeight = 0
  private dpr = 1

  /** WebGL2 不可用或着色器编译失败时返回 null（调用方退回 CSS 近似）。 */
  static create(canvas: HTMLCanvasElement): GlassRenderer | null {
    const gl = canvas.getContext('webgl2', { alpha: false, antialias: false, depth: false, stencil: false })
    if (!gl) return null
    try {
      return new GlassRenderer(gl)
    } catch (error) {
      console.warn('液态玻璃渲染器初始化失败', error)
      return null
    }
  }

  private constructor(gl: WebGL2RenderingContext) {
    this.gl = gl
    this.background = link(gl, BACKGROUND, BACKGROUND_UNIFORMS)
    this.blur = link(gl, BLUR, BLUR_UNIFORMS)
    this.glass = link(gl, GLASS, GLASS_UNIFORMS)
    const vao = gl.createVertexArray()
    const buffer = gl.createBuffer()
    if (!vao || !buffer) throw new Error('无法创建顶点缓冲')
    this.vao = vao
    this.buffer = buffer
    gl.bindVertexArray(vao)
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer)
    // 一个盖满屏幕的大三角形。
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW)
    gl.enableVertexAttribArray(0)
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0)
    gl.bindVertexArray(null)
  }

  get lost(): boolean {
    return this.gl.isContextLost()
  }

  /** 画布的 CSS 尺寸与设备像素比；画布本身的像素尺寸在这里设置。 */
  setSize(cssWidth: number, cssHeight: number, dpr: number): void {
    const canvas = this.gl.canvas as HTMLCanvasElement
    const width = Math.max(1, Math.round(cssWidth * dpr))
    const height = Math.max(1, Math.round(cssHeight * dpr))
    this.cssWidth = cssWidth
    this.cssHeight = cssHeight
    this.dpr = dpr
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width
      canvas.height = height
    }
  }

  render(scene: GlassScene): void {
    const gl = this.gl
    if (gl.isContextLost() || this.cssWidth <= 0 || this.cssHeight <= 0) return
    const width = gl.drawingBufferWidth
    const height = gl.drawingBufferHeight
    const { glass } = scene
    this.bgTarget = this.ensureTarget(this.bgTarget, width, height, true)
    const { lod, taps } = blurLevel(glass.blurRadius * this.dpr)
    const blurWidth = Math.max(1, width >> lod)
    const blurHeight = Math.max(1, height >> lod)
    this.tmpTarget = this.ensureTarget(this.tmpTarget, blurWidth, blurHeight, false)
    this.blurTarget = this.ensureTarget(this.blurTarget, blurWidth, blurHeight, false)

    const shapes = scene.shapes.slice(0, MAX_SHAPES)
    const rects = new Float32Array(MAX_SHAPES * 4)
    const radii = new Float32Array(MAX_SHAPES)
    const tintAlpha = new Float32Array(MAX_SHAPES)
    shapes.forEach((shape, i) => {
      rects.set([shape.x, shape.y, shape.width, shape.height], i * 4)
      radii[i] = shape.radius
      tintAlpha[i] = shape.tintAlpha
    })
    const setShapes = (uniforms: Uniforms<ShapeUniform>) => {
      gl.uniform4fv(uniforms.u_rects, rects)
      gl.uniform1fv(uniforms.u_radii, radii)
      gl.uniform1i(uniforms.u_count, shapes.length)
      gl.uniform1f(uniforms.u_roundness, Math.max(2, glass.superellipse))
    }

    gl.bindVertexArray(this.vao)
    gl.disable(gl.BLEND)

    // 1. 背景 + 投影。
    const bg = this.background
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.bgTarget.framebuffer)
    gl.viewport(0, 0, width, height)
    gl.useProgram(bg.program)
    gl.uniform2f(bg.uniforms.u_resolution, width, height)
    gl.uniform1f(bg.uniforms.u_dpr, this.dpr)
    setShapes(bg.uniforms)
    gl.uniform2f(bg.uniforms.u_shadowOffset, glass.shadowOffsetX, glass.shadowOffsetY)
    gl.uniform1f(bg.uniforms.u_shadowExpand, Math.max(glass.shadowExpand, 0.1))
    gl.uniform1f(bg.uniforms.u_shadowFactor, glass.shadowFactor / 100)
    if (scene.background.kind === 'image') {
      const source = scene.background
      this.uploadImage(source.image)
      gl.activeTexture(gl.TEXTURE0)
      gl.bindTexture(gl.TEXTURE_2D, this.imageTexture)
      gl.uniform1i(bg.uniforms.u_kind, 1)
      gl.uniform1i(bg.uniforms.u_pattern, 0)
      gl.uniform1i(bg.uniforms.u_image, 0)
      gl.uniform1f(bg.uniforms.u_imageAspect, source.width / Math.max(source.height, 1))
    } else {
      const { palette, strength, pattern } = scene.background
      gl.uniform1i(bg.uniforms.u_kind, 0)
      gl.uniform1i(bg.uniforms.u_pattern, pattern ? 1 : 0)
      gl.uniform3fv(bg.uniforms.u_base, palette.base)
      gl.uniform3fv(bg.uniforms.u_blobColor, palette.blobs.flatMap((blob) => blob.color))
      gl.uniform3fv(bg.uniforms.u_blobPlace, palette.blobs.flatMap((blob) => [blob.x, blob.y, blob.radius]))
      gl.uniform1f(bg.uniforms.u_blobStrength, strength)
    }
    gl.drawArrays(gl.TRIANGLES, 0, 3)
    gl.bindTexture(gl.TEXTURE_2D, this.bgTarget.texture)
    gl.generateMipmap(gl.TEXTURE_2D)

    // 2. 两遍高斯模糊（大半径先从低一级 mipmap 取样，等于降采样）。
    const blur = this.blur
    const weights = gaussianWeights(taps)
    gl.useProgram(blur.program)
    gl.uniform1i(blur.uniforms.u_radius, taps)
    gl.uniform1fv(blur.uniforms.u_weights, weights)
    gl.uniform1i(blur.uniforms.u_source, 0)
    gl.activeTexture(gl.TEXTURE0)
    gl.viewport(0, 0, blurWidth, blurHeight)
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.tmpTarget.framebuffer)
    gl.bindTexture(gl.TEXTURE_2D, this.bgTarget.texture)
    gl.uniform1f(blur.uniforms.u_lod, lod)
    gl.uniform2f(blur.uniforms.u_step, 1 / blurWidth, 0)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.blurTarget.framebuffer)
    gl.bindTexture(gl.TEXTURE_2D, this.tmpTarget.texture)
    gl.uniform1f(blur.uniforms.u_lod, 0)
    gl.uniform2f(blur.uniforms.u_step, 0, 1 / blurHeight)
    gl.drawArrays(gl.TRIANGLES, 0, 3)

    // 3. 玻璃合成，直接画到屏幕。
    const main = this.glass
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    gl.viewport(0, 0, width, height)
    gl.useProgram(main.program)
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, this.bgTarget.texture)
    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, this.blurTarget.texture)
    gl.uniform1i(main.uniforms.u_bg, 0)
    gl.uniform1i(main.uniforms.u_blur, 1)
    gl.uniform2f(main.uniforms.u_resolution, width, height)
    gl.uniform1f(main.uniforms.u_dpr, this.dpr)
    setShapes(main.uniforms)
    gl.uniform1f(main.uniforms.u_refThickness, Math.max(glass.refThickness, 0.01))
    gl.uniform1f(main.uniforms.u_refFactor, Math.max(glass.refFactor, 1))
    gl.uniform1f(main.uniforms.u_refDistance, glass.refDistance)
    gl.uniform1f(main.uniforms.u_refDispersion, glass.refDispersion)
    gl.uniform1f(main.uniforms.u_fresnelRange, glass.fresnelRange)
    gl.uniform1f(main.uniforms.u_fresnelHardness, glass.fresnelHardness / 100)
    gl.uniform1f(main.uniforms.u_fresnelFactor, glass.fresnelFactor / 100)
    gl.uniform1f(main.uniforms.u_glareRange, glass.glareRange)
    gl.uniform1f(main.uniforms.u_glareHardness, glass.glareHardness / 100)
    gl.uniform1f(main.uniforms.u_glareFactor, glass.glareFactor / 100)
    gl.uniform1f(main.uniforms.u_glareConvergence, glass.glareConvergence / 100)
    gl.uniform1f(main.uniforms.u_glareOpposite, glass.glareOppositeFactor / 100)
    gl.uniform1f(main.uniforms.u_glareAngle, (glass.glareAngle * Math.PI) / 180)
    gl.uniform1f(main.uniforms.u_blurEdge, glass.blurEdge ? 1 : 0)
    gl.uniform3fv(main.uniforms.u_tint, scene.tint)
    gl.uniform1fv(main.uniforms.u_tintAlpha, tintAlpha)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
    gl.activeTexture(gl.TEXTURE0)
    gl.bindVertexArray(null)
  }

  dispose(): void {
    const gl = this.gl
    for (const target of [this.bgTarget, this.tmpTarget, this.blurTarget]) {
      if (target) {
        gl.deleteFramebuffer(target.framebuffer)
        gl.deleteTexture(target.texture)
      }
    }
    if (this.imageTexture) gl.deleteTexture(this.imageTexture)
    gl.deleteBuffer(this.buffer)
    gl.deleteVertexArray(this.vao)
    for (const { program } of [this.background, this.blur, this.glass]) gl.deleteProgram(program)
    this.bgTarget = this.tmpTarget = this.blurTarget = null
    this.imageTexture = null
    this.imageSource = null
  }

  private ensureTarget(current: Target | null, width: number, height: number, mipmapped: boolean): Target {
    if (current && current.width === width && current.height === height) return current
    const gl = this.gl
    if (current) {
      gl.deleteFramebuffer(current.framebuffer)
      gl.deleteTexture(current.texture)
    }
    const texture = gl.createTexture()
    const framebuffer = gl.createFramebuffer()
    if (!texture || !framebuffer) throw new Error('无法创建渲染目标')
    gl.bindTexture(gl.TEXTURE_2D, texture)
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, mipmapped ? gl.LINEAR_MIPMAP_LINEAR : gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
    gl.bindFramebuffer(gl.FRAMEBUFFER, framebuffer)
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, texture, 0)
    gl.bindFramebuffer(gl.FRAMEBUFFER, null)
    return { texture, framebuffer, width, height }
  }

  private uploadImage(image: TexImageSource): void {
    if (this.imageSource === image && this.imageTexture) return
    const gl = this.gl
    if (!this.imageTexture) this.imageTexture = gl.createTexture()
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, this.imageTexture)
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true)
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image)
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false)
    gl.generateMipmap(gl.TEXTURE_2D)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
    this.imageSource = image
  }
}
