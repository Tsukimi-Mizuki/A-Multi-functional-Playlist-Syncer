/**
 * 液态玻璃的 GLSL ES 3.0 着色器。
 *
 * 折射（按斯涅尔定律算边缘的偏折量）、色散、菲涅尔反射与按角度分布的高光的算法改编自
 * liquid-glass-studio（Copyright (c) 2024 Charles Yin，MIT，见 THIRD_PARTY_NOTICES.md），
 * 改动：多个界面面板的矩形并成一个 SDF、坐标统一用 CSS 像素、颜色提亮改用 OKLab、背景换成内置渐变 / 图片。
 *
 * 坐标约定：着色器里的 `p` 是 CSS 像素、原点在左上、y 向下（与 getBoundingClientRect 一致）；
 * 采样纹理时再换回 WebGL 的 uv（原点在左下）。
 */

/** 同时参与计算的玻璃形状上限（主界面是标题栏、侧栏、歌曲区、日志栏，预览框是三块）。 */
export const MAX_SHAPES = 8
/** 单方向高斯模糊的最大采样半径；更大的模糊半径先降采样到低一级 mipmap 再模糊。 */
export const MAX_BLUR_TAPS = 48

export const VERTEX = /* glsl */ `#version 300 es
in vec2 a_position;
out vec2 v_uv;
void main() {
  v_uv = (a_position + 1.0) * 0.5;
  gl_Position = vec4(a_position, 0.0, 1.0);
}
`

/** 形状 SDF：超椭圆圆角矩形（四角用 |x|^n + |y|^n = r^n），多个形状取并集。 */
const SHAPES = /* glsl */ `
uniform vec4 u_rects[${MAX_SHAPES}];
uniform float u_radii[${MAX_SHAPES}];
uniform int u_count;
uniform float u_roundness;

float cornerSdf(vec2 q, float r, float n) {
  q = abs(q);
  return pow(pow(q.x, n) + pow(q.y, n), 1.0 / n) - r;
}

float rectSdf(vec2 p, vec4 rect, float radius) {
  vec2 hs = rect.zw * 0.5;
  vec2 q = p - (rect.xy + hs);
  float r = min(radius, min(hs.x, hs.y));
  vec2 d = abs(q) - hs;
  if (r > 0.0 && d.x > -r && d.y > -r) {
    vec2 corner = sign(q) * (hs - vec2(r));
    return cornerSdf(q - corner, r, u_roundness);
  }
  return min(max(d.x, d.y), 0.0) + length(max(d, 0.0));
}

float sceneSdf(vec2 p, out int index) {
  float best = 1e9;
  index = -1;
  for (int i = 0; i < ${MAX_SHAPES}; i++) {
    if (i >= u_count) break;
    float d = rectSdf(p, u_rects[i], u_radii[i]);
    if (d < best) {
      best = d;
      index = i;
    }
  }
  return best;
}

float sceneSdf(vec2 p) {
  int ignored;
  return sceneSdf(p, ignored);
}
`

/** 背景：内置渐变光斑或图片（cover 铺满），再减去形状的投影。 */
export const BACKGROUND = /* glsl */ `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 fragColor;

uniform vec2 u_resolution;
uniform float u_dpr;
uniform int u_kind;
uniform vec3 u_base;
uniform vec3 u_blobColor[4];
uniform vec3 u_blobPlace[4];
uniform float u_blobStrength;
uniform int u_pattern;
uniform sampler2D u_image;
uniform float u_imageAspect;
uniform vec2 u_shadowOffset;
uniform float u_shadowExpand;
uniform float u_shadowFactor;
${SHAPES}

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

vec2 coverUv(vec2 uv, float canvasAspect, float imageAspect) {
  if (canvasAspect > imageAspect) {
    float s = imageAspect / canvasAspect;
    uv.y = uv.y * s + 0.5 - 0.5 * s;
  } else {
    float s = canvasAspect / imageAspect;
    uv.x = uv.x * s + 0.5 - 0.5 * s;
  }
  return uv;
}

void main() {
  vec2 size = u_resolution / u_dpr;
  vec2 p = vec2(gl_FragCoord.x, u_resolution.y - gl_FragCoord.y) / u_dpr;
  vec3 color;
  if (u_kind == 1) {
    color = texture(u_image, coverUv(v_uv, size.x / size.y, u_imageAspect)).rgb;
  } else {
    color = u_base;
    float diagonal = length(size);
    for (int i = 0; i < 4; i++) {
      vec2 center = u_blobPlace[i].xy * size;
      float radius = u_blobPlace[i].z * diagonal;
      float falloff = exp(-pow(length(p - center) / radius, 2.0) * 2.2);
      color = mix(color, u_blobColor[i], falloff * u_blobStrength);
    }
    // 预览框里叠一层细斜纹：平滑的渐变被折射后看不出变化，有纹理才看得清折射与色散。
    if (u_pattern == 1) {
      float stripe = smoothstep(0.4, 0.5, abs(fract((p.x + p.y) / 26.0) - 0.5));
      color = mix(color, min(color * 1.4 + 0.06, vec3(1.0)), stripe * 0.6);
    }
    // 抖动一点点，避免大面积渐变出现色带。
    color += (hash(gl_FragCoord.xy) - 0.5) / 255.0;
  }
  if (u_count > 0 && u_shadowFactor > 0.0) {
    float d = sceneSdf(p - u_shadowOffset);
    color -= vec3(exp(-abs(d) / u_shadowExpand) * 0.6 * u_shadowFactor);
  }
  fragColor = vec4(clamp(color, 0.0, 1.0), 1.0);
}
`

/** 可分离高斯模糊的一个方向；源纹理可指定 mipmap 层级做降采样。 */
export const BLUR = /* glsl */ `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 fragColor;

uniform sampler2D u_source;
uniform float u_lod;
uniform vec2 u_step;
uniform int u_radius;
uniform float u_weights[${MAX_BLUR_TAPS + 1}];

void main() {
  vec4 color = textureLod(u_source, v_uv, u_lod) * u_weights[0];
  for (int i = 1; i <= ${MAX_BLUR_TAPS}; i++) {
    if (i > u_radius) break;
    vec2 offset = u_step * float(i);
    color += (textureLod(u_source, v_uv + offset, u_lod) + textureLod(u_source, v_uv - offset, u_lod)) * u_weights[i];
  }
  fragColor = color;
}
`

/** 玻璃：形状外直接输出背景；形状内是模糊背景 + 边缘折射 / 色散 + 色调 + 菲涅尔 + 高光。 */
export const GLASS = /* glsl */ `#version 300 es
precision highp float;
#define PI 3.14159265359
in vec2 v_uv;
out vec4 fragColor;

uniform vec2 u_resolution;
uniform float u_dpr;
uniform sampler2D u_bg;
uniform sampler2D u_blur;
uniform float u_refThickness;
uniform float u_refFactor;
uniform float u_refDistance;
uniform float u_refDispersion;
uniform float u_fresnelRange;
uniform float u_fresnelHardness;
uniform float u_fresnelFactor;
uniform float u_glareRange;
uniform float u_glareHardness;
uniform float u_glareFactor;
uniform float u_glareConvergence;
uniform float u_glareOpposite;
uniform float u_glareAngle;
uniform float u_blurEdge;
uniform vec3 u_tint;
uniform float u_tintAlpha[${MAX_SHAPES}];
${SHAPES}

vec2 sceneNormal(vec2 p) {
  const float h = 0.5;
  vec2 g = vec2(
    sceneSdf(p + vec2(h, 0.0)) - sceneSdf(p - vec2(h, 0.0)),
    sceneSdf(p + vec2(0.0, h)) - sceneSdf(p - vec2(0.0, h))
  );
  float len = length(g);
  return len > 1e-5 ? g / len : vec2(0.0);
}

vec3 toLinear(vec3 c) {
  return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(0.04045, c));
}

vec3 toSrgb(vec3 c) {
  c = max(c, 0.0);
  return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c));
}

// OKLab（Björn Ottosson 公开的转换矩阵）。
vec3 toOklab(vec3 c) {
  vec3 lms = vec3(
    0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b,
    0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b,
    0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b
  );
  lms = pow(max(lms, 0.0), vec3(1.0 / 3.0));
  return vec3(
    0.2104542553 * lms.x + 0.7936177850 * lms.y - 0.0040720468 * lms.z,
    1.9779984951 * lms.x - 2.4285922050 * lms.y + 0.4505937099 * lms.z,
    0.0259040371 * lms.x + 0.7827717662 * lms.y - 0.8086757660 * lms.z
  );
}

vec3 fromOklab(vec3 c) {
  vec3 lms = vec3(
    c.x + 0.3963377774 * c.y + 0.2158037573 * c.z,
    c.x - 0.1055613458 * c.y - 0.0638541728 * c.z,
    c.x - 0.0894841775 * c.y - 1.2914855480 * c.z
  );
  lms = lms * lms * lms;
  return vec3(
    4.0767416621 * lms.x - 3.3077115913 * lms.y + 0.2309699292 * lms.z,
    -1.2684380046 * lms.x + 2.6097574011 * lms.y - 0.3413193965 * lms.z,
    -0.0041960863 * lms.x - 0.7034186147 * lms.y + 1.7076147010 * lms.z
  );
}

// 边缘一圈的强度：距边缘越近越接近 1；范围越大、越硬，亮带越宽越实。
float rim(float depth, float range, float hardness) {
  return clamp(pow(1.0 - depth / 1500.0 * pow(500.0 / max(range, 0.01), 2.0) + hardness, 5.0), 0.0, 1.0);
}

void main() {
  vec2 p = vec2(gl_FragCoord.x, u_resolution.y - gl_FragCoord.y) / u_dpr;
  vec2 uv = gl_FragCoord.xy / u_resolution;
  vec3 bg = texture(u_bg, uv).rgb;
  int index;
  float sdf = sceneSdf(p, index);
  if (index < 0 || sdf > 1.0) {
    fragColor = vec4(bg, 1.0);
    return;
  }

  float depth = max(-sdf, 0.0);
  vec2 n = sceneNormal(p);

  // 斯涅尔定律：越靠近边缘入射角越大，偏折越多；超过折射厚度的内部不偏折。
  float edge = 0.0;
  if (depth < u_refThickness) {
    float x = 1.0 - depth / u_refThickness;
    float thetaI = asin(clamp(x * x, -1.0, 1.0));
    float thetaT = asin(clamp(sin(thetaI) / u_refFactor, -1.0, 1.0));
    edge = max(-tan(thetaT - thetaI), 0.0);
  }

  vec3 color;
  if (edge <= 0.0) {
    color = texture(u_blur, uv).rgb;
  } else {
    // 往玻璃内侧取样：边缘看到的是被「拉」过来的内部内容。CSS 的 y 向下，uv 的 y 向上。
    vec2 shift = -n * edge * u_refDistance * 1414.0;
    vec2 duv = vec2(shift.x, -shift.y) * u_dpr / u_resolution;
    float spread = u_refDispersion * 0.02;
    float blurMix = u_blurEdge > 0.5 ? 1.0 : clamp(depth / u_refThickness, 0.0, 1.0);
    vec2 uvR = uv + duv * (1.0 + spread);
    vec2 uvG = uv + duv;
    vec2 uvB = uv + duv * (1.0 - spread);
    color = vec3(
      mix(texture(u_bg, uvR).r, texture(u_blur, uvR).r, blurMix),
      mix(texture(u_bg, uvG).g, texture(u_blur, uvG).g, blurMix),
      mix(texture(u_bg, uvB).b, texture(u_blur, uvB).b, blurMix)
    );
  }

  float tintAlpha = u_tintAlpha[index];
  color = mix(color, u_tint, tintAlpha);

  float fresnel = rim(depth, u_fresnelRange, u_fresnelHardness) * u_fresnelFactor;
  color = mix(color, mix(vec3(1.0), u_tint, tintAlpha * 0.3), fresnel * 0.7);

  // 高光：按法线方向与光照角度分布，正对光的一侧最亮，对侧按「对侧强度」减弱。
  vec2 ng = vec2(n.x, -n.y);
  float angle = atan(ng.y, ng.x);
  if (angle < 0.0) angle += 2.0 * PI;
  float glareAngle = (angle - PI / 4.0 + u_glareAngle) * 2.0;
  bool farSide = (glareAngle > PI * 1.5 && glareAngle < PI * 3.5) || glareAngle < PI * -0.5;
  float facing = (0.5 + sin(glareAngle) * 0.5) * (farSide ? 1.2 * u_glareOpposite : 1.2) * u_glareFactor;
  facing = clamp(pow(facing, 0.1 + u_glareConvergence * 2.0), 0.0, 1.0);
  float glare = facing * rim(depth, u_glareRange, u_glareHardness);
  if (glare > 0.0) {
    vec3 lab = toOklab(toLinear(color));
    float chroma = length(lab.yz);
    lab.x = min(lab.x + 1.5 * glare, 1.0);
    if (chroma > 1e-4) lab.yz *= (chroma + 0.086 * glare) / chroma;
    color = mix(color, clamp(toSrgb(fromOklab(lab)), 0.0, 1.0), glare);
  }

  // 边缘抗锯齿：在 ±0.75 px 内与背景过渡。
  color = mix(color, bg, smoothstep(-0.75, 0.75, sdf));
  fragColor = vec4(color, 1.0);
}
`
