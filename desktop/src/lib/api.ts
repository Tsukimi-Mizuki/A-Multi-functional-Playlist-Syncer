/** 本地 sidecar HTTP API 客户端（契约见 docs/TAURI_API.md）。 */
import { isTauri } from "./tauri";
import type {
  AppSettingsDto,
  BatchLyricsResultDto,
  BatchLyricsRunRequestDto,
  BatchLyricsScanDto,
  CompareItemDto,
  CompareResultDto,
  DownloadOptionsDto,
  DownloadRequestDto,
  DownloadSummaryDto,
  FsBrowseDto,
  HealthDto,
  JobCreatedDto,
  JobStatusDto,
  LibraryScanResultDto,
  LocalTrackDto,
  LogEntryDto,
  LyricsSearchRequestDto,
  LyricsSearchResultDto,
  NcmImportRequestDto,
  NcmImportSummaryDto,
  NcmScanResultDto,
  PlaylistDetailDto,
  PlaylistsDto,
  ProgressDto,
  QrLoginStartDto,
  QrLoginStatusDto,
  SessionDto,
  SongDto,
  SyncRequestDto,
  SyncSummaryDto,
  TagInfoDto,
  TagWriteRequestDto,
  UserDto,
} from "./types";

const DEFAULT_BASE = "http://127.0.0.1:18400";

function trimBase(base: string): string {
  return base.replace(/\/+$/, "");
}

/** 当前使用的 sidecar 基地址。 */
export function apiBase(): string {
  if (typeof window !== "undefined" && window.__LPP_API_BASE__) {
    return trimBase(window.__LPP_API_BASE__);
  }
  const fromEnv = import.meta.env.VITE_API_BASE;
  if (fromEnv) return trimBase(fromEnv);
  return DEFAULT_BASE;
}

export function setApiBase(base: string | null | undefined): void {
  if (typeof window === "undefined") return;
  window.__LPP_API_BASE__ = base && base.trim() ? trimBase(base.trim()) : undefined;
}

let initPromise: Promise<string> | null = null;

/**
 * 启动时解析 sidecar 地址：
 * 1. window.__LPP_API_BASE__（Tauri 注入或已有值）
 * 2. Tauri 命令 server_info / wait_server（后台等待，不阻塞 UI）
 * 3. VITE_API_BASE / 默认 http://127.0.0.1:18400
 */
export function initApiBase(): Promise<string> {
  if (!initPromise) initPromise = resolveApiBase();
  return initPromise;
}

async function resolveApiBase(): Promise<string> {
  if (typeof window !== "undefined") {
    window.addEventListener("lpp-api-ready", (event: Event) => {
      const detail = (event as CustomEvent<{ baseUrl?: string } | string | null>).detail;
      const base = typeof detail === "string" ? detail : detail && detail.baseUrl;
      if (base) window.__LPP_API_BASE__ = trimBase(base);
    });
  }

  if (typeof window === "undefined" || window.__LPP_API_BASE__) return apiBase();
  if (!isTauri()) return apiBase();

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    try {
      const info = await invoke<{ baseUrl?: string | null; managed?: boolean } | string | null>(
        "server_info",
      );
      const candidate = typeof info === "string" ? info : info && info.baseUrl ? info.baseUrl : null;
      if (candidate) {
        window.__LPP_API_BASE__ = trimBase(candidate);
        return apiBase();
      }
    } catch {
      /* sidecar 尚未就绪：继续等待 */
    }

    void (async () => {
      let base: string | null = null;
      try {
        base = await invoke<string | null>("wait_server");
      } catch {
        base = null;
      }
      if (base) window.__LPP_API_BASE__ = trimBase(base);
      window.dispatchEvent(new CustomEvent("lpp-api-ready", { detail: base ? { baseUrl: trimBase(base) } : null }));
    })();
  } catch {
    /* 纯浏览器环境：走 VITE_API_BASE / 默认值 */
  }

  return apiBase();
}

/** 统一的 API 错误：message 已按 { error: { message, code } } 解析。 */
export class ApiError extends Error {
  readonly status: number;
  readonly code?: string;

  constructor(message: string, status = 0, code?: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const url = apiBase() + path;
  const headers: Record<string, string> = { Accept: "application/json" };
  if (init && init.body) headers["Content-Type"] = "application/json";
  let response: Response;
  try {
    response = await fetch(url, {
      ...init,
      headers: { ...headers, ...((init && init.headers) as Record<string, string> | undefined) },
    });
  } catch {
    throw new ApiError("无法连接本地服务（" + apiBase() + "），请确认 sidecar 已启动后重试。", 0);
  }

  if (response.status === 204) return undefined as T;

  const raw = await response.text();
  let parsed: unknown = null;
  if (raw) {
    try {
      parsed = JSON.parse(raw);
    } catch {
      parsed = raw;
    }
  }

  if (!response.ok) {
    const payload = parsed as { error?: { message?: string; code?: string } } | null;
    const message = payload && payload.error && payload.error.message
      ? payload.error.message
      : "请求失败（HTTP " + response.status + "）";
    throw new ApiError(message, response.status, payload && payload.error ? payload.error.code : undefined);
  }

  return parsed as T;
}

function getRequest<T>(path: string): Promise<T> {
  return request<T>(path);
}

function postRequest<T>(path: string, body?: unknown): Promise<T> {
  return request<T>(path, {
    method: "POST",
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

function putRequest<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, { method: "PUT", body: JSON.stringify(body) });
}

/* ---------------------------------------------------------------- 健康检查 */

export function getHealth(): Promise<HealthDto> {
  return getRequest<HealthDto>("/api/health");
}

/* ---------------------------------------------------------------- 会话登录 */

export function getSession(): Promise<SessionDto> {
  return getRequest<SessionDto>("/api/session");
}

export function logout(): Promise<void> {
  return postRequest<void>("/api/logout");
}

export function startQrLogin(): Promise<QrLoginStartDto> {
  return postRequest<QrLoginStartDto>("/api/login/qr");
}

export function getQrLoginStatus(key: string): Promise<QrLoginStatusDto> {
  return getRequest<QrLoginStatusDto>("/api/login/qr/" + encodeURIComponent(key));
}

/* ------------------------------------------------------------------ 设置 */

export function getSettings(): Promise<AppSettingsDto> {
  return getRequest<AppSettingsDto>("/api/settings");
}

export function updateSettings(settings: AppSettingsDto): Promise<AppSettingsDto> {
  return putRequest<AppSettingsDto>("/api/settings", settings);
}

/* ------------------------------------------------------------------ 歌单 */

export function getPlaylists(): Promise<PlaylistsDto> {
  return getRequest<PlaylistsDto>("/api/playlists");
}

export function getPlaylist(id: number): Promise<PlaylistDetailDto> {
  return getRequest<PlaylistDetailDto>("/api/playlists/" + id);
}

/* ------------------------------------------------------------- 本地曲库 */

export function scanLibrary(directories: string[]): Promise<JobCreatedDto> {
  return postRequest<JobCreatedDto>("/api/library/scan", { directories });
}

export function compareLibrary(songs: SongDto[], tracks: LocalTrackDto[]): Promise<CompareResultDto> {
  return postRequest<CompareResultDto>("/api/library/compare", { songs, tracks });
}

/* ------------------------------------------------------------- 下载/同步 */

export function download(songs: SongDto[], options: DownloadOptionsDto): Promise<JobCreatedDto> {
  const body: DownloadRequestDto = { songs, options };
  return postRequest<JobCreatedDto>("/api/download", body);
}

export function sync(items: { song: SongDto; localPath: string | null }[], options: DownloadOptionsDto): Promise<JobCreatedDto> {
  const body: SyncRequestDto = { items, options };
  return postRequest<JobCreatedDto>("/api/sync", body);
}

/* --------------------------------------------------------------- NCM 解锁 */

export function scanNcm(directories: string[]): Promise<NcmScanResultDto> {
  return postRequest<NcmScanResultDto>("/api/ncm/scan", { directories });
}

export function importNcm(requestBody: NcmImportRequestDto): Promise<JobCreatedDto> {
  return postRequest<JobCreatedDto>("/api/ncm/import", requestBody);
}

/* --------------------------------------------------------- 歌词/标签编辑 */

export function readTags(path: string): Promise<TagInfoDto> {
  return postRequest<TagInfoDto>("/api/tags/read", { path });
}

export function writeTags(requestBody: TagWriteRequestDto): Promise<{ ok: boolean }> {
  return postRequest<{ ok: boolean }>("/api/tags/write", requestBody);
}

export function searchLyrics(requestBody: LyricsSearchRequestDto): Promise<LyricsSearchResultDto> {
  return postRequest<LyricsSearchResultDto>("/api/lyrics/search", requestBody);
}

/** 封面代理地址（服务端带 UA/Referer 转发图片）。 */
export function coverProxyUrl(url: string | null | undefined): string | null {
  if (!url) return null;
  if (url.startsWith("data:") || url.startsWith("blob:")) return url;
  if (url.startsWith("http://") || url.startsWith("https://")) {
    return apiBase() + "/api/cover?url=" + encodeURIComponent(url);
  }
  return null;
}

/* --------------------------------------------------------------- 批量歌词 */

export function scanBatchLyrics(directory: string): Promise<BatchLyricsScanDto> {
  return postRequest<BatchLyricsScanDto>("/api/batch-lyrics/scan", { directory });
}

export function runBatchLyrics(requestBody: BatchLyricsRunRequestDto): Promise<JobCreatedDto> {
  return postRequest<JobCreatedDto>("/api/batch-lyrics/run", requestBody);
}

/* ------------------------------------------------------------ 文件系统 */

export function browseFs(path?: string): Promise<FsBrowseDto> {
  const query = path ? "?path=" + encodeURIComponent(path) : "";
  return getRequest<FsBrowseDto>("/api/fs/browse" + query);
}

/* ---------------------------------------------------------------- 任务 */

export function getJob(jobId: string): Promise<JobStatusDto> {
  return getRequest<JobStatusDto>("/api/jobs/" + encodeURIComponent(jobId));
}

export function cancelJob(jobId: string): Promise<void> {
  return postRequest<void>("/api/jobs/" + encodeURIComponent(jobId) + "/cancel");
}

export interface JobHandlers {
  onProgress?: (progress: ProgressDto) => void;
  onDone?: (result: unknown) => void;
  onError?: (message: string) => void;
}

export interface JobSubscription {
  close: () => void;
}

function parseSseData(raw: string): unknown {
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return raw;
  }
}

function extractResult(payload: unknown): unknown {
  if (payload && typeof payload === "object" && "result" in (payload as Record<string, unknown>)) {
    return (payload as Record<string, unknown>)["result"];
  }
  return payload;
}

function extractError(payload: unknown): string {
  if (!payload) return "任务执行失败";
  if (typeof payload === "string") return payload;
  if (typeof payload === "object") {
    const record = payload as Record<string, unknown>;
    if (typeof record.error === "string") return record.error;
    if (typeof record.message === "string") return record.message;
    if (record.error && typeof record.error === "object") {
      const nested = record.error as Record<string, unknown>;
      if (typeof nested.message === "string") return nested.message;
    }
  }
  return "任务执行失败";
}

/** 订阅长任务 SSE 进度：progress / done / error。 */
export function subscribeJob(jobId: string, handlers: JobHandlers): JobSubscription {
  const url = apiBase() + "/api/jobs/" + encodeURIComponent(jobId) + "/events";
  let closed = false;
  let source: EventSource | null = null;

  const close = () => {
    if (closed) return;
    closed = true;
    if (source) source.close();
  };

  const finalizeFromStatus = async () => {
    if (closed) return;
    close();
    try {
      const job = await getJob(jobId);
      if (job.state === "done") handlers.onDone && handlers.onDone(job.result);
      else if (job.state === "failed") handlers.onError && handlers.onError(job.error || "任务执行失败");
      else if (job.state === "canceled") handlers.onError && handlers.onError("任务已取消");
      else handlers.onError && handlers.onError("任务进度连接已断开");
    } catch {
      handlers.onError && handlers.onError("任务进度连接已断开");
    }
  };

  try {
    source = new EventSource(url);
  } catch {
    void finalizeFromStatus();
    return { close };
  }

  source.addEventListener("progress", (event) => {
    if (closed) return;
    const payload = parseSseData((event as MessageEvent).data);
    if (payload && typeof payload === "object") {
      handlers.onProgress && handlers.onProgress(payload as ProgressDto);
    }
  });

  source.addEventListener("done", (event) => {
    if (closed) return;
    const payload = parseSseData((event as MessageEvent).data);
    close();
    handlers.onDone && handlers.onDone(extractResult(payload));
  });

  source.addEventListener("error", (event) => {
    if (closed) return;
    const data = (event as MessageEvent).data;
    if (data) {
      const payload = parseSseData(data);
      close();
      handlers.onError && handlers.onError(extractError(payload));
      return;
    }
    void finalizeFromStatus();
  });

  return { close };
}

/* ---------------------------------------------------------------- 日志 */

/** 订阅 /api/logs SSE；返回取消订阅函数。 */
export function subscribeLogs(onLog: (entry: LogEntryDto) => void): () => void {
  let source: EventSource | null = null;
  try {
    source = new EventSource(apiBase() + "/api/logs");
  } catch {
    return () => undefined;
  }
  source.addEventListener("log", (event) => {
    const payload = parseSseData((event as MessageEvent).data);
    if (payload && typeof payload === "object") {
      onLog(payload as LogEntryDto);
    }
  });
  return () => {
    if (source) source.close();
    source = null;
  };
}

/** 登录成功后由登录页直接注入用户信息。 */
export type { UserDto, CompareItemDto, DownloadSummaryDto, SyncSummaryDto, NcmImportSummaryDto, BatchLyricsResultDto };
