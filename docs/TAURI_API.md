# Tauri 桌面端 API 契约（LunaticPrincess.Server sidecar）

Vue 前端通过 Tauri 启动的本地 HTTP sidecar（.NET，复用 LunaticPrincess.Core）访问全部业务能力。

- 监听地址：`http://127.0.0.1:{port}`（Rust 侧随机端口，前端通过 `invoke("server_info")` 或 `window.__LPP_API_BASE__` 获取；开发模式下回退 `VITE_API_BASE`，默认 `http://127.0.0.1:18400`）。
- 全部请求/响应为 `application/json; charset=utf-8`（图片代理除外）。
- 时间字符串为 ISO-8601；时长单位为毫秒。
- 错误响应统一为 `{ "error": { "message": string, "code"?: string } }`，HTTP 4xx/5xx。

## 长任务（扫描/下载/同步/NCM 导入/批量歌词）

`POST` 立即返回 `202 { "jobId": "..." }`，随后：

- `GET /api/jobs/{jobId}` → `{ "id": string, "state": "running" | "done" | "failed" | "canceled", "progress": ProgressDto | null, "result": any | null, "error": string | null }`
- `GET /api/jobs/{jobId}/events` → SSE 流：`event: progress`（`data: ProgressDto`）、`event: done`（`data: { result }`）、`event: error`；连接结束即任务结束。
- `POST /api/jobs/{jobId}/cancel` → `204`

`ProgressDto`：`{ "completed": number, "total": number, "stage": string, "item": string, "message": string | null, "percent": number | null }`

## 会话与登录

- `GET /api/session` → `{ "loggedIn": boolean, "user": { "userId": number, "nickname": string, "avatarUrl": string | null, "vipType": number } | null }`
- `POST /api/login/qr` → `{ "key": string, "qrContent": string, "expiresAt": string }`（二维码由前端用 `qrcode` 生成）
- `GET /api/login/qr/{key}` → `{ "state": "Waiting" | "Scanned" | "Confirmed" | "Expired" | "Failed", "message": string, "user": UserDto | null }`
- `POST /api/logout` → `204`

## 设置

- `GET /api/settings` → `AppSettingsDto`
- `PUT /api/settings` → 返回规范化后的 `AppSettingsDto`

`AppSettingsDto`（与 Core `AppSettings` 同名字段，camelCase）：`downloadDirectory`、`libraryDirectory`、`quality`（枚举名：`Standard|ExHigh|Lossless|HiRes|SpatialAudio|SurroundAudio|Master|AudioVivid|DolbyAtmos`）、`lyricsMode`（`None|Line|Word`）、`writeCover`、`overwriteExisting`、`skipExisting`、`fileNameTemplate`、`maxConcurrency`、`autoMatchLocal`、`tagSourcePriority`、`lyricsSourcePriority`（`NeteaseFirst|QQMusicFirst|NeteaseOnly|QQMusicOnly`）、`ncmImportFolders`（string[]）、`ncmImportOutputDirectory`、`lyricsEditorSearchSource`（`Netease|QQMusic`）、`lyricsEditorPreferWordLyrics`、`matchThreshold`、`theme`（`Dark|Light|System`）、`renderFrameRate`。

## 歌单

- `GET /api/playlists` → `{ "userId": number, "playlists": PlaylistSummaryDto[] }`
- `GET /api/playlists/{id}` → `PlaylistDetailDto`

`PlaylistSummaryDto`：`{ "id": number, "name": string, "coverUrl": string | null, "trackCount": number, "creatorNickname": string | null, "description": string | null }`

`PlaylistDetailDto`：`{ "id", "name", "coverUrl", "description", "creatorNickname", "trackCount", "isPartial": boolean, "songs": SongDto[] }`

`SongDto`：`{ "id": number, "name": string, "displayName": string, "artistText": string, "artists": [{ "id", "name" }], "albumId": number, "albumName": string, "coverUrl": string | null, "durationMs": number, "trackNumber": number, "fee": number }`

## 本地曲库

- `POST /api/library/scan` body `{ "directories": string[] }` → job；result `{ "tracks": LocalTrackDto[] }`
- `POST /api/library/compare` body `{ "songs": SongDto[], "tracks": LocalTrackDto[] }` → `{ "items": [{ "songId": number, "status": "Local" | "Ncm" | "Missing", "localPath": string | null, "matchedTitle": string | null, "score": number | null }] }`（纯本地计算，同步返回）

`LocalTrackDto`：`{ "path", "title", "artist", "album", "durationMs", "format", "bitrate", "isNcm": boolean, "hasCover": boolean, "neteaseId": number | null }`

## 下载 / 同步

- `POST /api/download` body `{ "songs": SongDto[], "options": { "targetDirectory": string, "quality": string, "lyricsMode": string, "writeCover": boolean, "overwriteExisting": boolean, "skipExisting": boolean, "fileNameTemplate": string, "maxConcurrency": number } }` → job；result `DownloadSummaryDto`
- `POST /api/sync` body `{ "items": [{ "song": SongDto, "localPath": string | null }], "options": { ...同下载 } }` → job；result `SyncSummaryDto`

`DownloadSummaryDto`：`{ "successCount", "skippedCount", "failedCount", "results": [{ "songId", "songName", "success", "outputPath", "errorMessage", "decryptedFromNcm", "skipped" }] }`

`SyncSummaryDto`：`{ "successCount", "failedCount", "downloadedCount", "decryptedCount", "skippedCount", "results": [{ "songId", "songName", "success", "source": "Downloaded|DecryptedNcm|Skipped|Failed", "outputPath", "message" }] }`

## NCM 解锁

- `POST /api/ncm/scan` body `{ "directories": string[] }` → `{ "files": [{ "path", "title", "artist", "album", "durationMs", "format", "bitrate", "coverUrl": null }] }`（同步）
- `POST /api/ncm/import` body `{ "files": string[], "outputDirectory": string, "overwrite": boolean, "autoMatchOnline": boolean, "matchOptions": { "tagSourcePriority", "lyricsSourcePriority", "overwriteExisting", "fetchLyrics", "preferWordLyrics" } | null }` → job；result `{ "results": [{ "sourcePath", "outputPath", "success", "format", "errorMessage", "enrichmentMessage" }], "successCount", "failedCount" }`

## 歌词 / 标签编辑器

- `POST /api/tags/read` body `{ "path": string }` → `TagInfoDto`
- `POST /api/lyrics/search` body `{ "title": string, "artist": string | null, "album": string | null, "durationMs": number, "source": "Netease" | "QQMusic", "preferWordLyrics": boolean, "matchThreshold": number }` → `{ "matches": [{ "source", "songId", "title", "artist", "album", "durationMs", "score", "coverUrl", "lyrics": { "lrc", "translatedLrc", "wordLrc" } }] }`
- `POST /api/tags/write` body `{ "path": string, "tags": TagWriteDto, "lyrics": string | null, "coverBase64": string | null }` → `{ "ok": true }`
- `GET /api/cover?url={encodedUrl}` → 图片字节（服务端代理，带 UA/Referer）

`TagInfoDto`：`{ "path", "title", "artist", "album", "albumArtist", "year", "trackNumber", "genre", "composer", "comment", "durationMs", "bitrate", "format", "hasCover", "coverBase64": string | null, "lyrics": string | null, "lyricsLength": number }`

`TagWriteDto`：`{ "title", "artist", "album", "albumArtist", "year", "trackNumber", "genre", "composer", "comment", "neteaseId": number | null }`

## 批量歌词

- `POST /api/batch-lyrics/scan` body `{ "directory": string }` → `{ "files": string[] }`
- `POST /api/batch-lyrics/run` body `{ "directory": string, "files": string[] | null, "options": { "tagSourcePriority", "lyricsSourcePriority", "overwriteExisting", "fetchLyrics", "preferWordLyrics" } }` → job；result `{ "items": [{ "path", "success", "sourceLabel", "message", "matchedTitle", "matchedArtist" }] }`

## 文件系统与日志

- `GET /api/fs/browse?path={path}` → `{ "path", "parent": string | null, "directories": [{ "name", "path" }], "audioFiles": [{ "name", "path", "size" }] }`（前端目录选择器的回退方案）
- `GET /api/logs` → SSE：`event: log`，`data: { "time": string, "level": "info" | "warn" | "error", "message": string }`
- `GET /api/health` → `{ "ok": true, "version": string }`
