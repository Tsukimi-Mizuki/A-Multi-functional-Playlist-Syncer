/** 与 docs/TAURI_API.md 契约一一对应的 DTO 类型定义。 */

export type JobState = "running" | "done" | "failed" | "canceled";

export interface ProgressDto {
  completed: number;
  total: number;
  stage: string;
  item: string;
  message: string | null;
  percent: number | null;
}

export interface JobStatusDto {
  id: string;
  state: JobState;
  progress: ProgressDto | null;
  result: unknown;
  error: string | null;
}

export interface JobCreatedDto {
  jobId: string;
}

export interface HealthDto {
  ok: boolean;
  version: string;
}

export interface UserDto {
  userId: number;
  nickname: string;
  avatarUrl: string | null;
  vipType: number;
}

export interface SessionDto {
  loggedIn: boolean;
  user: UserDto | null;
}

export interface QrLoginStartDto {
  key: string;
  qrContent: string;
  expiresAt: string;
}

export type QrLoginState = "Waiting" | "Scanned" | "Confirmed" | "Expired" | "Failed";

export interface QrLoginStatusDto {
  state: QrLoginState;
  message: string;
  user: UserDto | null;
}

export type AudioQuality =
  | "Standard"
  | "ExHigh"
  | "Lossless"
  | "HiRes"
  | "SpatialAudio"
  | "SurroundAudio"
  | "Master"
  | "AudioVivid"
  | "DolbyAtmos";

export type LyricsMode = "None" | "Line" | "Word";
export type SourcePriority = "NeteaseFirst" | "QQMusicFirst" | "NeteaseOnly" | "QQMusicOnly";
export type OnlineSource = "Netease" | "QQMusic";
export type ThemeMode = "Dark" | "Light" | "System";
export type RenderFrameRate =
  | "VSync"
  | "Fps30"
  | "Fps60"
  | "Fps90"
  | "Fps120"
  | "Fps144"
  | "Fps165"
  | "Fps240";

export interface AppSettingsDto {
  downloadDirectory: string;
  libraryDirectory: string;
  quality: AudioQuality;
  lyricsMode: LyricsMode;
  writeCover: boolean;
  overwriteExisting: boolean;
  skipExisting: boolean;
  fileNameTemplate: string;
  maxConcurrency: number;
  autoMatchLocal: boolean;
  tagSourcePriority: SourcePriority;
  lyricsSourcePriority: SourcePriority;
  ncmImportFolders: string[];
  ncmImportOutputDirectory: string;
  lyricsEditorSearchSource: OnlineSource;
  lyricsEditorPreferWordLyrics: boolean;
  matchThreshold: number;
  theme: ThemeMode;
  renderFrameRate: RenderFrameRate;
}

export interface ArtistDto {
  id: number;
  name: string;
}

export interface SongDto {
  id: number;
  name: string;
  displayName: string;
  artistText: string;
  artists: ArtistDto[];
  albumId: number;
  albumName: string;
  coverUrl: string | null;
  durationMs: number;
  trackNumber: number;
  fee: number;
}

export interface PlaylistSummaryDto {
  id: number;
  name: string;
  coverUrl: string | null;
  trackCount: number;
  creatorNickname: string | null;
  description: string | null;
}

export interface PlaylistDetailDto {
  id: number;
  name: string;
  coverUrl: string | null;
  description: string | null;
  creatorNickname: string | null;
  trackCount: number;
  isPartial: boolean;
  songs: SongDto[];
}

export interface PlaylistsDto {
  userId: number;
  playlists: PlaylistSummaryDto[];
}

export interface LocalTrackDto {
  path: string;
  title: string;
  artist: string;
  album: string;
  durationMs: number;
  format: string;
  bitrate: number;
  isNcm: boolean;
  hasCover: boolean;
  neteaseId: number | null;
}

export type LocalStatus = "Local" | "Ncm" | "Missing";

export interface CompareItemDto {
  songId: number;
  status: LocalStatus;
  localPath: string | null;
  matchedTitle: string | null;
  score: number | null;
}

export interface CompareResultDto {
  items: CompareItemDto[];
}

export interface LibraryScanResultDto {
  tracks: LocalTrackDto[];
}

export interface DownloadOptionsDto {
  targetDirectory: string;
  quality: string;
  lyricsMode: string;
  writeCover: boolean;
  overwriteExisting: boolean;
  skipExisting: boolean;
  fileNameTemplate: string;
  maxConcurrency: number;
}

export interface DownloadRequestDto {
  songs: SongDto[];
  options: DownloadOptionsDto;
}

export interface DownloadResultItemDto {
  songId: number;
  songName: string;
  success: boolean;
  outputPath: string | null;
  errorMessage: string | null;
  decryptedFromNcm: boolean;
  skipped: boolean;
}

export interface DownloadSummaryDto {
  successCount: number;
  skippedCount: number;
  failedCount: number;
  results: DownloadResultItemDto[];
}

export interface SyncRequestItemDto {
  song: SongDto;
  localPath: string | null;
}

export interface SyncRequestDto {
  items: SyncRequestItemDto[];
  options: DownloadOptionsDto;
}

export type SyncSource = "Downloaded" | "DecryptedNcm" | "Skipped" | "Failed";

export interface SyncResultItemDto {
  songId: number;
  songName: string;
  success: boolean;
  source: SyncSource;
  outputPath: string | null;
  message: string | null;
}

export interface SyncSummaryDto {
  successCount: number;
  failedCount: number;
  downloadedCount: number;
  decryptedCount: number;
  skippedCount: number;
  results: SyncResultItemDto[];
}

export interface NcmFileDto {
  path: string;
  title: string;
  artist: string;
  album: string;
  durationMs: number;
  format: string;
  bitrate: number;
  coverUrl: string | null;
}

export interface NcmScanResultDto {
  files: NcmFileDto[];
}

export interface OnlineMatchOptionsDto {
  tagSourcePriority: SourcePriority;
  lyricsSourcePriority: SourcePriority;
  overwriteExisting: boolean;
  fetchLyrics: boolean;
  preferWordLyrics: boolean;
}

export interface NcmImportRequestDto {
  files: string[];
  outputDirectory: string;
  overwrite: boolean;
  autoMatchOnline: boolean;
  matchOptions: OnlineMatchOptionsDto | null;
}

export interface NcmImportResultItemDto {
  sourcePath: string;
  outputPath: string | null;
  success: boolean;
  format: string | null;
  errorMessage: string | null;
  enrichmentMessage: string | null;
}

export interface NcmImportSummaryDto {
  results: NcmImportResultItemDto[];
  successCount: number;
  failedCount: number;
}

export interface TagInfoDto {
  path: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  year: string;
  trackNumber: string;
  genre: string;
  composer: string;
  comment: string;
  durationMs: number;
  bitrate: number;
  format: string;
  hasCover: boolean;
  coverBase64: string | null;
  lyrics: string | null;
  lyricsLength: number;
}

export interface TagWriteDto {
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  year: string;
  trackNumber: string;
  genre: string;
  composer: string;
  comment: string;
  neteaseId: number | null;
}

export interface TagWriteRequestDto {
  path: string;
  tags: TagWriteDto;
  lyrics: string | null;
  coverBase64: string | null;
}

export interface LyricsBundleDto {
  lrc: string | null;
  translatedLrc: string | null;
  wordLrc: string | null;
}

export interface OnlineLyricsMatchDto {
  source: OnlineSource;
  songId: number;
  title: string;
  artist: string;
  album: string;
  durationMs: number;
  score: number;
  coverUrl: string | null;
  lyrics: LyricsBundleDto;
}

export interface LyricsSearchRequestDto {
  title: string;
  artist: string | null;
  album: string | null;
  durationMs: number;
  source: OnlineSource;
  preferWordLyrics: boolean;
  matchThreshold: number;
}

export interface LyricsSearchResultDto {
  matches: OnlineLyricsMatchDto[];
}

export interface BatchLyricsOptionsDto {
  tagSourcePriority: SourcePriority;
  lyricsSourcePriority: SourcePriority;
  overwriteExisting: boolean;
  fetchLyrics: boolean;
  preferWordLyrics: boolean;
}

export interface BatchLyricsScanDto {
  files: string[];
}

export interface BatchLyricsRunRequestDto {
  directory: string;
  files: string[] | null;
  options: BatchLyricsOptionsDto;
}

export interface BatchLyricsItemDto {
  path: string;
  success: boolean;
  sourceLabel: string | null;
  message: string | null;
  matchedTitle: string | null;
  matchedArtist: string | null;
}

export interface BatchLyricsResultDto {
  items: BatchLyricsItemDto[];
}

export interface FsEntryDto {
  name: string;
  path: string;
}

export interface AudioFileEntryDto {
  name: string;
  path: string;
  size: number;
}

export interface FsBrowseDto {
  path: string;
  parent: string | null;
  directories: FsEntryDto[];
  audioFiles: AudioFileEntryDto[];
}

export interface LogEntryDto {
  time: string;
  level: "info" | "warn" | "error";
  message: string;
}

/** 任务进度表里的一项（前端本地状态）。 */
export interface TrackedJob {
  id: string;
  label: string;
  state: JobState;
  progress: ProgressDto | null;
  error: string | null;
  startedAt: number;
}

declare global {
  interface Window {
    __LPP_API_BASE__?: string;
  }
}
