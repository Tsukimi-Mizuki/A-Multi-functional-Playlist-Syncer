# 下载与同步的 C# 交叉验证夹具

`tests/csharp_download.rs` 用这里的文件比较 Rust 的 `DownloadService` / `SyncService` 与 C# 版 `DownloadService` / `MusicSyncService`。

| 文件 | 说明 |
|---|---|
| `scenarios.json` | 12 个场景：假的播放接口响应（地址 / 格式 / `level` / 试听 / 错误）、假 CDN（URL 到 `inputs/` 文件的映射，`@empty` 表示空内容，没登记的 URL 返回 404）、歌词、歌曲详情、预先存在的文件、下载选项；`mode` 为 `download` 或 `sync`（同步场景另有本地曲库文件） |
| `inputs/` | “下载内容”，全部合成，没有任何真实歌曲：`sample.mp3`（24 个静音帧）、`sample.flac`（`../audio/silence.flac` 的副本）、`sample.ncm`（C# `SyntheticNcmFile` 把 `sample.mp3` 与 `cover.png` 加密成的 NCM）、`cover.png`（1×1 PNG）、`cover.jpg`（伪随机字节，JPEG 文件头） |
| `expected.json` | 临时 C# 探针（直接引用 `LunaticPrincess.Core`，已删除、不入库）对同一批场景记录的结果：每首歌的成败 / 是否跳过 / 来源 / 输出文件名 / 消息，输出目录里的文件，以及 C# `AudioTagService` 读回的每个文件的标签（含封面 SHA-256），降档提示，出现过的阶段名 |

重新生成：用同样的 `scenarios.json` 写一个引用 `LunaticPrincess.Core` 的小程序，用真实的 `DownloadService`（`NcmDecoder` + `AudioTagService` + `LyricParser`，HTTP 换成按 URL 返回 `inputs/` 内容的假处理器）与 `MusicSyncService` 逐场景运行并按上面的形状输出即可。C# 的 HTTP 错误消息带原因短语（`HTTP 404 Not Found`），记录时已去掉 ` Not Found`，Rust 侧只有状态码。
