# 在线匹配的 C# 交叉验证夹具

`tests/csharp_online.rs` 用这里的文件比较 Rust 的 `lyrics::scorer` / `lyrics::batch` / `online::*` 与 C# 版 `LyricsMatchScorer` / `BatchLyricsService` / `OnlineMusicSearchService` / `KugouLyricsSource` / `LrclibLyricsSource`。三份文件都由临时 C# 探针（直接引用 `LunaticPrincess.Core`，已删除、不入库）生成，**同时记录输入与 C# 真实运行的输出**；全部是合成数据，没有任何真实歌词。

| 文件 | 说明 |
|---|---|
| `scorer.json` | `cases`：1156 组（本地歌曲, 在线候选）及 C# 的四路分值 / 总分 / `rejected` / 原因 / `confident` / 四个子函数分数；`normalize`：334 个字符串的 `NormalizeText` / `CompactText` / `StripBrackets` / `CoreTitle` / `IsInstrumental`；`queries`：400 组 `BuildQueries`；`splits`：640 个 `SplitMultiValue`（歌手 / 标题两套分隔符）；`thresholds`：58 个阈值文本的 `TryParseThreshold`；`formatted`：24 个阈值的 `FormatThresholdPercent` |
| `batch.json` | 30 个批量选择场景。`input`：文件名、本地标签、选项、各来源（按关键词）的搜索结果、搜索错误、详情、备用词源（候选 / 抛错）、被关掉的备用词源；`expected`：C# `BatchLyricsService.FetchForFileAsync` 的结果、写入的每个标签字段、搜索调用序列、详情调用、各备用词源收到的关键词 |
| `sources.json` | 31 个词源场景。`input`：场景类型（QQ / 网易云搜索与详情、酷狗、LRCLIB）、参数、按 URL 子串路由的脚本化 HTTP 响应（可按顺序给多条，用于 429 重试 / 域名回退）；`expected`：C# 解析出的字段、错误文案、**发出的每个请求（`方法 完整URL`）** |

重新生成：写一个引用 `LunaticPrincess.Core` 的小程序，用真实的 `LyricsMatchScorer` / `BatchLyricsService`（替身：`IOnlineMusicSearchService`、`IAudioTagService`、`ILyricsSource`）/ `OnlineMusicSearchService` + `KugouLyricsSource` + `LrclibLyricsSource`（HTTP 换成按 URL 路由的假处理器）逐场景运行并按上面的形状输出即可。QRC 密文用 `QqTripleDes.Encrypt`（内部类，反射调用）生成，KRC 用固定 XOR 密钥生成。记录时已做的归一：C# 的 `HttpRequestException` 消息带原因短语（`HTTP 500 Internal Server Error`），去掉了短语；`JsonException` 的英文原文在比较时与 Rust 的中文说明统一成同一个标记；`pubtime` 用边界值 253402300799（越界值 C# 会抛异常，Rust 返回无年份，见 `docs/TAURI_REWRITE.md`）。

网易云搜索的差异（2026-11）：Rust 的主路径改成了匿名 eapi（C# 只有网页接口 `GET /api/search/get/web`，它在海外 IP 下返回密文）。`neteaseSearch` 场景因此在比较时让 eapi 请求一律返回 404 并且不计入请求序列，让 Rust 回退到网页接口，再与 C# 的黄金输出逐项比较（回退路径的解析与请求）；eapi 本身由 `netease::search` 的单测覆盖。
