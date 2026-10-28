/** 设置项与业务枚举的中文标签（沿用旧版 Avalonia 文案）。 */
import type {
  AudioQuality,
  LyricsMode,
  OnlineSource,
  RenderFrameRate,
  SourcePriority,
  ThemeMode,
} from "./types";

export interface Option<T> {
  value: T;
  label: string;
}

export const QUALITY_OPTIONS: Option<AudioQuality>[] = [
  { value: "Standard", label: "标准" },
  { value: "ExHigh", label: "极高 (HQ)" },
  { value: "Lossless", label: "无损 (SQ)" },
  { value: "HiRes", label: "高解析度无损 (Hi-Res)" },
  { value: "SpatialAudio", label: "高清臻音 (SpatialAudio)" },
  { value: "SurroundAudio", label: "沉浸环绕声 (Surround Audio)" },
  { value: "Master", label: "超清母带 (Master)" },
  { value: "AudioVivid", label: "臻音全景声 (Audio Vivid)" },
  { value: "DolbyAtmos", label: "杜比全景声 (Dolby Atmos)" },
];

export const LYRICS_MODE_OPTIONS: (Option<LyricsMode> & { description: string })[] = [
  { value: "None", label: "不写入歌词", description: "只下载音频与标签，不写歌词" },
  { value: "Line", label: "逐行歌词（LRC）", description: "写入普通 LRC 时间轴歌词" },
  { value: "Word", label: "逐字歌词（增强 LRC）", description: "优先写入逐字歌词，不支持时回退逐行" },
];

export const SOURCE_PRIORITY_OPTIONS: Option<SourcePriority>[] = [
  { value: "NeteaseFirst", label: "网易云音乐优先" },
  { value: "QQMusicFirst", label: "QQ 音乐优先" },
  { value: "NeteaseOnly", label: "仅网易云音乐" },
  { value: "QQMusicOnly", label: "仅 QQ 音乐" },
];

export const ONLINE_SOURCE_OPTIONS: Option<OnlineSource>[] = [
  { value: "Netease", label: "网易云音乐" },
  { value: "QQMusic", label: "QQ 音乐" },
];

export const THEME_OPTIONS: Option<ThemeMode>[] = [
  { value: "System", label: "跟随系统" },
  { value: "Dark", label: "深色（液态玻璃）" },
  { value: "Light", label: "浅色（液态玻璃）" },
];

export const FRAME_RATE_OPTIONS: Option<RenderFrameRate>[] = [
  { value: "VSync", label: "垂直同步（跟随显示器刷新率）" },
  { value: "Fps30", label: "30 FPS" },
  { value: "Fps60", label: "60 FPS" },
  { value: "Fps90", label: "90 FPS" },
  { value: "Fps120", label: "120 FPS" },
  { value: "Fps144", label: "144 FPS" },
  { value: "Fps165", label: "165 FPS" },
  { value: "Fps240", label: "240 FPS" },
];

export const CONCURRENCY_OPTIONS: Option<number>[] = Array.from({ length: 8 }, (_, index) => ({
  value: index + 1,
  label: index + 1 + " 个并发任务",
}));

export const FILE_NAME_TEMPLATE_PRESETS: Option<string>[] = [
  { value: "{artist} - {title}", label: "作者 - 标题" },
  { value: "{title} - {artist}", label: "标题 - 作者" },
  { value: "{artist} - {title} ({year})", label: "作者 - 标题（含年份）" },
];

export function labelOf<T>(options: Option<T>[], value: T | null | undefined): string {
  const found = options.find((option) => option.value === value);
  return found ? found.label : "";
}

export const LOCAL_STATUS_META: Record<
  string,
  { label: string; badge: string; dot: string; description: string }
> = {
  Local: {
    label: "本地已存在",
    badge: "border-emerald-300/25 bg-emerald-400/10 text-emerald-200",
    dot: "bg-emerald-400",
    description: "本地曲库中已找到匹配文件",
  },
  Ncm: {
    label: "NCM 待解密",
    badge: "border-amber-300/25 bg-amber-400/10 text-amber-200",
    dot: "bg-amber-400",
    description: "本地只有加密的 .ncm 文件，需要解密",
  },
  Missing: {
    label: "需下载",
    badge: "border-fuchsia-300/25 bg-fuchsia-400/10 text-fuchsia-200",
    dot: "bg-fuchsia-400",
    description: "本地曲库中没有这首歌曲",
  },
};
