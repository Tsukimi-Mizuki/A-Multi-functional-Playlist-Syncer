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
| `models` | `Core/Models/` 中的设置用枚举，以及 `ThemeMode`、`LyricsSourceKind`（后续模块陆续补充） |
| `session` | `Core/Models/NeteaseSession.cs`、`Core/Storage/`（`ProtectedSessionStore`、`SessionAutoSaver`） |
| `auth` | 原来放在 `MainWindowViewModel` / `QrLoginViewModel` 里的登录编排（启动校验、续期、扫码落盘、退出、运行期复核） |
| `logs` | `MainWindowViewModel` 里的运行日志列表（400 条、内存） |
| `qr` | `QrLoginViewModel.RenderQrCode`（QRCoder → `qrcode` SVG） |
| `netease` | `Core/Netease/`（`models` / `mappers` / `playlists` / `songs` / `lyrics` 对应 `NeteaseModels`、`NeteaseMappers`、`NeteaseApiClient.Playlists/Songs/Lyrics`） |
| `playlist` | 原来放在 `MainWindowViewModel` 里的歌单加载编排（取列表 / 曲目、写日志、301 时走登录复核） |
| `ncm` | `Core/Ncm/` |
| `lyrics` | `Core/Lyrics/`（模块 5：`models` / `parser` / `qrc` / `krc` / `service`；`LyricsMatchScorer`、`BatchLyricsService` 在模块 10） |
| `tags` | `Core/Tags/` |
| `library` | `Core/Library/` |
| `download` | `Core/Download/` |
| `sync` | `Core/Sync/` |
| `online` | `Core/Online/` |
| `error` | `Core/Utilities/ErrorText.cs` |

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
| 6 | 标签读写 | MP3 / FLAC / M4A / OGG 标签、封面、歌词写入 | 待做 |
| 7 | NCM 解密与导入 | `.ncm` 解密、元数据与封面回写、导入窗口 | 待做 |
| 8 | 本地曲库与对比 | 曲库扫描、与歌单一对一匹配 | 待做 |
| 9 | 下载与同步 | 九档音质取链下载、一键同步编排 | 待做 |
| 10 | 在线匹配 | 网易云 / QQ / 酷狗 / LRCLIB、歌词与标签编辑器、批量匹配 | 待做 |
| 11 | 设置与外观 | 设置窗口、主题切换、帧率等 | 待做 |
| 12 | CI 打包、文档、删除 C# | 安装包 / zip、GitHub Actions、更新 AGENTS.md 与 README、删除 C# 代码 | 待做 |

## 待修问题

以下问题已确认、暂不处理；**用户决定等全部模块完成后再统一修复**，不要在中途模块里顺手改。

- **`/weapi/login/token/refresh` 实测返回 `code=400`**：桌面端扫码得到的会话调用续期接口时服务端拒绝（日志 `登录态续期未成功（续期登录状态失败：接口返回 code=400）`），请求与 C# 完全一致，所以 C# v1.3 同样受影响。续期是否真的生效尚未证实；登录态本身有效（`account/get` 成功）。需要长期观察 `MUSIC_U` 是否到期掉线，必要时改查 `eapi` 续期端点。详见「模块 3」的已知问题。

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
