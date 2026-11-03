import type { Song } from '../api/playlists'

/** 歌曲时长 `m:ss`；未知时 `--:--`（与 C# 版一致）。 */
export function formatDuration(durationMs: number): string {
  if (!Number.isFinite(durationMs) || durationMs <= 0) return '--:--'
  const totalSeconds = Math.floor(durationMs / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes}:${String(seconds).padStart(2, '0')}`
}

/** 歌手名以 `/` 连接；没有歌手时「未知歌手」。 */
export function artistText(song: Song): string {
  const names = song.artists.map((artist) => artist.name).filter((name) => name.trim() !== '')
  return names.length > 0 ? names.join('/') : '未知歌手'
}

/**
 * 网易云图片地址：`http://` 升级为 `https://`（WebView 的 CSP 不允许混合内容），
 * 并带上 `?param=宽y高` 让服务端直接返回缩略图，避免为 42px 的封面下载几百 KB 的原图。
 */
export function imageSrc(url: string | null | undefined, size: number): string | null {
  if (!url) return null
  if (url.startsWith('data:')) return url
  const secure = url.startsWith('http://') ? `https://${url.slice('http://'.length)}` : url
  return secure.includes('?') ? secure : `${secure}?param=${size}y${size}`
}

/** 文件大小：`1.2 KB` / `3.4 MB`；未知时 `—`。 */
export function formatFileSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—'
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB']
  let value = bytes / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`
}

/** 序号「3/12」；没有序号时返回 null，只有序号时就是「3」。 */
export function formatPosition(number: number | null, total: number | null): string | null {
  if (number == null) return null
  return total != null ? `${number}/${total}` : String(number)
}
