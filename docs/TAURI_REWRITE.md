# Tauri 重写计划

`GUI-Tauri` 分支正在把 Lunatic Princess **彻底重写**：业务层改用 Rust，界面改用 Tauri 2 + React + TypeScript。
C# / Avalonia 版（`src/`、`tests/`、`LunaticPrincess.sln`、`build.ps1` 的 .NET 部分）在整个重写期间**原样保留**，供逐模块对照，直到最后一个模块才删除。

## 目标

- 业务能力与 C# 版一致（登录、歌单、本地对比、下载、NCM 解密、歌词/标签/封面、在线匹配），数据文件（`settings.json`、`session.dat`）互相兼容，用户可无缝切换。
- 核心逻辑全部在平台无关的 Rust 库 `lp-core` 中，带单测；Tauri 外壳只做命令转发、窗口与插件。
- 视觉沿用现有「液态玻璃」基线（深色 / 浅色 / 跟随系统），无边框自绘标题栏。
- 仍然只面向 Windows 10/11 x64；不绕过版权与会员限制。

## 技术栈与版本

| 项 | 版本 / 说明 |
|---|---|
| Rust | 1.99.1 stable（edition 2024），需要 clippy / rustfmt |
| Tauri | crate `tauri` 2.12.5、`tauri-build` 2.7.4；npm `@tauri-apps/cli` / `@tauri-apps/api` 2.12.4 |
| 前端 | React 19.3、TypeScript 7.0、Vite 8.3、`@vitejs/plugin-react` 6.1，TS 严格模式 |
| 包管理 | pnpm 11（锁文件 `app/pnpm-lock.yaml`） |
| Rust 公共依赖 | `serde` 1、`thiserror` 2 |
| 运行环境 | WebView2、VS 2022 Build Tools（C++） |

不引入 UI 组件库（后续有需要再定）。

## 目录布局

```
app/
  package.json  pnpm-lock.yaml  vite.config.ts  tsconfig*.json  index.html
  src/                    # React + TS 前端（components / api / styles）
  Cargo.toml              # Cargo workspace：members = ["src-tauri", "crates/lp-core"]
  crates/lp-core/         # 平台无关业务库，模块与 C# Core 命名空间一一对应
  src-tauri/              # Tauri 外壳：命令、窗口、capabilities、图标；二进制名 LunaticPrincess
```

`lp-core` 模块与 C# 目录的对应关系：

| Rust 模块 | C# 目录 |
|---|---|
| `paths` | `Core/AppPaths.cs` |
| `settings` | `Core/Settings/`（`AppSettings`、`JsonSettingsStore`） |
| `models` | `Core/Models/` 中的设置用枚举，以及 `ThemeMode`、`LyricsSourceKind`；模块 9 补上 `AudioQuality` 的接口映射（`models/audio_quality.rs`，对应 `AudioQualityExtensions`） |
| `session` | `Core/Models/NeteaseSession.cs`、`Core/Storage/`（`ProtectedSessionStore`、`SessionAutoSaver`） |
| `auth` | 原来放在 `MainWindowViewModel` / `QrLoginViewModel` 里的登录编排（启动校验、续期、扫码落盘、退出、运行期复核） |
| `logs` | `MainWindowViewModel` 里的运行日志列表（400 条、内存） |
| `qr` | `QrLoginViewModel.RenderQrCode`（QRCoder → `qrcode` SVG） |
| `netease` | `Core/Netease/`（`models` / `mappers` / `playlists` / `songs` / `lyrics` 对应 `NeteaseModels`、`NeteaseMappers`、`NeteaseApiClient.Playlists/Songs/Lyrics`） |
| `playlist` | 原来放在 `MainWindowViewModel` 里的歌单加载编排（取列表 / 曲目、写日志、301 时走登录复核） |
| `ncm` | `Core/Ncm/`（模块 6：`client_comment` = `NeteaseClientComment`；模块 7：`decoder` = `NcmDecoder` / `INcmDecoder`） |
| `lyrics` | `Core/Lyrics/`（模块 5：`models` / `parser` / `qrc` / `krc` / `service`；模块 10a：`scorer` = `LyricsMatchScorer`、`batch` = `BatchLyricsService` + 任务编排 + 在线补全挂钩） |
| `tags` | `Core/Tags/`（模块 6：`model` / `service` / `text` / `view`；`service` 对应 `AudioTagService`） |
| `utilities` | `Core/Utilities/`（模块 6：`audio_file_types`；模块 7：`error_text`、`file_name_sanitizer`、`cancel`） |
| `library` | `Core/Library/`（模块 8：`track` / `scanner` / `matcher` / `text` / `view` 对应 `ILocalLibraryService.cs`、`LocalLibraryService`、`SongMatcher`；`manager` 对应 `MainWindowViewModel` 里的扫描缓存与对比） |
| `download` | `Core/Download/`（模块 7：`ncm_import` = `NcmImportService`；模块 9：`media` = `MediaFileHelper`、`fetch` = 其中的 HTTP 下载、`service` = `DownloadService`、`pool` = 并发上限的任务池、`task` = `MainWindowViewModel` 里下载 / 同步命令的编排） |
| `sync` | `Core/Sync/`（模块 9：`service` = `MusicSyncService`） |
| `online` | `Core/Online/`（模块 10a：`model` / `service`（`OnlineMusicSearchService`）/ `qq` / `kugou` / `lrclib` / `net` / `manager` / `view`）；`netease::search` = `NeteaseApiClient.Search.cs` |
| `editor` | 模块 10b：`LyricsEditorViewModel` 里记住来源与歌词格式、`HandleDroppedTargetsAsync` / `LoadDroppedPathsAsync` 的拖入解析、`Program.GetStartupLyricsFile`、`ElevationHelper`；另有封面图片文件读取 |
| `error` | `Core/Utilities/ErrorText.cs` 的错误类型部分（判定与文案在 `utilities::error_text`） |

构建产物（`app/target`、`app/dist`、`app/node_modules`、`app/src-tauri/gen/schemas`）都已 gitignore。

## 构建与测试

```powershell
.\build.ps1 -Tauri          # pnpm install → cargo fmt --check → clippy -D warnings → cargo test → pnpm tauri build --no-bundle
.\build.ps1 -Tauri -Test    # 只跑检查与测试，不打包
```

产物：`app\target\release\LunaticPrincess.exe`（单个 exe，前端已内嵌）。安装包 / zip 留到模块 12。

手动命令（在 `app/` 下）：

```powershell
pnpm install
cargo fmt --check; cargo clippy --workspace -- -D warnings; cargo test --workspace
pnpm tauri dev       # 开发（Vite 固定端口 1420）
pnpm build           # 只构建前端（tsc + vite）
```

开发注意事项：

- 新开的 shell 若找不到 `cargo` / `pnpm`，先刷新 PATH：`$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')`。
- **仓库位于 Edge 下载文件夹（低完整性级别）时的行为**：见下文「已知环境问题」。构建脚本会把 `TMP` 指到工作区 `.tmp`，原因相同。
- 窗口背景：Tauri 窗口为透明无边框，Rust 侧在 `setup` 中给窗口铺 Mica，失败回退 Acrylic；前端 `body` 保持透明，由半透明 token 叠加。

## 模块清单与进度

| # | 模块 | 内容 | 状态 |
|---|---|---|---|
| 1 | 项目骨架 | Cargo workspace、`lp-core` 空骨架、Tauri 外壳、自绘标题栏、主题 token、`build.ps1 -Tauri` | 完成 |
| 2 | 基础层 | 数据目录解析、`settings.json`、DPAPI `session.dat`（与 C# 版兼容） | 完成 |
| 3 | 登录 | weapi 加密、HTTP、Cookie 合并与续期、扫码登录 UI | 完成（续期效果待真机长期观察） |
| 4 | 歌单 | 歌单列表与详情（trackIds + 分批 song/detail）、歌单侧栏与歌曲表格 | 完成 |
| 5 | 歌词解析 | LRC / 增强 LRC / YRC / QRC（3DES 变体）/ KRC、网易云歌词接口、歌词预览 | 完成 |
| 6 | 标签读写 | MP3 / FLAC / M4A / OGG / OPUS / WAV 标签、封面、歌词读写（lofty），网易云 ID 与客户端 163 key，标签查看对话框 | 完成 |
| 7 | NCM 解密与导入 | `.ncm` 流式解密、元数据 / 封面 / 网易云 ID 回写、导入对话框（在线补全挂钩待模块 10） | 完成 |
| 8 | 本地曲库与对比 | 曲库扫描（并行读标签）、与歌单一对一匹配、状态列 / 统计 / 对比与重新扫描按钮 | 完成 |
| 9 | 下载与同步 | 九档音质取链下载（逐级降档、试听判失败、NCM 自动解密）、一键同步（跳过 / NCM 解密 / 下载三分支）、多选 + 工具栏 + 逐行进度 | 完成（真实下载需用户真机验证） |
| 10 | 在线匹配 | 网易云 / QQ / 酷狗 / LRCLIB、歌词与标签编辑器、批量匹配、拖放与启动参数 | 完成：10a 后端核心（打分 / 搜索与备用词源 / 批量服务 / 在线补全接入 / 命令与 TS 类型）；10b 编辑器与批量窗口界面、拖放、`--edit` / `--lyrics` 启动参数（网易云匿名搜索不可用见「待修问题」） |
| 11 | 设置与外观 | 设置窗口、主题切换、帧率等 | 待做 |
| 12 | CI 打包、文档、删除 C# | 安装包 / zip、GitHub Actions、更新 AGENTS.md 与 README、删除 C# 代码 | 待做 |

## 待修问题

以下问题已确认、暂不处理；**用户决定等全部模块完成后再统一修复**，不要在中途模块里顺手改。

- **`/weapi/login/token/refresh` 实测返回 `code=400`**：桌面端扫码得到的会话调用续期接口时服务端拒绝（日志 `登录态续期未成功（续期登录状态失败：接口返回 code=400）`），请求与 C# 完全一致，所以 C# v1.3 同样受影响。续期是否真的生效尚未证实；登录态本身有效（`account/get` 成功）。需要长期观察 `MUSIC_U` 是否到期掉线，必要时改查 `eapi` 续期端点。详见「模块 3」的已知问题。
- **网易云搜索当前匿名不可用**（模块 10a 联网探针发现，2026-11-04）：C# 与这里用的 `/api/search/get/web` 现在返回 `{"result":"<十六进制密文>"}`（C# 把它当成 0 条结果）；旧的 `/api/search/get`、`/weapi/search/get` 返回与关键词无关的歌曲；`/weapi/cloudsearch/get/web` 匿名返回 `code=50000005`。登录态下 `cloudsearch` 是否可用、或是否要改走 `eapi`，需要用户用真实账号验证后再定；目前 `search_songs` 遇到密文会报明确错误，QQ 音乐不受影响（当前真正可用的主词源）。C# v1.3 同样受影响。详见“模块 10a”。

## 模块 10a（在线匹配：后端核心）实现记录

> 模块 10 分两步：**10a（本节）= `lp-core` + Tauri 命令 + TS 类型**；10b = 歌词与标签编辑器、批量获取窗口的界面、拖放与启动参数，见下一节。两步都已完成，模块 10 完成。

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `netease::search` | `NeteaseApiClient.Search.cs` | `NeteaseClient::search_songs(keyword, limit)`：`GET /api/search/get/web`，数量夹在 1–50，空白关键词不发请求，丢弃 id 非法或歌名为空白的项；**`result` 是字符串（密文）时报明确错误**（见下“联网探针”）。 |
| `lyrics::scorer` | `LyricsMatchScorer`（`LyricsMatchScore`） | 与 C# **逐项一致**：归一化（NFKC → 小写 → 只留字母数字，按 UTF-16 码元判定）、`strip_brackets` / `core_title`、`is_instrumental`、`build_queries`（至多 6 组变体）、`duration_matches`（±8 s）、四路分值（标题 / 歌手 / 专辑 / 时长）与总分（有歌手 0.55 / 0.25 / 0.20，无歌手 0.72 / 0.20 / 0.08；专辑不符 ×0.97；版本标记多出 ×0.86、缺少 ×0.94；歌手相似度 < 0.30 ×0.75；伴奏直接排除）、`split_multi_value`、`try_parse_threshold`（60 / 60% / 0.6，银行家舍入到两位）、`format_threshold_percent`。阈值常量 0.30 / 0.60 / 1.00。 |
| `online::model` | `OnlineMusicSearch.cs`、`ILyricsSource.cs`、`LyricsSourceKind` 扩展 | `OnlineMusicSource`（序列化名 `Netease` / `QQMusic`）、`OnlineSongResult`、`OnlineSongDetails`（`get_lyrics(prefer_word, bilingual)` 的选择顺序与 C# 相同）、`LyricsSourceCandidate`、trait `LyricsSource`（`label()` + `fetch(&LyricsQuery, &CancelToken)`）、`OnlineError { Cancelled, Failed(中文) }`、`music_source_of`。 |
| `online::service` | `OnlineMusicSearchService` | trait `OnlineMusicSearch`（`search` / `get_details`，测试用手写假实现）+ `OnlineMusicSearchService`：网易云走 `NeteaseClient`（详情失败回落搜索摘要、歌词失败仍应用标签与封面），QQ 走下面的 `qq`。 |
| `online::qq`（私有） | 同上 QQ 部分 | 搜索 `/soso/fcgi-bin/search_cp`（`new_json=1`，兼容旧字段名；`c.y.qq.com` 连接失败 / 非 2xx 切 `c6.y.qq.com`；两个都失败报“QQ 音乐搜索接口不可用（…）”；pubtime 折算 UTC 年份、`index_cd` 从 0 起算 +1、专辑 mid 拼封面 CDN、限制 1–30）；歌词 `music.musichallSong.PlayLyricInfo`（POST JSON，`qrc=1&crypt=1&roma=1`，QRC 解密后得逐字，译文 / 罗马音可以是密文或 base64）失败回退旧 `fcg_query_lyric_new`（HTML 实体解码，有译文时合并成双语）。**保留 C# 的怪癖：PlayLyricInfo 没有译文时双语版本等同原文，所以 QQ 的 `has_translation` 恒为真，标签文案总是“双语…”。** |
| `online::kugou` / `online::lrclib` | `KugouLyricsSource`、`LrclibLyricsSource` | 酷狗三段链路（hash 搜索 → `krcs` 候选 + accesskey → `lyrics.kugou.com/download` 的 KRC，KRC 解不出来退 LRC；最多 2 组关键词、检查前 8 个候选）；LRCLIB（`?q=` 最多 3 个变体 → `track_name` + `artist_name` 兜底，429 / 503 退避重试一次，重试等待算进同一个 10 秒超时）。任何网络 / 证书 / JSON 错误都回落成“没有结果”，**只有取消才是 `Err`**。 |
| `online::net`（私有） | `HttpClient` + `CancelAfter` | 每次请求独立超时（QQ / 封面 20 s，酷狗 / LRCLIB 10 s）与取消；封面 ≤ 10 MB（声明超限不读，没声明边读边数）。 |
| `lyrics::batch` | `BatchLyricsService`、`BatchLyricsOptions`、`BatchLyricsResult` | `BatchLyricsService::fetch_for_file(path, options, cancel)`：标签读取 → 本地标题 / 歌手（没有标题用文件名：去音轨前缀 `^\s*\d{1,3}[\s._-]+`、下划线换空格）→ `build_queries` → 按来源次序（`order_sources`）逐源多变体搜索（每源 ≤ 3 组、每组 10 条、可信匹配即停）→ 取详情 → 歌词（要求有歌词，逐字 / 双语优先级同 C#）→ 备用词源 → 只覆盖 / 补缺 → 写标签；消息文案（“已写入标签、封面、双语逐字歌词（歌词来源：酷狗音乐）”“标签、封面与歌词已完整，跳过”“未找到匹配度足够的在线歌曲”“（未获取到歌词）”）逐字同 C#。`set_fallback_filter` 是**运行期过滤器**（每次需要备用词源时才调用）。`BatchLyricsOptions::auto_enrich()`（QQ 优先、覆盖、逐字）与 `auto_enrich_with(&AppSettings)`（阈值取 `matchThreshold`，`lyricsMode == None` 时只补标签与封面）。 |
| `lyrics::batch::BatchEnricher` | `DownloadService.TryEnrichAsync` / `NcmImportService` 的在线补全 | 实现模块 7 的 `OnlineEnricher` 挂钩：每次调用从**当前设置**（`SettingsProvider`）读阈值与歌词写入方式；结果映射成 `OnlineMatchOutcome`，所以 NCM 导入窗口显示“匹配「歌名 - 歌手」：已写入标签、封面、双语逐字歌词（QQ 音乐）”，下载 / 同步只显示成功说明或“在线匹配未完成：…”（失败只作提示，不影响主流程）。 |
| `lyrics::batch::task` | `BatchLyricsViewModel` 里与界面无关的部分 | `BatchManager`：`scan`（递归 / 不递归，`.mp3 .flac .m4a .ogg .wav .aac .wma .opus`）、`run`（去重、最多 3 个文件并发、逐文件 `started` / `finished` 事件、统计成功 / 跳过 / 失败、取消后不再启动新文件并保留已完成结果）、`cancel`；同一时间一个任务（`busy`）。来源次序不传就用设置里的值，阈值取设置。 |
| `online::manager` / `online::view` | `LyricsEditorViewModel` 里与界面无关的部分 | `OnlineManager`：`search`（带 `fileContext` 就逐条打分、按总分稳定降序排序、低于阈值的标 `belowThreshold`；`query` 留空用文件名推）、`load_result`（找回搜索摘要：请求里带 `song` 或最近 200 条搜索结果的缓存；网易云没有缓存也能按 ID 取）、`fetch_fallback`（备用词源，不受“备用词源”开关限制，与 C# 编辑器一致）、`threshold_view`；DTO 全是 camelCase。 |
| `netease::http`（扩展）、`lyrics::krc`、`library::text` | — | `HttpRequest` 新增 `body: Option<Vec<u8>>`（原始请求体，QQ 的 PlayLyricInfo 是 JSON POST）；`krc::decode_base64_utf8`（= `Convert.FromBase64String` + UTF-8，忽略空白）；`library::text::is_dotnet_letter_or_digit`（= `char.IsLetterOrDigit`）。 |
| `online::fallback_source_enabled` | `AppServices.ApplyFallbackLyricsSettings` | 按词源名读设置里的 `fallbackKugouLyrics` / `fallbackLrclibLyrics`，未知词源恒为开。外壳把它接成批量服务的过滤器，读的是共享的设置对象，所以保存设置后**下一首歌**就生效。 |

### 与 C# 行为的差异（有意为之）

- **实网修正：酷狗候选的 `duration` 现在是毫秒**（如 255582），C# 当秒乘 1000，所有候选都被时长过滤掉，酷狗源在实网上永远“没有结果”。这里 ≥ 10000 按毫秒、更小按秒（旧接口 / 夹具），交叉验证夹具（秒）结果不变。
- **网易云搜索端点失效时给明确错误**：C# 把密文当成“0 条结果”。这里报“接口返回了加密数据，暂时无法使用，请改用 QQ 音乐”（见“联网探针”与“待修问题”）。
- **下载 / 同步的在线补全也用设置里的匹配度阈值**：C# 的下载与同步路径用 `AutoEnrich` 的固定 0.60（只有 NCM 导入窗口用了设置值）；这里三条路径一致读设置。
- **取消**：`CancelToken`，在每个网络请求处立刻生效（C# 靠 `CancellationToken`）；标签写入在阻塞线程里，开始前检查一次。取消不再被“吞掉所有异常”的 catch 吃掉（C# 用 `when (!IsCancellationRequested)` 达到同样效果）。
- **超时不换镜像 / 不静默重试**：与 C# 一致，QQ 搜索超时直接报错（只有连接失败与非 2xx 才换 `c6`）；PlayLyricInfo 超时直接让详情失败（批量会换下一个来源）。
- `OrdinalIgnoreCase`（`feat.` 等连接词匹配）只折叠 ASCII 大小写；`parse_year_from_unix` 对 `DateTimeOffset` 范围之外的 `pubtime` 返回无年份（C# 会抛 `ArgumentOutOfRangeException` 让整个搜索失败）。
- 批量扫描按小写完整路径排序（C# 用当前区域的不区分大小写比较，中文区域的拼音顺序无法逐位复现）；不跟随目录符号链接，读不了的子目录跳过。
- 批量任务多了 `started` 事件（C# 界面只有结果），界面可以据此显示“处理中”；`tagPriority` / `lyricsPriority` 不传时用设置里的值（C# 窗口构造时传入）。
- 阈值文本只按 `.` 小数点解析（C# 还会退到当前区域，如 `0,6`）。

### Tauri 命令与事件（给 10b 用；类型在 `app/src/api/online.ts`）

| 命令 | 参数（前端 camelCase） | 返回 | 失败 `Err` |
|---|---|---|---|
| `online_search` | `source: 'Netease' \| 'QQMusic'`, `query: string`, `fileContext?: FileContext` | `OnlineSearchResponse { source, query, scored, threshold, thresholdPercent, items: [{ song, score: MatchScore \| null, belowThreshold }] }`。带 `fileContext`（`{ title?, artist?, album?, durationMs, filePath? }`）就逐条打分并按总分降序；`MatchScore { title, artist, album, duration, total, percent, rejected, rejectReason, confident }` | `{ kind, message }`：`emptyQuery` / `fallbackSource`（传了 `Kugou` / `Lrclib`）/ `failed` |
| `online_load_result` | `source`, `id`（搜索结果的 `sourceId`）, `wordLyrics: boolean`, `song?: OnlineSong` | `LoadedResult { song（详情更全）, lyrics（按偏好选好：双语优先、逐字不可用回退逐行）, lyricsLabel, wordFallback, hasWordLyrics, hasTranslation, lineLyrics / wordLyrics / bilingualLineLyrics / bilingualWordLyrics, cover: { mimeType, base64, size } \| null, statusText }`，`cover.base64` 可直接填进 `write_tags` 的 `coverBase64` | `unknownSong`（QQ 没搜索过又没传 `song`）/ `primarySource` / `failed` |
| `lyrics_source_fetch` | `source: 'Kugou' \| 'Lrclib'`, `title`, `artist`, `durationMs`, `album?`, `wordLyrics?` | `FallbackLyrics \| null`（`null` = 没有匹配；带 `score`、`lyrics`、`lyricsLabel`、`statusText`，**只有歌词**） | `noFile`（标题空白）/ `primarySource` / `failed` |
| `get_match_threshold` | — | `{ threshold, percent, min, max, default }` | — |
| `batch_scan` | `folder`, `recursive` | `{ folder, recursive, directoryMissing, files: [{ path, name, size }] }` | — |
| `batch_run` | `options: { files, tagPriority?, lyricsPriority?, overwriteExisting?, fetchLyrics?(默认 true), preferWordLyrics? }` | `BatchSummary { total, processed, success, skipped, failed, cancelled, results: [BatchResult] }` | `{ kind: 'busy' \| 'noFiles', message }` |
| `batch_cancel` | — | `boolean`（没有任务为 `false`） | — |

事件 **`batch:progress`**：`{ completed, total, index, path, fileName, stage: 'started' \| 'finished', result: BatchResult \| null }`；`BatchResult { filePath, fileName, status: 'success' \| 'skipped' \| 'failed', success, skipped, message, matchedTitle, matchedArtist, sourceLabel, lyricsFormat }`。

编辑器保存仍用模块 6 的 `write_tags`（`replaceAll: true`）。NCM 导入窗口的 `ncm_get_state` 现在返回 `onlineMatchAvailable: true`，提示文案换成“在线匹配来源：QQ 音乐逐字歌词优先，其次网易云音乐 · 匹配度 N%…”；`ncm_import` 的 `enrichmentMessage` 是真实的匹配说明；下载 / 同步的 `onlineMatchSkipped` 恒为 `false`，前端去掉了“将在后续版本提供”的提示（开发假后端同步更新；**批量与编辑器命令的假后端留给 10b**）。

### 新增依赖

无（Rust 与前端都没有新增）。

### 验证

- 单测：`lp-core` lib **917** 个（模块 10a 新增 178：`lyrics::scorer` 42——C# `LyricsMatchScorerTests` 全部移植，另加归一化 / 补充平面字符 / 括号 / 版本标记 / 多值切段 / 阈值银行家舍入的边界；`lyrics::batch` 42——C# `BatchLyricsServiceTests` 全部移植，另加来源次序、多变体上限与可信即停、搜索 / 详情失败跳过、覆盖与补缺逐字段、逐字 / 双语标签、阈值夹取、取消、写标签失败、文件名推断、扫描、`BatchEnricher` 的设置实时生效与 C# 文案、`BatchManager` 的并发 3 / 去重 / busy / 取消；`online` 88——QQ 22（搜索字段、**c6 域名回退**、两个域名都失败的文案、业务 code、超时不换域名、PlayLyricInfo 请求体 / 头、QRC 逐字、密文译文、旧接口回退、封面 10 MB 上限）、网易云经 `OnlineMusicSearchService` 8、酷狗 16（**异常回落**、LRC 兜底、候选过滤、至多 2 组关键词 / 8 个候选、毫秒时长）、LRCLIB 17（**429 / 503 重试一次**、`track_name` 兜底、候选筛选、重试等待计入超时）、编排 17（打分排序、文件名关键词、缓存找回、备用词源）、模型 / 视图 8；`netease::search` 6；**备用源开关过滤** 在 `batch` 与 `online::tests::misc`），外壳 25 个（新增 4：NCM 在线匹配可用、阈值读实时设置、批量任务空闲、请求结构），集成测试 36 个（新增 `csharp_online` 7）；workspace 合计 **978** 个（模块 9 为 789，净增 189）。`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`pnpm build`、`.\build.ps1 -Tauri` 通过；`.\build.ps1 -Test` C# 仍 404/404。
- **与 C# 的交叉验证**（`tests/csharp_online.rs` + `tests/fixtures/online/`，说明见该目录 README；临时 C# 探针直接引用 `LunaticPrincess.Core`，已删除、不入库；夹具全部合成，没有真实歌词）：
  - **打分**：1156 组（每个标题 / 歌手与自身及相邻项的组合 + 700 组随机组合，覆盖全角、括号、`feat.` / `CV:` / `with`、`/ & ；、` 多值、Live / Remix / Cover / TV size 版本标记、伴奏、emoji 与 CJK 扩展 B 汉字、NFKC 兼容字符、组合附加符号），标题 / 歌手 / 专辑 / 时长 / 总分与四个子函数的分数、`rejected` / 原因 / `confident` 全部一致，**最大差值 1.1e-16**（要求 1e-9）；334 个字符串的 `normalize` / `compact` / `strip_brackets` / `core_title` / `is_instrumental`、400 组 `build_queries`、640 个多值切段、58 个阈值文本的解析与 24 个阈值的百分数格式化**完全一致**。
  - **批量选择**：30 个场景（精确匹配、覆盖 / 补缺 / 已完整跳过、拆分标题与多歌手、QQ 优先 / 仅 QQ / 仅网易云、没有歌词换下一来源、详情失败、搜索失败换关键词、多变体取最佳、可信匹配即停、同分取先到、阈值 0.99 / 0.01、伴奏、文件名标题、无歌手按专辑加权、备用词源（故障源 → 酷狗 / 逐字 / 开关过滤 / 全部关闭 / 空歌词）、`fetchLyrics` 关闭、已有歌词不动、歌手拆分符），C# 用真实 `BatchLyricsService`（在线服务 / 标签服务 / 备用词源是脚本化替身），**30/30 完全一致**：成败 / 跳过 / 文案 / 匹配歌名歌手 / 来源 / 歌词格式 / 写入的每个标签字段（含封面 Base64、逐行 / 逐字歌词）/ 搜索调用序列（来源、关键词、条数）/ 详情调用 / 各备用词源收到的关键词。
  - **词源解析**：31 个场景（QQ 搜索字段与 `c6` 回退 / 全部失败 / 业务 code / 空数据 / 坏 JSON，QQ 详情的 QRC 逐字 + 密文 / base64 译文 + 罗马音、base64 LRC、旧接口回退（带译文 / 无译文 / retcode≠0 / 无数值 ID）、坏 JSON 回退，网易云搜索与详情，酷狗 KRC（带 / 不带译文）→ LRC 兜底 / 候选过滤 / 全失败 / 状态 404 / 时长取整，LRCLIB `q` / `track_name` / 429 重试 / 503 放弃 / 候选挑选），HTTP 响应按 URL 脚本化，C# 用真实 `OnlineMusicSearchService` / `KugouLyricsSource` / `LrclibLyricsSource`：解析出的每个字段、**发出的请求序列（方法 + 完整 URL）**，**31/31 完全一致**（两处有意差异在测试里归一：C# 的 `JsonException` 英文原文 vs 中文说明，C# 对越界 `pubtime` 抛异常——夹具改用边界值）。
- **联网探针**（一次性，已删除；2026-11-04，只看元数据和歌词统计，不下载音频，歌词正文不打印不入库）：对“Lemon / 米津玄師”“晴天 / 周杰伦”各查一遍：
  - **QQ 音乐**：搜索 10 条、最佳匹配分 1.000；详情拿到标签（专辑、2018 / 2003 年）、封面（13 KB / 33 KB）、**逐字 QRC 解密成功**（57 / 63 行）、译文；`PlayLyricInfo` 路径可用。
  - **LRCLIB**：两首都命中（92 / 44 行同步歌词，时长差 ≤ 1 s），无逐字、无译文。
  - **酷狗**：首轮探针发现“永远没有结果”的原因是候选 `duration` 为毫秒（已修，见上）；修后三段链路在实网上走通（`krcs` 候选 → `lyrics.kugou.com` 下载，命中 `Lemon - 米津玄師` 255582 ms）。本机 `mobilecdn.kugou.com`（hash 步骤）证书域名不匹配（`SEC_E_WRONG_PRINCIPAL`），被当作“没有 hash”继续；`lyrics.kugou.com` 的 KRC 下载偶发 `unexpected EOF during handshake`，当次回落 LRC（无逐字）——和 `docs` 里已有的“tls handshake eof”是同一类瞬时故障，**没有加重试**（与 C# 一致）。
  - **网易云搜索：当前匿名不可用**。`/api/search/get/web`（C# 与这里用的端点）现在返回 `{"result":"<十六进制密文>"}`；旧的 `/api/search/get` 与 `/weapi/search/get` 返回与关键词无关的结果（搜“Lemon 米津玄師”得到一串叫“42”的歌）；`/weapi/cloudsearch/get/web` 匿名返回 `code=50000005`。**C# v1.3 同样受影响**（C# 把密文当 0 条结果，批量里网易云优先时每个文件白白多发 3 次搜索）。登录态下 `cloudsearch` 是否可用没有验证（不碰真实登录态），已记入“待修问题”。QQ 音乐不受影响，所以当前真正可用的主词源是 QQ 音乐。
- **实机**（exe 复制到 `D:\claude\lp-run\`，`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，`settings.json` 只指向 `D:\claude` 下的目录，**没有触碰真实登录态与用户目录**，测完已清空 `D:\claude`）：用 WebView2 远程调试端口对真实 exe 调用新命令：`get_match_threshold` → `{0.6, "60"}`；`ncm_get_state` → `onlineMatchAvailable: true` + 新提示；`online_search`（`Kugou` → `fallbackSource`；空白 → `emptyQuery`；`Netease` → 加密数据错误；`QQMusic` + `fileContext` → 20 条、总分 1.0 / 0.84 / 0.83 降序、`belowThreshold` 标注）；`online_load_result` → 双语逐字、封面 JPEG、状态文案；未搜索过的 QQ ID → `unknownSong`；`lyrics_source_fetch`（LRCLIB → 命中并打分；空标题 → `noFile`）；`batch_cancel` 空闲为 `false`。**NCM 导入 4 个合成样本（勾选在线补全）**：3 个带元数据的虚构歌曲给出“在线匹配失败：未找到匹配度足够的在线歌曲”，1 个没有元数据的样本按文件名“no metadata”匹配到了 QQ 上的同名歌曲（“匹配「NO METADATA (Explicit) - GRINX」：已写入标签、封面（未获取到歌词）（QQ 音乐）”——C# 的同一逻辑也会这样），`ncm:progress` 依次出现 `decrypting` → `matching` → `finished`。**批量**：`batch_scan` 找到 2 个合成 MP3；`batch_run`（逐字优先）按文件名“Lemon - 米津玄師”的合成 MP3 写入标签 / 封面 / 双语逐字歌词（`read_tags` 读回标题 Lemon、专辑レモン、2018、有封面、歌词 8994 字符以 `[00:00.000]<` 开头），虚构名的那个“未找到匹配度足够的在线歌曲”；`batch:progress` 事件 `started` ×2 → `finished` ×2，`completed` 0 → 1 → 2；空文件列表 → `noFiles`。

### 已知限制 / 留给 10b 与后续模块

- 歌词与标签编辑器、批量获取窗口、拖放与启动参数已在 10b 完成；设置里的“在线匹配度 / 备用词源”输入在模块 11：设置保存命令还没有，目前阈值与备用词源开关只能改 `settings.json` 后重启，但运行期读取与“保存后即时生效”的机制（共享设置对象 + 每次调用时读取）已经就绪并有测试。
- 网易云搜索当前不可用（见“待修问题”）；备用词源是否启用只影响“批量与自动补全”，编辑器里的备用词源检索不受开关限制（与 C# 一致）。

## 模块 10b（歌词与标签编辑器、批量获取窗口、拖放与启动参数）实现记录

### 形态选择：主窗口内的模态面板，不是独立 Tauri 窗口

编辑器与批量窗口都是叠在主窗口上的大型模态面板（批量窗口再叠在编辑器之上）。理由：

- 拖放事件（`tauri://drag-*`）、IPC、主题 token 与「未保存修改」确认都在同一个 WebView 里，主窗口与编辑器共用一套逻辑；独立窗口要另配 capabilities、无边框自绘标题栏、窗口材质（Mica / Acrylic）与 WebView 数据目录，收益只是可以同时摆在两处，而 C# 版的编辑器本来就是 `ShowDialog` 的模态窗口。
- `Modal` 组件新增了模态栈：Esc 只关最上面一个，编辑器上叠着批量窗口 / 确认框时不会一起关掉。
- 编辑器与批量窗口内容很多，玻璃底改成 97% 不透明（`color-mix`），小对话框保持原来的通透。

### 内容

| 位置 | 对照 C# | 要点 |
|---|---|---|
| `lp-core::editor` | `LyricsEditorViewModel` 的记忆逻辑、`HandleDroppedTargetsAsync` / `LoadDroppedPathsAsync`、`Program.GetStartupLyricsFile`、`ElevationHelper` | `remember_editor_options`（只改 `lyricsEditorSearchSource` / `lyricsEditorPreferWordLyrics`，没变化返回 `false` 不写盘）；`resolve_drop`（文件直接取，文件夹取按名排序的第一个音频，多个目标取第一个可用并报告 `resolvedCount`；一个都没有且拖入了 `.ncm` → 提示改用「导入 NCM」，否则第一个失败原因）；`parse_startup_args`（`--lyrics` / `--edit <文件>`，从第二项起找，文件必须存在，取完整路径；文件不存在只在日志里警告）；`read_cover_file`（PNG / JPEG / WebP / GIF / BMP，按文件头校验，≤ 10 MB，返回 Base64 与 MIME）；`is_elevated`（`OpenProcessToken` + `TokenElevation`，非 Windows 恒为 `false`）；`EditorStateView`（上次来源 / 歌词格式 / 阈值 / 来源优先级文案 / 是否管理员 / 受支持扩展名）。 |
| `tags`（扩展） | — | `AudioTagData::remove_cover` / `WriteTagsRequest.removeCover`：保存时删除文件里的全部封面（同时给了新封面时以新封面为准）。**没提供封面时封面保持原样**（`replace_all` 也不例外），所以前端不必把原封面来回传，封面过大不能预览时也不会丢。 |
| Tauri 命令 | — | `editor_get_state`、`editor_remember_options(source, preferWordLyrics)`、`resolve_drop(targets, log)`（`log` 为真时写运行日志，主窗口拖放用）、`read_cover_file(path)`、`take_startup_file()`（只交出一次）。提权运行时启动日志里写一条拖放受限警告（同 C#）。 |
| `src/api/editor.ts`、`api/tags.ts` | — | 新增 `writeTags`、`pickAudioFile(title)`、封面 / 文件夹选择、`keywordFromFileName`（同 C# `BuildSearchKeywordFromFileName`）等。 |
| `state/useLyricsEditor.ts` + `components/LyricsEditorDialog.tsx` | `LyricsEditorViewModel`、`LyricsEditorWindow.axaml` | 读取（标题、艺术家、专辑、专辑艺术家、年份、音轨 / 碟号、风格、作曲家、注释、封面、歌词、格式 / 时长 / 码率 / 大小）、编辑、保存（`replaceAll: true`，保留网易云 ID，清空即删除，数字字段校验「年份必须是数字。」等）、更换 / 移除 / 还原封面、在线搜索（四个来源下拉、逐行 / 逐字优先，两者记忆到设置；Enter 触发）、打开文件后用清理过的文件名自动搜索一次、结果按匹配度降序显示「匹配度 xx%」（悬停四路分值）、低于阈值标「（低于阈值）」、点击结果填入标签 / 封面 / 歌词（备用词源只填歌词并标「仅歌词」，切换歌词格式时重新载入已选结果）、只显示当前阈值、右上角「批量获取」（以当前文件所在目录为初始目录）、提权时顶部提示拖放受限。未保存的修改在关闭 / 打开别的文件 / 重新读取 / 拖入新文件前确认；底栏显示「● 有未保存的修改」。保存成功后重新读取文件，表单与封面反映文件里的真实内容。 |
| `components/BatchLyricsDialog.tsx` | `BatchLyricsViewModel`、`BatchLyricsWindow.axaml` | 音乐目录（浏览 / 手输）、包含子文件夹、覆盖已有、同时获取歌词、歌词格式、来源优先级与阈值文案（取自设置）、`batch_scan` → `batch_run`，`batch:progress` 驱动逐文件状态（处理中 / 成功绿 / 跳过灰 / 失败红）与进度条，**列表虚拟滚动**（复用 NCM 导入的表格样式），取消后没处理到的行标「已取消」，结束后以汇总结果为准（事件与返回值不保证先后）；关闭时若在运行会先取消。 |
| `state/useFileDrop.ts` + `components/DropOverlay.tsx` | `DropOverlayController` | 监听 `tauri://drag-enter / over / drop / leave`（系统文件拖入网页时 WebView 自己收不到）；离开后 **180 ms** 才隐藏提示层，期间再次进入就取消。主窗口拖入音频直接用编辑器打开，拖入 `.ncm` 在顶部提示条与运行日志里提示改用「导入 NCM」，多个文件只取第一个并写日志；编辑器已打开时拖入文件载入到编辑器（多个时状态栏说明只载入第一个）。登录 / NCM 导入 / 歌词预览等对话框占着界面时不接收拖放。 |
| 主窗口入口 | `MainWindow.axaml` 的「歌词编辑」 | 标题栏「歌词编辑」按钮（打开空编辑器，里面点「打开音频文件」）；歌曲表格里本地状态为「已存在」的行尾多一个「编」按钮，在编辑器中打开匹配到的本地文件；`--edit` / `--lyrics` 启动参数启动后直接打开编辑器。所有可交互控件都有中文 `aria-label`。 |
| `dev/mockBackend.ts` | — | 编辑器与批量窗口的全部场景：内存里的标签存储（保存后重新读取能看到）、在线搜索（带打分，低于阈值 / 超长标题 / 无时长 / 无结果）、载入结果（封面是真 PNG）、备用词源、批量（失败 / 跳过 / 取消 / 3000 个文件）、拖放解析。`?mock=neteasefail`（网易云报“加密数据”）、`startup`（`--edit`）、`kugounone`、`batchbig`、`elevated`；控制台 `__lpMock.drop([...])` / `dragEnter` / `dragLeave` 模拟拖放。 |

### 与 C# 行为的差异（有意为之）

- **任何搜索都不自动应用结果**：C# 手动搜索会自动选中第一个达到阈值的结果并立即填入表单（打开文件后的自动搜索同样如此）；这里只高亮排序、让用户点击后再载入，避免悄悄覆盖正在编辑的内容。备用词源也一样。
- **「移除封面」「还原封面」「更换封面」是新增**：C# 编辑器只能显示封面。`removeCover` 是 module 6 写入接口的新增字段（见上）。
- **拖入 `.ncm` 到编辑器**也提示改用「导入 NCM」（C# 编辑器里只会说“不支持 .ncm 格式”）。
- **没有标题标签的文件用备用词源检索**时，标题取清理过的文件名（C# 会因标题为空而无结果）。
- 启动参数指向的文件不存在时在运行日志里警告（C# 静默忽略）。
- 「歌词编辑」入口打开空编辑器（C# 打开所选歌曲的本地文件；这里表格是勾选模型，没有单一“当前歌曲”，所以改为行尾的「编」按钮）。
- 批量窗口默认勾选「覆盖已有标签与歌词」「包含子文件夹」，与 C# 一致；来源次序不再由窗口传入，后端用设置里的值。

### 新增依赖

无。`windows-sys` 只多开了两个 feature（`Win32_Security`、`Win32_System_Threading`，用于查询进程令牌是否提权），不是新 crate。前端没有引入测试框架：界面用假后端在浏览器里手测并截图（理由：逻辑已放在 `lp-core` 里单测，前端只做编排与展示，与既有模块一致）。

### 验证

- 单测：`lp-core` lib **942** 个（模块 10b 新增 25：`editor` 21——记忆只改两个字段 / 无变化不写盘、编辑器状态序列化、四种来源优先级文案、拖入解析（单文件 / 文件夹按名排序 / 多目标 / `.ncm` / 音频优先于 `.ncm` / 不支持 · 空文件夹 · 不存在 · 空列表的中文提示 / camelCase）、启动参数（`--edit` / `--lyrics`、相对路径转完整路径、文件不存在、缺值与未知参数、第一项是程序自己、多个取第一个存在的）、封面文件（PNG 读取 / 非图片 · 空 · 不存在 · 目录 · 超 10 MB 的中文提示 / 各格式文件头）、提权检测不崩；`tags` 4——`remove_cover` 在 MP3 / FLAC / M4A / OGG / OPUS / WAV 每种容器里删光图片且不动其它字段、新封面优先于 `remove_cover`、没提供封面时 `replace_all` 也不清封面、请求 JSON 解析），外壳 **30** 个（新增 5：记住编辑器选项只写两个字段、没变化不写盘、设置没读出来时不覆盖磁盘、启动文件只交出一次、启动文件不存在只记警告），集成测试 36 个（不变）；workspace 合计 **1008** 个（模块 10a 为 978，净增 30）。`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`pnpm build`、`.\build.ps1 -Tauri` 通过；`.\build.ps1 -Test` C# 仍 404/404。
- **浏览器假后端手测**（1280×800，深色 + 浅色）：`--edit` 启动直接打开；搜索结果带匹配度与低于阈值标注；点击结果填入标签 / 封面 / 歌词，状态栏文案同后端；保存后重新读取（假后端内存）；更换 / 移除 / 还原封面；备用词源只填歌词；修改后拖入文件弹出未保存确认，放弃后载入并在状态栏说明“拖入了 2 个文件”；拖入 `.ncm` / 不支持的格式给出提示；拖离后 180 ms 才隐藏提示层；批量窗口运行 / 取消 / 虚拟滚动，Esc 只关最上面的窗口；`neteasefail` 场景下搜索失败显示中文错误而不是崩溃；歌曲行的「编」按钮。
- **实机**（exe 复制到 `D:\claude\lp-run\`，`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，`settings.json` 只指向 `D:\claude` 下的目录，合成 MP3 样本放在 `D:\claude\samples`，**没有触碰真实登录态与用户目录**，测完已清空 `D:\claude`；通过 WebView2 远程调试端口驱动界面、调用命令）：
  - `--edit "D:\claude\samples\Lemon - 米津玄師.mp3"` 启动后直接打开编辑器，文件名、格式信息「MP3 · 0:01 · 128 kbps」正确，并用清理后的文件名“Lemon - 米津玄師”自动搜索了一次、没有自动应用。
  - 网易云（记忆的默认来源）搜索在这台机器上失败，界面显示中文错误「搜索失败：网络连接失败…」；改选 QQ 音乐：20 条结果按匹配度降序（71%，悬停显示「标题 93% · 歌手 50% · 专辑 50% · 时长 0%」）；点击第一条填入标题 Lemon / 艺术家 米津玄師 / 专辑 レモン / 年份 2018 / 歌词 1263 字符 / 封面 13.2 KB；切换逐字优先会重新载入已选结果。QQ 在第一次尝试时遇到 `unexpected EOF during handshake`（与“外部接口”里已记录的瞬时 TLS 故障同类），重试成功。
  - 来源与歌词格式记忆：`settings.json` 里 `lyricsEditorSearchSource` 变为 `QQMusic`、`lyricsEditorPreferWordLyrics` 变为 `true`，其它字段原样保留。
  - 保存：「已保存」后用 `read_tags` 读回，标题 / 艺术家 / 专辑 / 年份 / 封面 13543 字节 / 歌词 1263 字符与表单一致。
  - 拖放：直接向真实 WebView 发送 `tauri://drag-*` 事件（没有从资源管理器真实拖拽，系统层的 OLE 拖放无法在无人值守时模拟）：提示层出现、离开后 180 ms 内仍在、之后隐藏；拖入 2 个文件载入第一个并说明；`.txt` 给出「暂不支持「.txt」格式…」；`.ncm` 给出改用「导入 NCM」的提示；拖入文件夹载入其中按名排序的第一个音频。
  - 批量：编辑器里点「批量获取」，初始目录是当前文件所在目录；对含 2 个合成 MP3 的子目录运行：「Lemon - 米津玄師.mp3」匹配到 QQ 音乐并写入标签、封面与歌词，虚构名的那个「未找到匹配度足够的在线歌曲」，汇总「完成：成功 1，跳过 0，失败 1。」。
  - 深色与浅色截图确认（见下“已知限制”里的玻璃底说明）。

### 已知限制 / 留给后续模块

- **没有从资源管理器真实拖拽过**（只验证了事件链路与解析逻辑）；提权运行时 Windows 会阻断拖放，界面只能提示。
- **「歌曲行的『编』按钮」没有用真实登录态验证**（不碰真实账号），只在假后端里验证。
- 网易云匿名搜索不可用（见「待修问题」）：它是记忆的默认来源，所以全新设置下打开编辑器时自动搜索会先报错，用户需要手动切到 QQ 音乐；是否把默认来源改成 QQ 音乐留给用户决定。
- `online::qq` 在 QQ 搜索不可用时，错误文案里带有 reqwest 的英文原文（10a 的行为，未改）。
- 设置里的「在线匹配度」「备用词源」「来源优先级」输入还在模块 11。

## 模块 9（按音质下载与一键同步）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `models::audio_quality` | `AudioQualityExtensions`、`AudioQualityTests` | 给已有的 `AudioQuality` 补 `api_level` / `api_levels`（`audio_vivid` 有 `vivid` 别名）/ `bitrate` / `display_name` / `from_api_level`（不区分大小写、`higher` 是 `exhigh` 的旧写法）/ `fallback_chain`（从本档逐级降到标准）。 |
| `netease::player` | `NeteaseApiClient.Songs.cs` 的 `GetSongPlaybackInfoAsync`、`NeteaseMappers.MapPlaybackInfo` | `NeteaseClient::get_song_playback_info`：从请求档位开始逐级降档，每档先试 `/weapi/song/enhance/player/url/v1`（`level` + `encodeType=flac`，多种写法依次试）再试经典接口（`br`）；**v1 一旦抛错，本首歌后面的档位不再试 v1**；两个接口都带桌面客户端 Cookie（模块 3 的 `build_playback_cookie_header`）；所有档位都没有地址时返回最后一个失败原因（保留 `code` / 类别，所以 301 仍可识别）。`SongPlaybackInfo` 含 `url / bitrate / size / format / level / fee / is_trial / 试听区间 / 有效期 / error_message`，失败文案（VIP / 版权 / 无权访问 / code=…）与 C# 逐字相同。 |
| `netease::http`（扩展） | `HttpClient` 流式读取 | `HttpTransport` 新增 `send_stream`（响应头到达即返回，`BodyStream::next_chunk` 逐块取），**有默认实现**（退化为 `send` 后单块），所以只实现 `send` 的假传输照常可用；`ReqwestTransport` 覆盖它做真正的流式读取，并新增 `for_downloads(连接 15 s, 相邻读取间隔 60 s, 总时长 30 分钟)`（C# 给音频下载的 `HttpClient` 总超时也是 30 分钟）——下载不套接口用的 20 秒总超时。 |
| `utilities::cancel`（扩展） | `CancellationToken` | `CancelToken` 加了 `cancelled()`（`Notify`）与 `cancellable(token, future)`：令牌一被取消就丢弃正在等待的 future。取消在等网络时立刻生效，不必等下一个分块。 |
| `download::media` | `MediaFileHelper` | 扩展名推断（接口 `type` 优先于 URL，主机名里的点不算扩展名，回退 `.mp3`）、NCM 魔数、`parse_year`、`build_base_name`（模板 + `FileNameSanitizer`）、`find_existing_audio_file`、`write_tags_atomically`（下载 / 解密结果改名成临时文件 → 写标签 → 改名成最终文件名；**标签写入失败保留音频**）、`move_to_unique_path`（冲突时 `(歌曲ID)`、`(歌曲ID) (2)`…）、`create_tag_data`（备注 `LunaticPrincess`、网易云 ID 等）、`TempFiles` 守卫（正常结束、出错、future 被取消丢弃三种路径都会删临时文件）。 |
| `download::fetch` | `DownloadToFileAsync` / `TryDownloadBytesAsync` | `MediaFetcher`：流式写临时文件、进度回调（有总长度每个整数百分点一次，没有每 256 KB 一次）；封面有 10 MB 上限（响应头声明超限直接不读；没声明就边读边数），任何失败返回 `None` 不打断主流程。 |
| `download::pool` | `SemaphoreSlim` + `Task.WhenAll` | `run_pool`：按输入顺序、拿到并发名额才启动下一个；结果按下标归位；取消后不再启动新任务；任务 panic 只影响那一首（`Slot::Panicked`）。 |
| `download::service` | `DownloadService`、`IDownloadService` | `DownloadService::download(songs, options, sink, cancel)`：见下文流程。`DownloadOptions`（含 `from_settings`）、`DownloadProgress`、`DownloadItemResult`、`DownloadOutcome`；`DownloadApi` trait（`NeteaseClient` 实现它，测试用手写假实现）；`Downloader` trait（单曲下载，同步服务通过它走下载分支）。 |
| `sync::service` | `MusicSyncService`、`ISyncService` | `SyncService::sync(requests, options, sink, cancel)`：本地是 NCM → 解密 + 在线数据重写标签；本地已有普通音频且不要求重新下载 → 跳过；否则下载。每个请求恰好一个结果，顺序与输入一致。 |
| `download::task` | `MainWindowViewModel` 的 `SyncSelectedAsync` / `SelectMissing` 与完成后的重新扫描 | `TransferManager`：`download_tracks` / `download_missing` / `sync_playlist` / `cancel`；`TaskSlot`（同一时间只有一个任务）；挑选歌曲、写运行日志、结束后重新扫描曲库；`TransferStarted` / `TransferSummaryView` / `TransferError { kind: busy \| noSongs \| playlistNotLoaded \| cancelled \| failed }` 等给界面的 DTO。 |
| `library::manager`（扩展） | — | `playlist_songs`、`match_playlist_raw`（返回匹配到的本地文件本身而不是视图）、`PlaylistMatches`（`availability(row)` / `local_track(row)`，重复歌曲沿用第一次出现那一行的结果）；`match_playlist` 改为建立在它之上，行为不变。 |

### 单曲下载流程（与 C# 一致，逐步对应）

1. 按模板算出基名；`skip_existing && !overwrite_existing` 且下载目录里已有同基名的音频（`.mp3 .flac .m4a .mp4 .ogg .opus .wav`，不区分大小写）→ **直接跳过，不发起任何网络请求**。
2. 取链（含逐级降档）。没有地址 → 失败，原因用接口给的中文文案；**试听片段（`freeTrialInfo`）一律判失败**：「该歌曲仅返回试听片段（需要 VIP 会员），已跳过下载。」。
3. 服务端返回的 `level` 与请求档位不同 → 提示「实际音质：无损 (SQ)（杜比全景声 (Dolby Atmos) 不可用，已自动降级）」（进度事件的 `message`，同时放进结果的 `quality_note` 与日志）。实测：匿名请求 Hi-Res 一首免费歌曲，服务端静默给出 `exhigh`（见联网探针）。
4. 流式下载到 `.lp-<进程号>-<序号>.part`。HTTP 非 2xx →「媒体下载失败：HTTP 404」；内容为空 →「下载内容为空，已放弃该歌曲。」；声明长度与实收不符 →「下载中断：只收到 x / y 字节」。
5. 文件头是 `CTENFDAM` → 改名 `.ncm` 并用模块 7 的解密器解到下载目录（允许覆盖临时输出），**输出格式以解密结果的扩展名为准**，封面优先用 NCM 内嵌的。
6. 歌词（模块 5 的解析与序列化：逐行写普通 LRC，逐字模式另写增强 LRC，译文默认写入；失败只当没有歌词）、封面（`write_cover`；NCM 内嵌封面优先，否则下载专辑封面，> 10 MB 或失败跳过）。
7. 写标签（标题 / 歌手 / 专辑 / 专辑艺术家 = 第一位歌手 / 音轨 / 碟号 / 年份 / 封面 / 歌词 / 网易云 ID / 备注 `LunaticPrincess`）并原子落盘；冲突加 `(歌曲ID)` 后缀，`overwrite_existing` 则直接覆盖。标签失败 →「标签写入失败，已保留音频文件。」仍算成功。
8. 在线补全（`auto_match_online`）：沿用模块 7 的 `OnlineEnricher` 挂钩，**模块 10a 已接入**（`BatchEnricher`）；没有实现时（测试里）跳过，`DownloadOutcome.online_match_skipped` / `TransferSummaryView.online_match_skipped` 置位，界面与运行日志各提示一次（不在每首歌的结果里重复）。接入后的文案与 C# 相同（`已写入标签…（QQ 音乐）`、`在线匹配未完成：…`）。

进度：`DownloadProgress { completed, total, index, song_id, song_name, stage（与 C# 相同的中文阶段名）, phase（resolving / downloading / decrypting / tagging / matching）, message, percent, bytes_done, bytes_total, finish? }`；每首歌处理完时最后一条带 `finish`（成败 / 是否跳过 / 来源 / 输出路径 / 说明 / 实际音质），`completed` 只在这时加一。

### 一键同步的三个分支

- **本地已有 NCM**：先用头部读元数据（只作回退，失败不影响解密）→ 交给 `NcmImportService` 解密并写入 NCM 内嵌标签（解到下载目录下的**临时子目录**）→ 取最新歌曲详情（失败回退请求里的歌曲）→ 歌词 / 封面（NCM 内嵌封面优先，否则下载）→ 按在线数据重新写标签，文件名取「在线值 → NCM 头 → 本地扫描结果 → 请求歌曲」里第一个非空的 → 原子落盘。
- **本地已有普通音频**：`redownload_existing`（设置里的「覆盖已存在文件」）为假 → 跳过（消息「本地已存在，跳过。」），为真 → 走下载。
- **本地没有**：走下载，下载服务自己的「已存在则跳过」仍然有效（映射成跳过）。

### 与 C# 行为的差异（有意为之）

- **取消时保留已完成的结果**（下载、同步、NCM 导入一致）：C# 取消抛 `OperationCanceledException` 丢掉整批结果；这里返回 `cancelled: true` 与已完成的结果，在途歌曲的临时文件被清理，界面把没处理到的行标成「已取消」。取消在等网络时立刻生效。
- **同步按设置的并发数同时处理多首**：C# 的同步逐首顺序处理（每首歌各调一次下载服务，下载服务自己的并发上限形同虚设）。上千首的歌单逐首下载太慢，这里与下载共用同一个任务池。结果顺序、三个分支的行为与 C# 一致（交叉验证覆盖）。
- **同步解密 NCM 不碰下载目录里已有的文件**：C# 把 `.ncm` 直接解密到下载目录、文件名取自 `.ncm` 的主干，目录里恰好有同名音频会被覆盖；这里解密到临时子目录，写完标签再改名成最终文件名（有测试）。
- **冲突文件名原子占位**：先 `create_new` 占位再改名覆盖占位文件（Windows 上 `fs::rename` 会覆盖已有文件，不能直接拿来检测冲突）；并发下载同名歌曲不会互相覆盖（有测试）。
- **下载不完整判失败**：响应头声明了长度而实收更少时失败（C# 依赖 `HttpClient` 自己发现）。
- **歌单里重复出现的同一首歌，「下载缺失」与「一键同步」只处理一次**（并发处理同一首歌只会得到两份文件）；「下载选中」按勾选的歌曲 ID 去重。
- **曲库目录不存在时**：「下载缺失」与「一键同步」按“本地为空”处理并给出警告（C# 在这种情况下扫描失败、所有行显示需下载）。
- **在线补全未接入时统一提示一次**（见上），C# 的下载服务在没有批量服务时静默跳过。
- C# 的 `Albums.cs` 的 `GetAlbumAsync` 在下载路径里**从未被调用**（任务书提到“下载时补专辑元数据”，实际 C# 用歌单歌曲自带的专辑信息），所以没有移植。
- HTTP 错误文案没有 C# 的原因短语（`HTTP 404`，不是 `HTTP 404 Not Found`）；网络类错误是中文的「下载失败：网络连接中断（…）」「下载超时：…」。

### Tauri 外壳与前端

- 命令：`download_tracks(playlistId, trackIds)`、`download_missing(playlistId)`、`sync_playlist(playlistId)`（都返回 `TransferSummaryView`，失败 `Err` 为 `{ kind, message }`）、`download_cancel()`、`set_quality(quality)`（**只改 `settings.json` 里的 `quality`**，其他字段原样保留；设置文件当初没读出来时只改内存不写盘；未知名字明确拒绝，因为 `AudioQuality` 的反序列化会把未知值悄悄变成默认档位——实机验证时发现）。事件：`download:started`（`{ kind, total, songIds }`）、`download:progress`（见上）；结束后重新扫描曲库时照常发 `library:progress`。`get_app_state` 新增 `qualityOptions`（九档的成员名 + 中文显示名，界面下拉直接用）。同一时间只有一个下载 / 同步任务（`busy`）；用的是设置的快照，任务运行期间改设置不影响正在跑的任务。
- 前端：歌曲表格新增**复选框列**（点复选框切换、**按住 Shift 再点连选一段**、表头复选框全选 / 全不选且有半选状态，按歌曲 ID 记，换歌单清空）与**下载状态列**（徽标：排队中 / 下载中 42% / 解密中 / 写入标签 / 在线匹配 / 已下载 / 已解密 / 已跳过 / 失败 / 已取消；第二行是失败原因（红）、实际音质提示、字节进度或补充说明；下载中有细进度条；悬停提示完整文字与输出路径）。表格宽度不够时（容器查询 < 730 px）先隐藏专辑列。
- 工具条（本地对比工具条下方）：**音质下拉**（`aria-label="下载音质"`，改动立即生效并写回设置，任务运行中禁用）、「下载选中（N）」「下载缺失」「一键同步」「取消」、已选数量与「清除」；运行中显示「正在同步 18/35」+ 整体进度条，点取消后显示「正在取消…（等当前歌曲收尾，已下载的文件会保留）」；结束后一行汇总（与桌面版日志同文案，有失败时用警告色）+「知道了」；没勾选 / 没有缺失只给一句提示而不是错误。正在扫描 / 对比本地曲库时不开始新任务。任务成功结束后清掉勾选，并重新对比本地（后端已经重新扫描，直接复用扫描结果），所以「本地状态」列与统计立刻更新。所有可交互控件有中文 `aria-label`。
- 逐曲进度事件很密集：前端先写进 ref，每帧最多刷新一次界面；任务结束后以汇总里的结果覆盖逐行状态（事件与命令返回值不保证先后）。
- 开发假后端 `mockBackend.ts`：假下载 / 同步任务（最多 3 首并行，歌曲 ID 是 13 的倍数的失败、11 的倍数的降档，同步里按本地状态走跳过 / 解密 / 下载）；`?mock=dlslow` 让每首歌慢一倍，`dlnote` 让每首都带降档提示。

### 新增依赖

无（Rust 与前端都没有新增；`tokio` 只用了已有的 `rt` / `sync` / `time` 特性里的 `JoinSet`、`Semaphore`、`Notify`；`reqwest` 的 `read_timeout` 与 `chunk()` 是已有特性）。

### 验证

- 单测：`lp-core` lib 739 个（模块 9 新增 131：`models::audio_quality` 6——C# `AudioQualityTests` 全部移植；`netease::player` 12——C# 取链的 7 个测试全部移植，另加 Audio Vivid 别名、业务 301 保持可识别、无效 ID、畸形响应与失败文案；`download::media` 12；`download::pool` 6；`download::service` 44——C# `DownloadServiceTests` 的 21 个全部移植，另加降档提示逐档、未知 `level` 不提示、`lyricsMode=None` 不取歌词、在线补全各分支（成功 / 失败 / 跳过 / 出错 / 未接入）、NCM 解密失败的原因与清理、并发下载同名歌曲不互相覆盖、文件名模板、取消（下载中途网络卡死 / 批量取消保留已完成结果）都清理临时文件、内容被截断、进度按字节与百分比、封面过大 / 恰在上限 / 封面地址 404、真实 lofty 标签服务端到端；`sync::service` 27——C# `MusicSyncServiceTests` 的 18 个全部移植，另加三分支并行、并发上限与结果顺序、NCM 解密不碰同名的无关文件、真实解密器 + 合成 NCM + lofty 的端到端、NCM 文件名回退链；`download::task` 16；`library` 4；`utilities::cancel` 净增 4），外壳 21 个（新增 6：音质选项、只改 `quality` 一个字段、设置不可信时不写盘、拒绝未知音质、任务槽空闲时取消为空操作、事件名），集成测试 29 个（新增 `csharp_download` 2）；workspace 合计 789 个。`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`pnpm build`、`.\build.ps1 -Tauri` 通过；`.\build.ps1 -Test` C# 仍 404/404。
- **与 C# 的交叉验证**（`tests/csharp_download.rs` + `tests/fixtures/download/`，说明见该目录 README；临时 C# 探针直接引用 `LunaticPrincess.Core`，已删除、不入库）：同一份 12 个场景（基础下载 + 封面 + 逐行歌词；FLAC + 逐字歌词 + 译文 + 别名；降档提示；下载下来的是 NCM；文件名模板含非法字符与占位符；同名冲突加 `(ID)` 后缀；跳过已存在；覆盖已存在；五种失败混合（版权 / 试听 / HTTP 404 / 空内容 / 成功）；格式缺失时取 URL 扩展名；同步三分支；同步 NCM 在线数据缺失时的文件名回退），C# 用真实的 `DownloadService` / `MusicSyncService` / `NcmDecoder` / `AudioTagService` 跑一遍，Rust 用真实的 lofty + `FileNcmDecoder` 跑一遍。**12/12 场景完全一致**：每首歌的成败 / 是否跳过 / 来源 / 输出文件名 / 消息，输出目录里的文件集合（也证明没有遗留临时文件 / 目录），每个文件读回的标题 / 歌手 / 专辑 / 专辑艺术家 / 年份 / 音轨 / 碟号 / 歌词（含逐字与译文）/ 网易云 ID / 备注 / 是否有封面 / 封面 SHA-256 / 格式，降档提示逐字相同，下载场景的阶段名集合相同（同步场景 C# 把导入进度的消息当阶段名，只比较共有的阶段）。
- **联网探针**（一次性，已删除）：匿名取 5 首公开歌曲的播放地址、请求极高与 Hi-Res 两档，只看元数据、**没有下载任何音频**：4 首版权受限（`code=404`，文案与 C# 相同）；1 首免费歌曲拿到地址，两档都是 `level=exhigh`、`type=mp3`、码率 320000、大小 4247553、非试听、`fee=0`——请求 Hi-Res 却静默得到 `exhigh`，正是「实际音质」提示要覆盖的真实场景。
- 浏览器假后端（`pnpm dev`，暗色 + 浅色）：复选框点选 / Shift 连选 / 全选半选；「下载选中（4）」运行中显示排队 / 写入标签状态与「正在下载 0/4」，结束后各行「已下载」、汇总文案正确；一键同步 35 首：「正在同步 18/35」、逐行「已下载 + 实际音质提示 / 失败 + 红色原因 / 已解密 / 已跳过 + 说明」、结束汇总「同步完成：成功 34 首（下载 14、NCM 解密 4、跳过 16），失败 1 首」；下载缺失中途取消：「正在取消…」→「下载已取消：已处理 10/15，成功 9 首，失败 1 首」；窗口较窄时专辑列被隐藏。
- 实机（exe 复制到 `D:\claude\lp-run\`，`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，`settings.json` 里 `downloadDirectory` / `libraryDirectory` 预先指向 `D:\claude` 下的目录，**没有触碰真实登录态与用户目录**；测完已清理）：测试数据目录没有登录态，加载不了真实歌单，所以**真实下载没能实机验证，需要用户在真机上确认**——请用一个自己有权访问的账号：①下载选中一首免费歌曲，确认文件落在下载目录、标签 / 封面 / 歌词正确、状态列显示「已下载」；②用有 / 没有 Hi-Res 权益的账号各下一首，确认「实际音质」提示与实际文件的格式相符；③一键同步里有 `.ncm` 的歌，确认被解密并改名；④下载过程中点取消，确认下载目录里没有 `.part` / `.tmp` 残留。用 WebView2 的远程调试端口（只开在本次测试进程上）对真实 exe 调用后端命令：`get_app_state` 带出九档 `qualityOptions`、设置里的 `quality`；`download_cancel` 空闲时返回 `false`；`download_tracks` / `download_missing` / `sync_playlist` 对没有加载过的歌单返回 `playlistNotLoaded`；`set_quality` 只改了 `quality`（`maxConcurrency` 等其他字段原样保留）。

### 已知限制

- ~~在线补全未接入~~：模块 10a 已接入（见上），外壳里 `onlineMatchSkipped` 恒为 `false`。
- 没有“暂停 / 继续”与失败重试；失败的歌曲可以再点一次「下载选中」。
- 取消在“等网络 / 分块之间”立即生效，但已经进入标签写入或改名的那一首会先完成再停。
- 下载目录与曲库目录是两个设置：下载完成后重新扫描的是曲库目录，下载目录不在曲库目录里时下载的歌不会被对比识别到。
- 前端没有自动化测试（与 C# 的 ViewModel 一样靠实机验证）；纯逻辑（Shift 区间选择、进度并入）放在可导出的函数里，但目前没有前端测试框架。

## 模块 8（本地曲库与对比）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `library::track` | `ILocalLibraryService.cs` | `LocalTrack`（路径、标题 / 歌手 / 专辑、时长、格式、码率、是否 NCM、NCM 头元数据、网易云 ID、是否有封面；另有只供界面用的 `title_from_file_name`）、`LibraryScanProgress`、`ScanOutcome`、`LibraryError::Cancelled`。 |
| `library::scanner` | `LocalLibraryService.cs` | `LocalLibraryService::scan(目录, 进度回调, 取消令牌)`：递归遍历（按绝对路径不区分大小写去重、重叠根目录只扫一次、不存在的根目录与空白路径忽略）、扩展名 `.mp3 .flac .m4a .mp4 .ogg .opus .wav .ncm`（不区分大小写；注意与编辑器用的 `audio_file_types` 不同）、Windows 上跳过带 System 属性的文件与带 System / 重解析点属性的目录（符号链接与联接点不跟随，符号链接指向文件的照常收录）、结果按完整路径 `OrdinalIgnoreCase` 排序。普通音频读标签交给模块 6 的 `AudioTagService`，`.ncm` 用模块 7 的 `NcmDecoder::read_info` 读文件头元数据（`is_ncm_file` 为假或读失败都当作没有元数据）；标题 / 歌手缺失时用 `parse_file_name`（「歌手 - 歌名」、中文 / 全角 / 长破折号、去掉前导音轨号 `01.` `02 -` `3)` `07 `）回退；单个文件或目录失败只跳过，无权限的目录计入 `skipped_directories`。 |
| `library::matcher` | `SongMatcher.cs`、`ISongMatcher.cs` | **与 C# 一致的算法**：第一阶段网易云 ID 精确匹配（本地文件同 ID 重复时只认第一个；ID 小于等于 0 不参与；被占用的本地文件不再给别的歌）→ 第二阶段所有候选放进优先队列（分数高优先，同分歌曲下标小优先，再同分本地下标小优先），被占用的候选重新找下一个最优 → 每首本地文件最多匹配一首歌。综合分 = 标题 0.55 + 歌手 0.30 + 时长 0.15，**阈值固定 0.72**（与设置里的「在线匹配度阈值」无关），时长容差 12 秒线性衰减，歌手 / 时长缺失给中性 0.5，标题取歌名与别名里最高的相似度；相似度是相等 → 1、包含 → 0.85 + 0.15 × 长度比、其余字符二元组 Dice。返回值用下标指回输入（`SongMatch { song_index, local_index, method, confidence, availability }`），`LocalAvailability` 为 `Missing / Available / EncryptedNcm`。 |
| `library::text` | `SongMatcher` 里的 `NormalizeForMatch` / `SplitArtistTokens` / `Similarity` | 归一化 = NFKC → 去括号及括号内内容（`( [ { 【 「 『 《 〈 〔 （`，嵌套按深度计）→ 去空白 / 控制字符 / 标点 / 符号 → 小写。**按 UTF-16 码元复现 .NET 的语义**：.NET 按码元判定类别，补充平面字符（emoji 等）的两个代理码元既不是标点也不是符号，所以 C# 会**保留** emoji、不会给它们转小写；这里同样用 `Vec<u16>` 表示归一化文本，长度、包含关系、二元组都按码元算。NFKC 与通用类别用 ICU4X 的内置数据（`icu_normalizer` / `icu_properties`，Tauri 的 `url` 依赖本来就在用；`icu_normalizer` 关掉了默认特性以免多拉 `utf16_iter` / `write16`，Cargo.lock 没有新增 crate）。 |
| `library::manager` | `MainWindowViewModel` 里的 `_localTracks`、`TryScanLibraryInternalAsync`、`RebuildRows` | `LibraryManager`：内存里缓存最近一次扫描结果（对应 C# 的 `_localTracks`，不落盘；曲库目录文本变了缓存自动作废）与最近 6 个歌单的曲目；**同一时间只跑一个扫描**，后来的请求排队，轮到时直接复用刚出炉的结果；`scan`（强制重扫）、`match_playlist(歌单 ID, never | ifNeeded)`、`cancel`；进度节流（每 80 毫秒一条，最后一条必发）；写运行日志（`开始扫描本地曲库：…`、`本地曲库扫描完成：N 个文件（其中 NCM M 个）`、`本地对比：已存在 x，NCM 待解密 y，需下载 z`、目录不存在 / 已取消的警告，措辞同 C#）。 |
| `library::view` | — | 给界面的 camelCase DTO：`LocalMatchView`（`availability`: available / ncm / missing，`method`: none / neteaseId / metadata / fileName，分数、文件路径与文件名、格式、本地标题 / 歌手）、`MatchStats`、`MatchOutcomeView`（含 `scanned`：曲库是否扫描过）、`LibraryCommandError { kind: directoryMissing | cancelled | playlistNotLoaded | failed, message }`。歌单里同一首歌出现多次时，每一行沿用**第一次出现**那一行的匹配结果（与 C# 的 `matches.TryAdd` 一致：本地有这首歌，重复的行也应显示「已存在」）；统计按歌单行数算。 |

### 与 C# 行为的差异（有意为之）

- **并行读标签**：C# 逐个文件顺序读；这里用最多 8 个工作线程并行（标签读取互不依赖），结果仍按路径排序，顺序与线程调度无关（有测试，扫描 1 万个小文件约 1.1 秒）。进度回调在锁内调用，已处理数单调递增。
- **取消不保留部分结果**：返回 `Cancelled`，之前缓存的扫描结果原样保留（不完整的曲库拿去匹配只会得到一堆误报的「需下载」）。
- **「文件名猜测」不在匹配器里**：C# 的 `MatchMethod.FileName` 从未被产生，文件名回退发生在扫描阶段（标签里没有标题 / 歌手时推断出来，再作为普通元数据参与评分）。这里保留同一枚举值；界面把「标题取自文件名的元数据匹配」标成「文件名匹配」（`title_from_file_name`，只改显示，不改评分与结果）。
- **未对比的显示**：C# 没扫描过时所有行显示「需下载」，统计也算需下载；这里显示「—」并提示「尚未对比本地曲库」，因为没扫描不等于本地没有。
- **按钮名**：任务书用「对比本地」「重新扫描」（C# 是「重新对比」「扫描曲库」）。「对比本地」= 没有扫描结果就先扫描，再对比；「重新扫描」= 强制重扫，再对比。C# 没有按状态筛选，这里也没有加。
- **评分剪枝**：先算便宜的歌手 / 时长分，就算标题满分也过不了阈值的候选直接跳过标题比较（标题相似度不超过 1，浮点乘加按同一顺序求值是单调的，不改变任何结果；与 C# 的交叉验证分数逐位一致，见下）。

### Tauri 外壳与前端

- 命令：`scan_library()`（扫描设置里的 `libraryDirectory`，发 **`library:progress`** `{ scanned, total, fileName }`，返回 `{ directory, totalFiles, ncmFiles, skippedDirectories }`）、`match_playlist({ playlistId, scan: "never" | "ifNeeded" })`（返回与歌单歌曲一一对应的 `matches`、`stats`、`scanned` 等）、`library_cancel()`。`get_playlist_tracks` 加载完成后把曲目交给 `LibraryManager` 记住（所以 `match_playlist` 只传歌单 ID，不必把整份歌单再传一遍）。`get_app_state` 的设置摘要新增 `autoMatchLocal`。
- 前端：`state/useLocalMatch.ts`（歌曲加载完成后自动对比——开了 `autoMatchLocal` 就先扫描，没开则只在已经扫描过时对比，与 C# 一致；扫描期间保留上一次的结果，不闪回「未对比」；取消后回到取消前的状态并提示；换歌单或重新加载只采纳最后一次请求）、`components/LocalToolbar.tsx`（歌单头部下方：「本地已存在 x / NCM 待解密 y / 需下载 z」三枚徽章 + 曲库文件数、「对比本地」「重新扫描」按钮、扫描时的进度条 / 当前文件 / 「取消」、目录不存在等错误提示）、歌曲表格「本地状态」列（徽标：本地已存在 / NCM 待解密 / 需下载，颜色用主题变量 `--success` / `--warning` / `--danger` 的淡化底色，深浅色都检查过；悬停提示本地文件名、匹配方式与分数、完整路径）。所有可交互控件有中文 `aria-label`（「对比本地曲库」「重新扫描本地曲库」「取消扫描本地曲库」，徽标 `本地状态：…`）。本地状态列加宽到 100px。
- 开发假后端：默认自动扫描约 3 秒（带进度）后按歌曲 ID 给出已存在 / NCM / 需下载；`?mock=libmanual` 关闭自动对比，`libmissing` 曲库目录不存在，`libbig` 扫描 2 万个文件。

### 新增依赖

| 依赖 | 版本 | 用途 |
|---|---|---|
| `icu_normalizer` | 2.4，`default-features = false` + `compiled_data`（Cargo.lock 里已有，现为 lp-core 的直接依赖） | NFKC 归一化（对应 .NET 的 `string.Normalize(FormKC)`） |
| `icu_properties` | 2.4（同上） | Unicode 通用类别（对应 `char.IsPunctuation` / `IsSymbol` / `IsControl` / `IsWhiteSpace`） |

前端没有新增依赖。

### 验证

- 单测：`lp-core` lib 608 个（模块 8 新增 62：`library::tests` 51——C# `LocalLibraryServiceTests` 8 个与 `SongMatcherTests` 12 个全部移植，另加 ID 精确优先于完美模糊分且空出的文件归另一首、被 ID 占用的文件不再复用、本地 ID 重复、ID 为 0、冲突按分数裁决且落选者退到次优、同分按下标、文件名猜测与 NCM 头元数据参与匹配、中性分（歌手 / 时长未知）、1000 首对 3000 个文件、重复歌曲共享第一次的匹配、视图 camelCase、扫描顺序稳定（并行读取）、无权限目录跳过并计数（用 `icacls` 真的拒绝读取，环境改不了 ACL 时跳过）、取消（开始前与中途）、符号链接不跟随、编排器的缓存 / 目录变化作废 / 目录不存在 / 歌单未加载 / 取消保留旧结果 / 并发请求只扫一次 / 进度节流；`library::text` 11），外壳 15 个（新增 2：请求反序列化、曲库目录取自设置且空闲时取消为空操作），集成测试 `csharp_library` 10 个；workspace 合计 650 个。`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`pnpm build`、`.\build.ps1 -Tauri` 通过；`.\build.ps1 -Test` C# 仍 404/404。
- **与 C# 的交叉验证**（临时 C# 探针直接引用 `LunaticPrincess.Core`，已删除、不入库；样本与期望值在 `tests/fixtures/library/`，说明见该目录的 README，约 510 KB，音频是静音帧 / 伪随机字节）：
  - **匹配器**：五组随机歌单 + 曲库共 385 首歌（同名系列、重复歌曲、错误 / 不存在的 ID、NCM、缺歌手 / 时长、各种标题与歌手变体、同一首歌的多个版本、噪声文件），Rust 与 C# 对**每一首**的匹配文件、方式、状态**完全一致**，分数最大差值 1.1e-16（要求 1e-9）。
  - **文本函数**：归一化 354 个字符串（含随机拼出的 NFKC、括号、标点、符号、控制字符、补充平面字符）、相似度 700 组、歌手拆分、时长分、文件名推断 46 个，全部一致。
  - **字符类别**：对全部 65536 个 BMP 码元比对 `IsWhiteSpace / IsControl / IsPunctuation / IsSymbol` 与 `ToLowerInvariant`：只有 **.NET 8（Unicode 15.0）里还未分配而 ICU4X（更新的 Unicode）已归类的字符**不同（类别 47 个：Balinese、货币符号 U+20C1–20C4、控制图片 U+2427–2429、阿拉伯符号等；小写 8 个：Unicode 16 新增的拉丁大小写对）。测试里固化了这份白名单，出现白名单之外的差异就失败。
  - **扫描**：C# 用真实的 `LocalLibraryService`（TagLib + 真实 `NcmDecoder`）扫描 79 个文件的合成曲库（见 README），Rust 用真实的 lofty + `FileNcmDecoder` 扫描同一目录：77 个文件的集合与顺序一致，标题 / 歌手 / 专辑 / 格式 / 是否 NCM / 网易云 ID / 是否有封面 / NCM 头元数据逐项一致，时长差不超过 30 毫秒（两个库按帧头估算），普通音频码率允许 20% 偏差（静音 FLAC / Opus 在 TagLib 里读不出码率）。
  - **一份 74 首的假歌单对这个曲库**：用 C# 的扫描结果喂 Rust 匹配器，每首的文件 / 方式 / 状态 / 分数（1e-9）与 C# 一致（68 首有本地文件，其中 3 个 NCM）；用 Rust 自己的扫描结果，决定全部一致，分数差在 1e-3 内（时长估算的差）。
- 浏览器假后端（`pnpm dev`）：自动扫描进度、徽标三态、统计徽章、重新扫描时保留旧结果、取消、目录不存在提示、`libmanual` 手动对比、浅色主题。
- 实机（exe 复制到 `D:\claude\lp-run\`，`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，`settings.json` 里 `libraryDirectory` 预先指向 `D:\claude` 下的合成曲库，**没有触碰真实登录态与用户目录**）：测试数据目录没有登录态，所以没法加载真实歌单，**真实歌单上的状态列 / 统计 / 按钮没能在实机验证，需要用户在真机上确认一次**。用 WebView2 的远程调试端口（只开在本次测试进程上）对真实 exe 调用后端命令：`get_app_state` 带出 `autoMatchLocal`；`scan_library` 扫描合成曲库得到 77 个文件 / 6 个 NCM（与 C# 一致），`library:progress` 事件单调递增、首尾正确，运行日志出现「本地曲库扫描完成：77 个文件（其中 NCM 6 个）」；`match_playlist` 对没有加载过的歌单返回 `playlistNotLoaded`；空闲时 `library_cancel` 返回 false；另用 1 万个小 MP3 复制品扫描约 1.1 秒，扫描中途 `library_cancel` 返回 `cancelled`。测完已清理 `D:\claude`。

### 已知限制

- 扫描结果只缓存在内存里（与 C# 一致）：曲库目录里的文件在两次扫描之间变化不会被感知，点「重新扫描」才会更新；设置窗口（模块 11）改了曲库目录后缓存会因目录文本不同而自动作废。
- 下载 / 同步结束后的重新扫描与对比刷新已在模块 9 里接上（见下）。
- 标签库对损坏文件的行为（读不出标签就回退文件名）与 C# 一致，但 lofty 与 TagLib 对个别畸形文件可能一个读得出一个读不出，这类文件的标题 / 歌手可能不同。
- 与 .NET 的 Unicode 版本差异见上（只涉及 .NET 8 之后才分配的字符）。

## 模块 7（NCM 解密与导入）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `ncm::decoder` | `NcmDecoder.cs`、`INcmDecoder.cs` | `NcmDecoder` trait（同步，调用方 `spawn_blocking`）+ `FileNcmDecoder`。文件头、AES-128-ECB 解 RC4 密钥与元数据、封面帧、32 KB 分块 XOR 与 C# 逐字节一致：**音频从不整文件读进内存**，边读边解边写同目录临时文件 `.<主干>.<pid>-<序号>.ncmtmp`，成功后原子改名，取消 / 失败都会清理，不会留下半截成品。输出格式看解密后的首块（`ID3` / 帧同步字 → mp3，`fLaC` → flac，其余按 flac）。可预期的失败（文件头不匹配、长度字段非法、文件被截断、没有音频、目标已存在且未允许覆盖）返回 `success == false` 的结果，文案与 C# 逐字相同；参数非法、文件不存在、取消、磁盘 IO 错误是 `Err(NcmError)`。元数据损坏绝不中断音频解密（`metadata == None`）。读头部时不按声明长度预先分配内存（C# 会按封面声明的最大 64 MB 先分配）。 |
| `ncm::synthetic`（仅测试） | `SyntheticNcmFile.cs` | 按规范做**逆运算**造出真实可解的 `.ncm`（AES 加密密钥 / 元数据、密钥盒另写一份、RC4 变种密钥流异或音频），与生产实现互相独立。 |
| `download::ncm_import` | `NcmImportService.cs`、`MediaFileHelper.CreateNcmTagData`、`NcmImportViewModel` 里与界面无关的部分 | `NcmImportService::import`：路径去重（绝对路径、不区分大小写、保持顺序）→ 依次处理（与 C# 一样**不并发**，解密是磁盘顺序读写）→ `is_ncm_file` → 解密 → 写回内嵌标题 / 歌手（拆分 `/ & 、; ； , ，`）/ 专辑 / 专辑艺术家 / 封面 / 网易云 ID / 备注 `LunaticPrincess`（交给模块 6 的 `AudioTagService`，它自己就是“写临时副本 → 原子替换”）。标签写入失败时音频保留，结果是 `标签写入失败（音频已解密保留）：…`。进度回调逐文件回报“开始 / 匹配中 / 完成”（带最终结果），取消令牌 `CancelToken`。另有 `scan_ncm_files`（递归、不区分大小写的 `.ncm`、按完整路径 OrdinalIgnoreCase 排序、不存在的文件夹与无权限的子文件夹单独报告、不跟随目录联接点）、`prepare_output_directory`（必须完整路径，建不了给中文原因）、`restore_state` / `remember_state`（**只改 `ncmImportFolders` 与 `ncmImportOutputDirectory` 两个字段**）、`NcmJobSlot`（同一时间只有一个导入任务，`ncm_cancel` 取消它）、`view`（给界面的 camelCase DTO）。 |
| `utilities::error_text` | `ErrorText.cs` | 权限不足 / 磁盘已满 / 文件被占用（含 33 锁定冲突）/ 路径过长的识别与中文文案（`describe`、`describe_write`、`try_get_path`）。.NET 异常消息里带路径而 `io::Error` 通常不带，所以写入场景由调用方传目标目录。`error::CoreError::Io` 现在也走这里的判定。 |
| `utilities::file_name_sanitizer` | `FileNameSanitizer.cs` | `sanitize`（按 UTF-16 码元截断）、`build_file_name`（模板占位符不区分大小写、按 title→artist→album→track→year→id 依次替换）。NCM 导入用不到（输出名就是 `.ncm` 主干 + 格式），随本模块移植是因为任务书要求，模块 9 的下载直接用。 |
| `utilities::cancel` | `CancellationToken` | 同步 IO 用的轻量取消令牌（`Arc<AtomicBool>`）。 |

### 与 C# 行为的差异（有意为之）

- **取消时保留已完成的结果**：C# 取消会抛 `OperationCanceledException`，整批结果丢失；这里返回 `NcmImportOutcome { cancelled: true, results: 已完成的部分 }`，界面把没处理到的文件标成“已取消”。
- **写标签不再额外复制一份**：C# 先复制成 `.tagtmp` 再写再替换；`AudioTagService` 本身已经是原子写，所以直接写输出文件。
- **扫描更稳**：C# 的 `EnumerateFiles` 遇到一个无权限的子目录就中止整个文件夹；这里逐目录跳过、只计数。
- 元数据里数字字段按 C# 的转换规则（带小数的 `bitrate` 当 0、`duration` 按双精度截断、整数夹取），`musicName` 为数字时取其文本（serde 的数字文本与 .NET 的原始文本在 `1e3` 这类写法上可能不同，病态输入）。

### 在线补全挂钩（模块 10a 已接入）

C# 的“解密后自动获取在线标签、封面与歌词”依赖 `BatchLyricsService`（模块 10）。本模块只定义挂钩：`OnlineEnricher` trait（`enrich(audio_path, cancel) -> Result<OnlineMatchOutcome, EnrichError>`，实现自己读取当前设置里的来源优先级 / 匹配度阈值 / 是否取歌词）和 `describe_match`（与 C# `DescribeMatch` 同文案）。`NcmImportService::new(decoder, tags, None)` 目前不带实现：勾选了“自动在线匹配”时每个成功的文件在结果里得到 `在线匹配将在后续版本提供，已跳过。`；`AppState` 里装配的位置、勾选框的显示与记忆都已就绪，模块 10 只需把实现传进去（`ncm_get_state` 的 `onlineMatchAvailable` 会自动变 true，提示文案换成 C# 的“QQ 音乐逐字歌词优先，其次网易云音乐 · 匹配度 N%”）。

### Tauri 外壳与前端

- 命令：`ncm_get_state`（记住的文件夹、输出目录——记住的为空回退下载目录、在线匹配是否可用与提示）、`ncm_scan(folders)`、`ncm_save_state(folders, outputDirectory)`、`ncm_import(request)`（长任务；逐文件发 **`ncm:progress`** 事件 `{ completed, total, index, path, fileName, stage: decrypting|matching|finished, message, result? }`，返回汇总 `{ results, total, succeeded, failed, cancelled, outputDirectory, matchedOnline }`；同时写运行日志）、`ncm_cancel`。设置记忆走模块 2 的 `JsonSettingsStore`，**设置文件当初没读出来（正在用默认值）时不写盘**，免得把用户真正的设置覆盖掉。
- 标题栏新增「导入 NCM」按钮（`aria-label="导入并解密 NCM 文件"`）→ `NcmImportDialog`：输入文件夹列表（添加 / 移除；打开时从 `settings.ncmImportFolders` 恢复并重新扫描）、输出目录文本框 + 「浏览…」（目录选择器用 `tauri-plugin-dialog`，capability 沿用 `dialog:allow-open`）、「覆盖已存在的解密文件」（每次打开默认不勾）、「解密后自动获取在线标签、封面与歌词」（默认勾选，勾选状态记在 `localStorage`，C# 版每次打开都默认勾选）、状态行 + 总体进度条、**固定行高 38px 的虚拟滚动文件列表**（文件名、大小、状态：待处理 / 解密中… / 已解密 · FLAC → 输出名 · 补全说明 / 失败：中文原因 / 已取消）、「已发现 N 个文件」、「开始解密 / 取消」「关闭」。添加文件夹、移除文件夹、选择输出目录、开始解密、关闭窗口时都会写回设置；关闭窗口会取消正在进行的导入（与 C# 一致）。所有可交互控件有中文 `aria-label`。
- 开发假后端 `mockBackend.ts` 新增 `ncm_*`：`?mock=ncmempty` 没有记住的文件夹，`ncmbig` 每个文件夹 400 个文件；假结果里第 4 个文件解密失败、第 5 个标签写入失败。

### 新增依赖

无（Rust 与前端都没有新增；测试里的 SHA-256 是 `csharp_ncm.rs` 里手写的几十行，带已知向量自测）。

### 验证

- 单测：`lp-core` lib 546 个（模块 7 新增 108：`ncm::decoder` 41——C# `NcmDecoderTests` 16 个全部移植，另加 5 MB / 160 分块的流式解密、各种分块边界、密钥材料长度 1–300、非法长度字段逐项、截断位置、无音频、取消中途停在分块之间且清理临时文件、提交失败保留原目标、元数据各种形态与 C# 转换规则；`download::ncm_import` 41——C# `NcmImportServiceTests` 11 个全部移植，另加取消保留已完成结果、在线补全挂钩（成功 / 失败 / 出错 / 取消 / 没有实现）、真实解密器 + 真实 lofty 的 MP3 / FLAC 端到端、扫描、输出目录校验、设置记忆往返且保留其他字段、视图序列化；`NcmJobSlot` 3；`utilities::error_text` 15（C# `ErrorTextTests` 全部移植并适配 `io::Error`）、`file_name_sanitizer` 7（C# 3 个 + 边界）、`cancel` 1），外壳 13 个（新增 4：请求反序列化、记忆只改两个字段、设置不可信时不写盘、同时只有一个任务），`csharp_ncm` 集成测试 4 个；workspace 合计 576 个。`.\build.ps1 -Tauri`（fmt / clippy / 测试 / 打包）通过；`.\build.ps1 -Test` C# 仍 404/404。
- **与 C# 的交叉验证**（临时 C# 探针直接引用 `LunaticPrincess.Core`，复制 `SyntheticNcmFile`，已删除、不入库；样本与期望值在 `tests/fixtures/ncm/`，共约 200 KB，音频是静音帧 / 伪随机字节，没有任何真实歌曲）：C# 合成 32 个 `.ncm`（合法 MP3 / FLAC、ID3 头与帧同步字两种 MP3、多分块、40 KB 封面、无元数据 / 坏元数据 / 各种元数据形态、密钥材料长度 1 与 300、`a.b.ncm` 与 `.ncm` 文件名，以及 17 种损坏：各位置截断、魔数、密钥 / 元数据 / 封面长度非法、密钥数据过短等）。
  - **解密**：C# 与 Rust 对每个样本的 `ReadInfo`（元数据或错误文案）、`Decrypt`（成功与否、**错误文案逐字相同**、输出格式与文件名、**输出文件的长度与 SHA-256 完全相同**、封面 SHA-256、全部元数据字段）**32/32 完全一致**。
  - **导入写标签**：11 个可写标签的样本，Rust 导入后读回的标题 / 艺术家 / 专辑 / 专辑艺术家 / 网易云 ID / 封面（SHA-256）/ 格式 / 备注 / 年份 / 歌词与 C# `NcmImportService` + `AudioTagService` 读回的**完全一致**；反方向（Rust 导入写出的 11 个文件由 C# `AudioTagService` 读回）与 C# 自己写出的结果**零差异**。
  - `FileNameSanitizer`：13 个 `sanitize` 向量与 8 个 `build_file_name` 向量与 C# 输出完全一致。
- 浏览器假后端（`pnpm dev`）：开对话框自动恢复文件夹并扫描、不存在的文件夹提示、开始 / 逐文件状态 / 取消后“已取消”、添加 / 移除文件夹重扫。
- 实机（exe 复制到 `D:\claude\lp-run\`、`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，未触碰真实登录态；用 Windows UI 自动化 + 截图，只用合成样本）：对话框渲染正常；**系统文件夹选择器**真实走通（UIA 点「添加文件夹…」、把路径粘进「資料夾:」框回车，添加了 `ncm-in\周杰伦`，立刻扫描出 3 个文件，`settings.json` 里 `ncmImportFolders` 已写入）；预置 4 个文件夹（含 1 个不存在）与输出目录后**重新启动再打开对话框**：文件夹列表、输出目录原样恢复，自动重新扫描出 37 个文件并提示“1 个文件夹不存在”；点「开始解密」：37 个文件（合法 MP3 / FLAC、49 KB 多分块、30 个批量样本、1 个截断的、1 个其实是 MP3 的）→ 35 成功 2 失败，状态列逐行显示“已解密 · MP3 → 名字.mp3 · 在线匹配将在后续版本提供，已跳过。”，失败行显示中文原因，输出目录 35 个文件、无临时文件，运行日志出现“NCM 导入完成：成功 35 个，失败 2 个”，`settings.json` 里 `maxConcurrency` 等其他字段原样保留。测完已清理 `D:\claude`。

### 已知限制

- ~~在线补全未接入~~：模块 10a 已接入（`BatchEnricher`），勾选后给出真实的匹配说明；没有实现时（测试里）仍显示“将在后续版本提供”。
- 导入窗口没有“选择单个 `.ncm` 文件”的入口（C# 版也只有文件夹）；拖放导入在后续模块随主窗口拖放一起做。
- 取消在“当前文件处理完 / 下一个分块之间”生效；已经解密出来的文件保留在输出目录。

## 模块 6（标签读写）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `tags::model` | `IAudioTagService.cs` | `AudioTagData`（写入，含 `replace_all`）、`AudioTagInfo`（读取）、`TagError`（中文文案）。比 C# 多了 `track_total` / `disc_total`（C# 靠 TagLib 隐式保留已有总数，这里显式暴露）和 `cover_mime_type`。 |
| `tags::service` | `AudioTagService.cs` | `AudioTagService` trait（同步，调用方 `spawn_blocking`）+ `LoftyTagService`。读写规则逐条对应 C# 的 `ReadCore` / `ApplyTag` / `ApplyComment` / `ApplyCover`。底层是 lofty 的通用 `Tag` + `ItemKey`，写回时转成容器自己的标签类型（ID3v2 / VorbisComments / ilst / RIFF INFO / APE），这样编辑器没展示的字段（TXXX、PRIV、UFID、ReplayGain、自定义 Vorbis 字段、`ilst` 里的其他原子、带描述的 `COMM`）原样保留，有测试。 |
| `tags::text` | `AudioTagService` 里的私有静态方法 | `null_if_blank` / `join_values`（`/` 连接）/ `split_values`（`/ ; ； 换行`）/ `clean_comment` / 网易云 ID 标记 `LunaticPrincess:<id>`（正则、清理、溢出时不回落）/ `detect_image_mime`。 |
| `tags::view` | — | 给界面用：`TagView`（封面转 data URL，超过 8 MB 不内联只标记；网易云 ID 用字符串避免 JS 精度丢失）、`WriteTagsRequest`（封面 Base64）、`TagCommandError { kind, message }`。 |
| `ncm::client_comment` | `NeteaseClientComment.cs` | `163 key(Don't modify):<base64>` 的 AES-128-ECB + PKCS7 解密、取 `musicId`（数字或字符串）、`build_comment`（测试用）。`MODIFY_KEY` 留给模块 7 的 NCM 解密共用。Base64 按 .NET `FromBase64String` 的宽容度（要求规范填充、容忍末尾多余比特）。 |
| `utilities::audio_file_types` | `AudioFileTypes.cs` | 扩展名判定、`try_resolve`（文件 / 文件夹 → 第一个受支持文件）、`list_supported_files`；扩展名规则与 .NET `Path.GetExtension` 一致。 |

### 写入的安全性（与 C# 的有意差异）

C# 是就地 `file.Save()`，写到一半失败会损坏文件。Rust 版：复制成同目录临时副本 `<原名>.lp-tmp-<进程号>-<序号>` → 改动全部写进副本 → 重新解析副本确认类型没变 → `rename` 原子替换原文件。任何一步失败都清理副本、保留原文件（有测试：原文件被占用时写入失败，原文件字节不变，且不留副本）。不写预留填充（`preferred_padding(0)`）：我们总是整体写出，填充没有意义，反而让 Ogg 每次多几 KB。

### 各容器的字段映射与 C# 的差异

- **MP3**：ID3v2.3（UTF-16）新建，文件里已有 2.4 就保持 2.4（C# 的 TagLib 行为）。2.3 里多个艺术家 / 流派 / 作曲家用 `/` 拼成一个帧（lofty 默认写 2.4 的 NUL 分隔，2.3 的读取方——包括 TagLib——会在 NUL 处截断，只剩第一个值，交叉实测发现的）；2.4 则每个值一个条目。年份由 lofty 在 2.3 下写成 `TYER`（C# 在 2.3 标签里写 `TDRC`，不合规）。备注与歌词的语言码写 `eng`（C# 取系统区域，如 `zho`）。
- **ID3v1**：C# 每次保存都会另写一份 ID3v1；读取时仍像 TagLib 的 `CombinedTag` 一样“主标签优先、其余兜底”。`replace_all` 写入时会移除 ID3v1，否则清空的字段会被旧的 ID3v1 兜底读出来。
- **FLAC**：**封面写入与读取都是原生 `PICTURE` 块**。任务书里说 C# 用 `METADATA_BLOCK_PICTURE` 存 FLAC 封面，实测不是：TagLib 的 FLAC 封面落在 `FlacMetadata`（即原生 PICTURE 块，交叉夹具里解析块类型确认；`csharp_tags.rs` 有测试固化），只有 Ogg Vorbis / Opus 才用 `METADATA_BLOCK_PICTURE`（Ogg 没有原生图片块，lofty 与 TagLib 一样）。所以 FLAC 在两边一致，没有需要记录的差异；另外读取仍兼容 FLAC 的 VorbisComment 里残留的 `METADATA_BLOCK_PICTURE`（别的工具写的），并在换封面时清掉，避免重复封面（有测试）。
- **Vorbis / Opus / FLAC 备注**：TagLib 写 `COMMENT`，lofty 也是；读取时 `COMMENT` 优先，其次 `DESCRIPTION`。歌词写 `LYRICS`（和 TagLib 一致），读取兼容 `LYRICS` / `UNSYNCEDLYRICS`。年份写 `DATE`。
- **M4A**：TagLib 把多值字段拼成 `a; b` 写进同一个 data 原子，读时按 `;` 拆开；lofty 不拆，所以读 `ilst` 时额外按 `;` 拆（否则 C# 写出的文件会显示成 `a; b`）。C# 的 M4A 在「备注 + 网易云 ID 标记」同时存在时会丢标记（TagLib 的 AppleTag 备注只取 `;` 前一段，C# 版的 bug）；Rust 版整串保存，能读回，C# 写出的 M4A 里读不到的 ID 仍读不到（文件里就没有）。
- **WAV**：与 C# 一样同时写 ID3v2 块和 RIFF INFO。TagLib 追加 `id3 ` / `LIST` 块后**不更新 RIFF 头长度**（文件比头里声明的大），lofty 按声明长度找块会找不到旧的 `id3 ` 块、新建一个 `ID3 ` 块，旧值仍被读到；所以写 WAV 前，若声明长度之后的数据恰好能按块边界走完，就先把 RIFF 头长度修正到文件实际大小（有测试）。
- **WMA**：lofty 不支持 ASF，读返回 `None`（「格式不支持」）、写返回 `Unsupported`。`AudioFileTypes` 仍保留 `.wma`（与 C# 一致，供后续拖放用），「查看标签」的文件选择器过滤器里去掉了它。这是相对 C# 的功能缺口。
- **读取的小改进**：封面优先取「正面封面」（C# 取第一张图）；备注不再把带描述的 `COMM`（`iTunNORM` 等程序自用数据）当作用户备注；网易云 ID 会扫描全部备注帧；`replace_all` 保存时，年份没变就保留已有的完整日期（`2003-05-21` 不会被截成 `2003`）；读取时 IO 故障（权限、占用）返回 `Err` 带中文原因，而不是像 C# 一样一律当成「无法读取」。
- **lofty 的小怪癖**：Vorbis 里 `ENCODER` 字段会被当作厂商串并从字段里消失（Ogg 写入会保留文件原有的厂商串）；`VorbisComments::from(Tag)` 内部的 swap 式移除会打乱同名多值字段的顺序，写入前把 `EncoderSoftware` 条目挪到最前面规避（有测试）。

### Tauri 外壳与前端

- 新增 `tauri-plugin-dialog`（capability `dialog:allow-open`）；命令 `read_tags(path)` → `TagView | null`（`null` = 格式不支持或损坏；IO 故障是 `{ kind, message }`），`write_tags(path, request)` 已实现、有 lp-core 测试，本模块前端不调用（编辑器在模块 10）。`AppState` 新增 `tags: Arc<dyn AudioTagService>`，读写在 `spawn_blocking` 里跑。
- 标题栏「查看标签」按钮（`aria-label="查看本地音频文件的标签"`）→ 系统「打开文件」对话框（过滤 `mp3 / flac / m4a / mp4 / ogg / opus / wav / aac`）→ `TagsDialog`：封面（无封面 / 封面过大两种占位）、全部字段（标题、艺术家、专辑、专辑艺术家、年份、音轨 `3/12`、碟号、流派、作曲家、备注、网易云 ID，空值显示「—」，每个值有 `aria-label`）、歌词（只读 `<pre>`）、文件信息（格式、时长、码率、大小、封面类型与大小、路径），加载中 / 无法读取 / 失败（重试）三种状态，「选择其他文件…」「关闭」；深色 / 浅色已在浏览器假后端里检查。标题栏右侧新增 `titlebar-actions` 容器放工具按钮与账号区。
- 开发假后端 `mockBackend.ts`：`plugin:dialog|open` 依次返回五个假路径，`read_tags` 按文件名给出完整标签 / 无标签 / 已损坏（`null`）/ 无权限（失败）/ 封面过大。

### 新增依赖

| 依赖 | 版本 | 用途 |
|---|---|---|
| `lofty` | 0.25.5 | 标签与音频属性读写（对应 TagLibSharp 2.3.0） |
| `tauri-plugin-dialog` | 2.8.4 | 系统打开文件对话框 |
| `@tauri-apps/plugin-dialog` | 2.8.4 | 前端调用上面的插件 |

### 验证

- 单测：`lp-core` lib 438 个（模块 6 新增 75：`tags::service` 约 40——C# `AudioTagServiceTests` 11 个与 `NeteaseClientCommentTests` 的标签部分全部移植，另加六种容器的「写满 / 清空 / 局部更新 / 换封面」往返、未展示字段保留、FLAC 原生 PICTURE 与旧 `METADATA_BLOCK_PICTURE` 清理、ID3v2.3/2.4、ID3v1、WAV 头修复、M4A 拆分、写入失败不破坏原文件等；`tags::text` 11、`tags::view` 8、`ncm::client_comment` 8、`utilities::audio_file_types` 6、夹具 2），`csharp_tags` 集成测试 4 个，合计 workspace 460 个；`.\build.ps1 -Tauri`（fmt / clippy / 测试 / 打包）通过；`.\build.ps1 -Test` C# 仍 404/404。
- 测试夹具：MP3 / WAV / M4A 在 `tags/fixtures.rs` 里按格式规范用代码生成（M4A 只有容器结构真实，AAC 帧是占位字节）；FLAC / Ogg Vorbis / Ogg Opus 是 Python `soundfile` 生成的 0.5 秒静音，提交在 `tests/fixtures/audio/`（几百字节到 2.7 KB，生成方式见该目录 README）；无任何受版权保护的音频。`LP_EXPORT_SAMPLES_DIR` / `LP_EXPORT_RUST_TAGS_DIR` 环境变量能让对应的测试把样本导出到目录，供跨实现探针使用。
- **与 C# 的交叉兼容实证**（临时 C# 探针直接引用 `LunaticPrincess.Core` 调用 `AudioTagService`，已删除、不入库；同一份写入数据 `tests/fixtures/tags/spec-full.json`：标题含 emoji、两个艺术家、专辑、专辑艺术家、音轨 / 碟号、年份、流派、作曲家、备注、多行普通 / 逐字歌词、网易云 ID、PNG 封面）：
  - **C# 写 → Rust 读**：MP3 / FLAC / M4A / OGG / OPUS / WAV 各三个场景（写满 / 局部更新 / `replace_all` 清空）+ 一个带客户端 `163 key` 备注的 MP3，共 19 个文件已提交为夹具，`csharp_tags.rs` 要求每个字段（含封面字节、MIME、歌词、网易云 ID）与 C# 读出的一致；时长允许 25 ms、码率允许 20% 误差（M4A 样本的 AAC 帧是占位字节，lofty 按实际数据量得到 2 kbps，TagLib 读 esds 声明的 128 kbps，不比较）。唯一允许的差异是 M4A 的网易云 ID（见上，Rust 能读到的不少于 C#）。
  - **Rust 写 → C# 读**：同样三个场景在六种容器上共 18 个文件，C# `AudioTagService.ReadAsync` 读回的全部字段（含封面 SHA-256、歌词、网易云 ID）与 C# 自己写的完全一致；另外 6 个「Rust 在 C# 写好的文件上局部改标题」的文件，除标题外所有字段、封面不变。第一轮实测发现并修复了三个问题：2.3 多值被 NUL 截断、M4A 读取没拆 `;`、TagLib 写的 WAV 头长度不对导致旧 ID3 块读到旧值。
- 实机：release 版 exe 复制到 `D:\claude\` 运行（`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data` 式的独立数据目录，未碰真实登录态），用 Windows UI 自动化点「查看标签」、操作系统文件对话框（把路径粘进「文件名」框回车），对 Rust 写出的 MP3 / M4A / OPUS、C# 写出的 FLAC / WAV、无标签 MP3、非音频文件逐个打开：字段、歌词、封面信息、网易云 ID、文件信息、「无法读取」提示都正确（读出的是 UIA 控件名称，并截图确认对话框渲染）。窗口没有被切到后台。测完已清理 `D:\claude`。

### 已知限制

- WMA 不支持（见上）。
- Windows 资源管理器只认 RIFF INFO 的 WAV 标签：写入时两种都写，但 RIFF INFO 没有总数、歌词、封面。
- 读取时 `bitrate` 是 lofty 的 `audio_bitrate`（kbps），个别容器与 TagLib 报告的值不同（FLAC / Opus 更准确）。

## 模块 5（歌词解析）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `lyrics::models` | `LyricModels.cs`、`RawLyrics` | `LyricWord` / `LyricLine` / `LyricDocument` / `RawLyrics`；时间用 `i32` 毫秒，边界运算沿用 C# `int` 的回绕语义（`wrapping_*`），极端输入不会 panic。 |
| `lyrics::parser` | `LyricParser.cs` | LRC（多时间戳行、`[offset]`、元数据标签、3 种小数位）、增强 LRC 词标签、YRC v0 与 JSON 风格行、QRC（含 `<Lyric_1 …/>` 外壳）、译文合并（全局贪心最近邻，800ms 容差，`//` 占位不写入）、`to_plain_lrc` / `to_enhanced_lrc(doc, include_translation)`。逐字优先级 QRC > YRC > LRC。纯函数。 |
| `lyrics::html` | `WebUtility.HtmlDecode` | QRC 外壳里的 `&amp;` 等实体还原，按 .NET 算法实现（HTML 4.01 的命名实体表 + `HTML 4.01 全部 252 个命名实体 + `&apos;`apos;`，数值实体、无效实体原样保留），不引入依赖。 |
| `lyrics::qrc` | `QqTripleDes.cs`、`QrcDecryptor.cs` | QQ 的非标准 3DES 变体按位逐轮移植（含 S2 / S4 表里变体自带的重复值），已知向量 `133457799BBCDFF1 / 0123456789ABCDEF → F63A972CE52AA327`；`looks_encrypted` / `try_decrypt`（十六进制 → 3DES-ECB → zlib → 换行归一、去尾部 NUL）。 |
| `lyrics::krc` | `KrcLyricsParser.cs` | `krc1` 魔数 + 16 字节 XOR + zlib（失败回落明文）、`[language:<base64 JSON>]` 的 `type=1` 逐行译文、逐字时间轴解析。与 C# 一样只读 `type=1`，不处理 `type=0` 的罗马音。 |
| `lyrics::zlib` | .NET `ZLibStream` | 共用的宽容解压：**数据被截断时返回已解出的部分**（.NET 8 行为，由 C# 探针的截断夹具核对），损坏（头 / 块 / 校验和）才失败；流结束后的多余字节（QRC 块填充）忽略。 |
| `netease::lyrics` | `NeteaseApiClient.Lyrics.cs`、`MapLyrics` | `get_lyrics`：公开接口 `GET /api/song/lyric?…&yv=1` → `weapi/song/lyric` → `weapi/song/lyric/v1` 三级兜底（任何 `NeteaseError` 都落到下一级，三条全败返回带「获取歌曲 N 歌词失败」前缀的最后一个错误，保留 code，所以 301 仍可识别）；`nolyric` / `uncollected` 返回空结果而非失败。 |
| `lyrics::service` | `DownloadService.TryBuildLyricsAsync` 的序列化规则 | `LyricsService::get(id, mode)`：逐行 → `to_plain_lrc`，逐字优先 → 有逐字数据时 `to_enhanced_lrc`、否则回退逐行；译文默认写入。返回 `LyricsPreview`（文本、是否逐字、是否有逐字数据、是否含翻译、行数、是否为空、来源）。歌词接口匿名可用，**不要求登录**；只有业务 301 且当前带着登录态时才走 `LoginManager::handle_runtime_auth_error`（续期再复核，仍未登录才清除）。 |

### Tauri 外壳与前端

- 命令 `get_song_lyrics(id, mode)`（`mode` 为 `line` / `word`）；失败 `Err` 为 `{ kind: sessionExpired | failed, message }`。
- 歌曲表格：双击一行，或点行尾新增的「词」按钮（`aria-label="查看歌词：歌名"`），打开歌词预览对话框（复用 `Modal`，新增 `className` 属性做 `modal-wide` 加宽，玻璃样式不变）。表格列模板多了一列 32px，最小窗口宽度 960 下仍放得下。
- 对话框：胶囊分段控件「逐行 / 逐字优先」（`role=radiogroup`），元信息「来源：网易云音乐 · 逐字 / 逐行歌词 · 含 / 无翻译 · N 行」，请求逐字但没有逐字数据时提示「这首歌没有逐字歌词，已显示逐行歌词」；只读 `<pre>` 预览（可选中文字，不写任何文件）；加载中 / 失败（重试）/ 暂无歌词（纯音乐或未收录）三种状态；已取回的形式在对话框内缓存，切回去不重新请求；Esc / 点遮罩关闭。深色 / 浅色均在浏览器假后端里检查过。
- 开发假后端 `mockBackend.ts` 新增 `get_song_lyrics`：歌曲 ID 能被 11 整除 → 失败，7 → 暂无歌词，5 → 仅逐行，其余逐字 + 翻译。

### 与 C# 行为的差异（有意为之）

- 无。解析 / 序列化 / 解密的输出与 C# 逐字节一致（见下）。仅有两处为了对齐 .NET 8 而特意保留的“怪行为”：JSON 行 `t` 超出 `int` 范围时，`(int)double` 在 .NET 8 / x64 得到 `int.MinValue`（Rust 版照此处理，.NET 9 起 C# 会饱和）；zlib 截断时返回部分数据而不是失败。
- KRC 的 UTF-16 BOM 识别只做 LE / BE，没做 UTF-32（`StreamReader` 会识别）；HTML 实体解码里数值实体只接受纯数字（C# 的 `uint.TryParse` 还容忍空白与正负号）。二者都是病态输入。

### 新增依赖

| crate | 版本 | 用途 |
|---|---|---|
| `flate2` | 1.1.10（默认纯 Rust 后端 `miniz_oxide`） | QRC / KRC 的 zlib 解压 |
| `regex` | 1.13.1 | LRC 时间戳 / offset、QRC 外壳、KRC 行与词标签（与 .NET 的 `Regex` 语义一一对应，比手写扫描更不容易偏离） |

### 验证

- 单测：`lp-core` lib 363 个（模块 5 新增 102 个：C# 的 `LyricParserTests` 15 + `QrcLyricParserTests` 7 + `QrcDecryptorTests`（含已知向量）+ `KrcLyricsParserTests`（夹具逻辑 `KrcFixture` 一并移植）+ `NeteaseApiClientLyricTests` 5 全部移植，另加边界、HTML 解码、zlib、`LyricsService` 的 301 复核 / 匿名 / 网络错误等），外壳 9 个，`csharp_compat` 5 个，新增 `csharp_lyrics` 4 个；`.\build.ps1 -Tauri` 的 fmt / clippy / 测试 / 打包全部通过；`.\build.ps1 -Test` C# 仍 404/404。
- **与 C# 逐字节对比**（`tests/csharp_lyrics.rs` + `tests/fixtures/lyrics/`）：临时 C# 探针（直接编译 C# 版 `LyricParser` / `QqTripleDes` / `QrcDecryptor` / `KrcLyricsParser` 源码，探针已删除、不入库）对 92 个解析用例（LRC / 增强 LRC / 译文 / YRC / QRC / 序列化）记录文档结构与四种序列化输出，22 个 KRC 解析用例，31 个解密用例（C# 的 .NET `ZLibStream` + 3DES 变体生成的密文，含截断、校验和损坏、BOM、非法 UTF-8、明文兜底），5 组 3DES 随机向量。Rust 结果**全部完全相等**。夹具里只有手写的假文本。
- 联网探针（一次性，已删除）：匿名取 10 首公开歌曲的网易云歌词（6 首有歌词：49 行逐字、62 行逐行、50 行、30 行逐字 + 翻译、24 行逐字、63 行；其余为纯音乐 / 未收录），Rust 与 C# 的解析与四种序列化输出逐一 `deepEqual` 完全一致。真实歌词有版权，未入库。
- 浏览器假后端：双击打开、行尾按钮、逐行 / 逐字切换、逐字回退提示、暂无歌词、失败重试状态、浅色主题。
- 实机：release 版 exe 在 `D:\claude\` 下启动正常；测试期间有真人在该实例上扫码登录成功（只在 `D:\claude\lp-data`，未碰真实登录态，测完已清理），但窗口随后被切到后台、系统不允许自动化抢回前台，**歌词预览对话框的实机操作没能验证，需要用户在真机上双击一首歌确认**。

## 模块 4（歌单）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `netease::models` | `NeteaseModels.cs` | `ArtistRef` / `AlbumRef` / `NeteaseSong` / `PlaylistSummary` / `PlaylistDetail`，camelCase 序列化可直接交给前端；`PlaylistDetail.track_ids` 不序列化（前端用不到，长歌单省流量）。 |
| `netease::mappers` | `NeteaseMappers.cs`（歌曲 / 歌单部分） | 字段读取全部宽容（数字 / 字符串混用）；`ar`→`artists`、`al`→`album`、`alia`/`alias`、`dt`/`duration`、`no`/`trackNo`、`cd`/`disc` 的回落顺序与 C# 一致；`maxbr` / `fee` 来自歌曲自带 `privilege` 或顶层 `privileges` 数组（按 id 关联）。 |
| `netease::playlists` | `NeteaseApiClient.Playlists.cs` | `get_user_playlists`：`uid` + `limit=1000` 翻页（`more=false` 或页不满即停，最多 10 页，按 id 去重）；`get_playlist_detail`：`/weapi/v6/playlist/detail`（`n=1000, s=8`）先收内嵌 `tracks`，再以 `trackIds` 为全量有序来源，缺失的按 **500 首 / 批**调 `song/detail` 补齐，最后按 `trackIds` 顺序还原；进度回调 `(已加载, 总数)`。 |
| `netease::songs` | `NeteaseApiClient.Songs.cs`（`GetSongDetailsAsync`） | `get_song_details`：去重、500 一批、按调用方顺序返回；业务 JSON 同时带 `c` 与 `ids`。播放地址 / 取链属于下载模块，模块 9 再补。 |
| `playlist::PlaylistService` | `MainWindowViewModel` 的加载与 `HandleOperationError` | `list()` / `tracks(id, progress)`：写日志（`已加载 N 个歌单`、`歌单「X」加载完成，共 N 首`、`歌单较大，正在补全 N 首歌曲详情…`）；会话里没有 userId 时先补取账号；接口业务 301 时调用 `LoginManager::handle_runtime_auth_error`（先续期再复核，仍未登录才清除），复核后仍有效 / 无法确认则提示「已保留登录，可稍后重试」。错误以 `{ kind, message }`（`notLoggedIn` / `sessionExpired` / `failed`）交给前端。 |

### Tauri 外壳与前端

- 命令：`list_playlists`、`get_playlist_tracks(id)`；事件 `playlist:progress`（`{ playlistId, loaded, total }`，只有需要分批补齐时才发）。
- 前端加载时机：启动校验（`validate_login`）**结束后**，已登录就加载歌单并选中第一个；扫码登录成功（账号 ID 变化）后同样自动加载；退出登录后清空。等校验结束再加载是为了不与启动期登录复核并发（C# 也是顺序执行）。昵称 / 会员状态的刷新不会触发重新加载。
- 左侧侧栏：展开 272px，收起 88px 只显示封面（封面 42→56px 同步过渡）；宽度 0.24s 过渡、文字淡出；列表上下边缘用 CSS `mask-image` 渐隐；侧栏顶部按钮与 `Ctrl+B` 切换，收起状态记在 `localStorage`（仅界面偏好，失败静默）；选中项高亮；上下方向键在歌单间移动焦点；刷新按钮带“刷新中…”状态，刷新失败保留旧列表并提示。C# 版侧栏没有「我创建的 / 我收藏的」分组，故未做；封面 `http://` 升级为 `https://` 并带 `?param=宽y高` 取缩略图。
- 右侧：歌单头部（封面、名称、创建者 · 曲目数、简介两行截断）+ 歌曲表格（序号、标题 + 别名、歌手、专辑、时长、本地状态占位「—」，模块 8 再填；C# 版另有「本地文件」列，同样等模块 8）。**固定行高 48px 的自写虚拟滚动**（`lib/virtual.ts`，3000 首时 DOM 里只有约 24 行），表头与行共用同一 grid 列，滚动条槽位固定保证对齐，没有新增依赖。
- 状态：未登录、加载歌单列表中、列表失败（重试）、账号无歌单、未选歌单、曲目加载中（含「已加载 500/1832 首」与进度条）、曲目失败（重新加载）、空歌单、部分曲目缺失横幅（可重新加载）。后端错误同时写入运行日志。
- 开发辅助：`src/dev/mockBackend.ts` 在 `pnpm dev` 的普通浏览器里（没有 `__TAURI_INTERNALS__`）用 `?mock=loggedout|empty|listerror|slow` 装上假后端检查渲染；生产构建里 `import.meta.env.DEV` 恒为 false，该文件整体被摘除（已在 `dist` 里确认没有它的任何内容）。

### 与 C# 行为的差异（有意为之）

- 某一批 `song/detail` 失败时**重试一次**，仍失败才把该批略过并置 `is_partial`；业务 301 则立即向上返回错误走登录复核（C# 吞掉所有 `NeteaseApiException`，包括 301）。部分缺失时界面给出横幅与「重新加载」，而不是像 C# 那样整个加载失败。`is_partial` 的定义不变（`songs.len() < trackIds.len()`），所以歌单里有已下架歌曲时也会提示。
- 发行日期固定按东八区格式化（C# 按本机时区）：网易云的发行日期是北京时间 0 点，按 UTC 会少一天。
- 歌曲自带的 `privilege` 为 `null` 等非对象时回退到顶层 `privileges`（C# 会丢掉）。
- 没有“按歌单取消在途请求”：快速切换歌单时前端只采纳最后一次请求的结果，旧请求在后端跑完即丢弃。

### 新增依赖

无（Rust 与前端都没有新增）。

### 验证

- 单测：`lp-core` lib 261 个（模块 4 新增 62 个：mapper、歌曲详情、用户歌单分页、歌单详情 trackIds 分批 / 顺序 / 进度 / 部分缺失 / 301、`PlaylistService` 的 301 复核与错误分类），外壳 9 个，`csharp_compat` 5 个；`cargo fmt --check`、`clippy -D warnings`、`pnpm build`、`.\build.ps1 -Tauri` 通过。
- 联网探针（一次性，已删除）：匿名取 3 个公开歌单（热歌榜 200 首、飙升榜 100 首、抖音排行榜 81 首），名称 / 创建者 / 曲目 / 歌手 / 专辑 / 时长 / 别名 / 发行日期 / `maxbr` / `fee` 均正确解析，顺序与 `trackIds` 一致；`song/detail` 批量接口正常。
- 浏览器假后端：3000 首列表虚拟滚动（滚到 2500 附近 DOM 仍只有 24 行）、表头列与行列对齐、`Ctrl+B` 收起 / 展开、选中高亮、空歌单 / 曲目失败 / 部分缺失 / 未登录 / 列表失败各状态、浅色主题。
- 实机（exe 复制到 `D:\claude\lp-run\`、`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，未触碰真实登录态）：测试期间有真人扫码登录，随后看到了**真实账号的 16 个歌单**，封面正常；1235 首的大歌单走通了分批补齐（「歌单较大，正在补全 235 首歌曲详情…」→「加载完成，共 1235 首」，进度条显示 1000/1235），歌曲表格的标题 / 歌手 / 专辑 / 时长 / 别名显示正确。

## 模块 3（登录）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `netease::crypto` | `WeapiCrypto` | AES-128-CBC 双层加密 + 无填充 RSA（`num-bigint` 模幂）；C# 测试里的两组独立向量（Node.js 生成）原样搬来，结果逐字节一致。 |
| `netease::cookie_merger` | `CookieMerger` | 按名覆盖、`Max-Age` 优先于 `Expires`、空值不覆盖、保持顺序；Cookie 日期按 RFC 6265 §5.1.1 手写解析（支持 `01-Jan-1970` 形式、两位年份、asctime），不引入日期库。15 个 C# 用例全部移植。 |
| `netease::http` | `HttpClient` | `HttpTransport` trait（`BoxFuture`，dyn 兼容）；生产实现 `ReqwestTransport`：`reqwest 0.13` + `native-tls`（Windows 即 Schannel，系统证书与系统代理），不启用 cookie 容器；测试用 `test_util::FakeTransport`（按序返回、记录请求、可挂起响应制造“在途”窗口），**测试完全不联网**。 |
| `netease::client` | `NeteaseApiClient.cs` | 会话 + 会话代数（`SetSession` 后在途旧请求的 Set-Cookie 不再合并）；`post_weapi` / `get_json` / 裸表单；独立 20 秒超时（`tokio::time::timeout`，与传输层超时无关）；合并后会丢 `MUSIC_U` 时忽略；变化时同步通知订阅者（回调里 panic 会被隔离）；`build_playback_cookie_header` 去重；`NeteaseError`（`code` / `is_http_status` / `is_not_logged_in()` 仅业务 301 / `is_transient_network()` / `user_message()`）。 |
| `netease::login` | `NeteaseApiClient.Login.cs` | `create_qr_login`（桌面端 `type=3`，任何失败降级 web `type=1`）、`poll_qr_login`（801 / 802 / 803 / 800 / 其他，轮询回到出码时的 host 与 type）、803 后合并 Set-Cookie 与响应体 cookie、`__csrf_token` → `__csrf` 别名、读取资料失败仍保留 Cookie 登录态、`get_current_user`、`refresh_login`（新增 `try_refresh_login` 带出失败原因）。 |
| `session::SessionAutoSaver` | `SessionAutoSaver` | 订阅客户端会话变化；写入时取**当前**会话、连续变化合并、已退出则跳过、`flush()` 排空；写盘在 `spawn_blocking` 里串行执行。存储抽象成 `SessionStorage` trait（`SessionStore` 是文件实现，测试用 `RecordingStore`，可用闸门卡住写入来构造竞争）。 |
| `auth::LoginManager` | `MainWindowViewModel` / `QrLoginViewModel` 里的登录逻辑 | `restore`（同步读 `session.dat`）、`validate`（启动校验）、`qr_start` / `qr_poll`、`logout`、`handle_runtime_auth_error`、`ensure_keepalive`、`shutdown`；见下文不变量。 |
| `logs::LogHub` | 日志列表 | 最多 400 条的内存环形缓冲 + 实时监听器；条目 `id` 单调递增，前端启动时先监听、再 `get_logs`、按 `id` 去重，不漏也不重复。 |
| `qr` | QRCoder | `qrcode` 0.14（只开 `svg`），纠错级别 Q，白底黑码含静区，深色主题下也能扫。前端用 `data:image/svg+xml` 的 `<img>` 显示，不需要二维码库。 |

### 登录不变量（与 C# v1.3 一致，均有单测）

- **「登录失效」只认接口业务 `code == 301`**，或 `account/get` 返回 200 但 account / profile 为空；HTTP 401 / 302 / 5xx、406、-460、超时、网络错误一律只记“暂时无法验证登录状态，已保留登录”，不清除。
- 判定失效前先 `refresh_login` 再复核一次，仍明确未登录才清除（`301 → 续期成功 → 恢复`、`301 → 仍未登录 → 清除`、`复核时网络错误 → 保留` 都有用例）。
- 清除登录态顺序：先 `set_session(None)`、再 `flush` 自动落盘、最后删 `session.dat`，已排队的落盘因“当前已无会话”被跳过，不会写回（用写盘闸门构造了竞争场景）。
- 运行期 301 复核同一时间只做一次（RAII 复位标志，复核被取消也会放开）。
- 定时续期默认 12 小时，间隔可注入（单测用暂停时钟推进）；首次在“启动校验”或“扫码成功”时启动，幂等，可停止、可重启。
- 扫码确认时先落盘再让登录态生效；保存失败则不留“内存已登录、重启就丢”的半截状态。

### 与 C# 行为的差异（有意为之）

- 扫码轮询遇到**暂时性网络错误**（超时 / 连接失败）时，前端最多连续容忍 3 次再提示刷新；C# 把它们都包成 `NeteaseApiException`，一次失败就报错。
- 803 拿到 Cookie 却没读到资料时，Rust 在后台补取一次账号资料并刷新界面。
- 续期失败的日志带原因：`登录态续期未成功（续期登录状态失败：接口返回 code=400），将继续使用当前登录状态`。
- JSON 用 `serde_json::Value`（键按字母序），C# 是插入序；服务端不关心键序，`weapi` 业务 JSON 的解密结果只在内容上与 C# 相同。

### Tauri 外壳与前端

- 命令：`qr_start`（key + SVG + 有效期）、`qr_poll`（错误折叠进返回值：`state` 为 `waiting` / `scanned` / `expired` / `failed` / `success` / `error`，`transient` 标记可重试）、`login_status`、`validate_login`、`logout`、`recheck_login`（给后续模块在 301 时调用）、`get_logs`、既有的 `get_app_state`（会话摘要新增完整账号信息，仍不含 Cookie）。
- 事件：`app:log`（新日志）、`auth:state`（登录成功 / 退出 / 失效 / 资料刷新，载荷是账号摘要）。应用退出（`RunEvent::Exit`）时停掉续期任务并 `flush` 自动落盘。
- 前端：标题栏右侧账号区（未登录「扫码登录」；已登录圆形头像 + 昵称 + 「黑胶 VIP」/「网易云用户」+「退出登录」，退出前有确认对话框）、玻璃卡片风格的模态扫码对话框（二维码、状态文字、过期 / 失败时「刷新二维码」、成功后自动关闭、约 2 秒轮询、关闭对话框即停止轮询、Esc / 点遮罩关闭）、底部日志面板（400 条，可收起，新日志自动滚动）。所有可交互控件都有中文 `aria-label`。头像 `http://` 在后端升级为 `https://`（CSP 不允许混合内容），前端追加 `?param=64y64`。

### 新增依赖

| crate | 版本 | 用途 |
|---|---|---|
| `tokio` | 1.54（`rt` `sync` `time`；测试加 `macros` `test-util`） | 超时、定时续期、`watch`、`spawn_blocking` |
| `reqwest` | 0.13.5（`default-features = false`，`native-tls` `http2` `system-proxy` `form` `gzip` `deflate`） | 生产 HTTP |
| `aes` / `cbc` | 0.9.3 / 0.2.1（`alloc`） | weapi AES-128-CBC |
| `num-bigint` | 0.5.1 | 无填充 RSA 模幂 |
| `base64` / `rand` | 0.23.1 / 0.10.3 | Base64、随机 secret |
| `qrcode` | 0.14.1（`svg`） | 二维码 SVG |

### 验证

- 单测：`lp-core` lib 199 个（模块 3 新增约 120 个，含 `weapi` 向量、`CookieMerger`、客户端 / 扫码 / 续期、`SessionAutoSaver`、`LoginManager` 编排、日志、二维码），外壳 7 个，`csharp_compat` 5 个；`.\build.ps1 -Test` C# 仍 404/404；`cargo fmt --check`、`clippy -D warnings`、`pnpm build`、`.\build.ps1 -Tauri` 全部通过；`lp-core` 连跑 15 次无抖动。
- 联网探针（一次性，已删除）：`unikey`（桌面端出码）拿到 36 位 key，轮询返回 801（`等待扫码`），二维码 SVG 生成成功。
- 实机（exe 复制到 `D:\claude\`、`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，未触碰真实登录态；测完已清理）：「扫码登录」按钮、对话框、二维码与「请使用网易云音乐 App 扫码」显示正常。测试期间有真人用手机扫了码，因此顺带走通了**真实的 803 确认流程**：登录成功、头像 / 昵称 / 「黑胶 VIP」显示、日志「登录成功」；重启后 DPAPI 的 `session.dat` 恢复登录、启动校验「账号信息同步完成」；退出登录确认对话框 → `session.dat` 被删除、回到「扫码登录」。

### 已知问题与待真机观察

- **`/weapi/login/token/refresh` 对这次桌面端扫码得到的会话返回 `code=400`**（日志：`登录态续期未成功（续期登录状态失败：接口返回 code=400）`）。请求与 C# 完全一致，所以 C# 版大概率同样如此；登录态本身有效（`account/get` 成功）。这正是 AGENTS.md 里“续期效果需真机长期观察”的那一点：目前能确认的是“Set-Cookie 合并 + 落盘”这条路径有单测，但服务端是否真的靠 refresh 下发新 `MUSIC_U` 还没有实证。若长期观察发现 `MUSIC_U` 到期掉线，需要改查 `eapi` 续期端点。
- 需要用户真机验证的点：**①** 长时间运行（超过 12 小时 / 数天）后登录是否保持；**②** 真正的 301（`MUSIC_U` 被服务端作废）时“先续期再复核、仍未登录才清除”的体验与日志；**③** 对话框关闭后确实不再发轮询请求（实现上由 `cancelled` 标志保证，未做网络层抓包）。
- 前端没有自动化测试（与 C# 的 ViewModel 一样靠实机验证）。

## 模块 2（基础层）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `paths` | `AppPaths.cs` | 数据目录：环境变量（新名优先，空白再看旧名）→ 程序目录下已有 `data\`（便携）→ `%LOCALAPPDATA%\LunaticPrincess`（只有旧目录 `NeteaseMusicManager` 时整体迁移）→ 程序目录 `data\` → `%TEMP%\LunaticPrincess`；每个候选都做真实写入探测，不可写就降级。默认下载目录同理（便携 `data\Music` → `%MyMusic%\LunaticPrincess`，旧目录已存在则沿用 → `data\Music` → `%TEMP%`）。环境变量、程序目录、已知文件夹收拢成 `PathEnv`，解析逻辑可用临时目录完整单测；已知文件夹用 `SHGetKnownFolderPath`。 |
| `settings` / `models` | `AppSettings.cs`、`JsonSettingsStore.cs` | 21 个字段、默认值、`normalize()`（并发 clamp 1–8，阈值银行家舍入到 2 位并 clamp 0.30–1.00，NCM 文件夹去重等）；camelCase、枚举按名字、键名读取时不区分大小写；旧值迁移 `Higher`→`ExHigh`、未知音质丢弃；临时文件 + rename 原子写入。 |
| `session` | `NeteaseSession.cs`、`ProtectedSessionStore.cs` | 文件头 `NMMSES01` + mode（1=DPAPI，2=明文）+ payload；DPAPI 为 CurrentUser、entropy `LunaticPrincess.Session.v1`，与 C# 完全一致。`DataProtector` trait 抽象加密，单测用 XOR 假实现，Windows 上另有真实 DPAPI 往返测试。`load` 区分“文件不存在 / 没有可用 Cookie”（`Ok(None)`）与“损坏、无法解密、无法解析、读取失败”（`Err` + `last_load_failure()`，原因文案与 C# 相同）。 |
| `error` | `ErrorText.cs` | 新增 `Json` / `Dpapi` / `Path` / `Session` 变体；IO 错误按权限不足、占用、磁盘已满、路径过长给出中文提示。 |

Windows API 选用 `windows-sys` 0.61 而不是 `windows`：只需要 `CryptProtectData` / `CryptUnprotectData` / `LocalFree` / `SHGetKnownFolderPath` 几个 C 接口，`windows-sys` 是纯 FFI 声明、没有 COM 包装，并且 Tauri 已经依赖同一版本，不增加编译量。

Tauri 侧：启动时解析路径、加载设置与登录态并放入托管状态 `AppState`；命令 `get_app_state` 返回数据目录、便携模式、设置摘要与登录摘要（只有是否登录与昵称，**不含 Cookie**），读取失败的中文原因一并返回，前端占位卡片显示「数据目录 / 便携模式 / 已保存登录」。

### 与 C# 行为的差异（有意为之）

- 设置文件里某个字段类型错误（例如枚举写成未知名字）时，C# 会整份重置为默认值，Rust 只让该字段回落到默认，其余保持。文件整体无法解析 / 不是对象时两边都用默认设置。
- 设置读取遇到“不存在 / 损坏”以外的 IO 错误（权限不足、被占用）时，Rust 返回 `Err`，外壳改用默认设置并在界面提示；C# 对 `IOException` 静默使用默认值。
- `NeteaseSession.logged_in_at` 以 ISO 8601 字符串原样保存（C# 的 `DateTimeOffset` 序列化形式），无损往返；Rust 自己生成时使用 UTC（`+00:00`），C# 可直接解析。

### 兼容性实证

用临时 C# 探针（调用现有 `ProtectedSessionStore` / `JsonSettingsStore`，数据全是假的：`MUSIC_U=fake; __csrf=abc`、昵称「测试」，探针放在仓库外并已删除）与 Rust 互读：

- C# 写 → Rust 读：`settings.json`（21 个字段全部取非默认值）与默认设置逐字段一致；DPAPI 的 `session.dat` 读出的 cookie / 昵称 / userId / vipType / 头像 / `loggedInAt` 完全一致。
- Rust 写 → C# 读：`settings.json` 21 个字段正确；`session.dat`（含 Rust 自己生成的默认 `loggedInAt`）被 C# 正常读取，`LastLoadFailure` 为空。
- 无隐私的 C# 输出已作为夹具提交在 `app/crates/lp-core/tests/fixtures/`（`settings.csharp-*.json`、`session.csharp.json`、`session.csharp-mode2.dat`），`tests/csharp_compat.rs` 校验字段、键顺序与往返。DPAPI 文件绑定 Windows 用户，无法作为夹具，所以互读 DPAPI 只记录在这里。

### 已知环境问题：仓库位于 Edge 下载文件夹时 WebView2 创建失败

现象：在 `D:\edge浏览器下载\DSH\…` 下直接运行 `LunaticPrincess.exe`，`WebView2 error: 0x800700AA`（请求的资源正在使用中）；复制到 `D:\claude\…` 等其他位置正常。

**根因不是中文路径，也不是代码页**。`D:\edge浏览器下载` 是 Edge 的默认下载文件夹，带有 **Low Mandatory Level（低完整性级别）** 的强制标签，并向下继承给其中的每个文件。进程的完整性级别取“父进程”与“可执行文件”中较低者，所以放在该目录树下的 exe（包括 `cargo test` 生成的测试程序、仓库里的 `.dotnet\dotnet.exe`）都以低完整性运行，不能写入普通完整性的位置（如 `%LOCALAPPDATA%`、`%TEMP%`、`D:\claude\`），报 `os error 5`。Tauri 默认把 WebView2 用户数据目录放在 `%LOCALAPPDATA%\<identifier>\EBWebView`，低完整性进程无法使用，于是创建环境失败。用同样的目录名放到别的位置（`D:\claude\载`、`D:\claude\edge浏览器下载`、很深的路径）都正常，只有直接位于该文件夹下才失败；探针程序复制到该目录树下也复现了“无法在 `D:\claude` 建目录”。

修复：
1. 主窗口改为在 `setup` 里用 `WebviewWindowBuilder::from_config`（`tauri.conf.json` 中 `create: false`）创建，并显式 `data_directory(<数据目录>\webview)`，窗口尺寸、无边框、透明、Mica→Acrylic 回退不变。
2. 数据目录解析本身带写入探测：低完整性进程写不进 `%LOCALAPPDATA%` 时会自动降级到程序目录下的 `data\`（便携模式），而该目录同样位于低完整性目录树内、可写，因此 `webview\` 也可写。
3. `build.ps1 -Tauri` 把 `TMP` / `TEMP` 指到工作区 `.tmp`（与 .NET 流程一致），否则测试程序在该仓库路径下无法创建临时目录。

验证：在仓库原路径运行 `app\target\release\LunaticPrincess.exe`，窗口与卡片正常显示（数据目录落在程序目录 `data\` 或显式指定的、低完整性可写的目录）。**副作用**：在这种位置运行时，登录态与设置会落在程序目录 `data\` 而不是 `%LOCALAPPDATA%\LunaticPrincess`，与用普通方式启动时不是同一份；若要共用，请把仓库移出 Edge 下载文件夹，或把 exe 复制到普通位置运行。根治办法是移走仓库或去掉该文件夹的低完整性标签（属于系统安全设置，本项目不代为修改）。
