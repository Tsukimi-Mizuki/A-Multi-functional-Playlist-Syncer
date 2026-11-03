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
| `ncm` | `Core/Ncm/`（模块 6：`client_comment` = `NeteaseClientComment`；模块 7：`decoder` = `NcmDecoder` / `INcmDecoder`） |
| `lyrics` | `Core/Lyrics/`（模块 5：`models` / `parser` / `qrc` / `krc` / `service`；`LyricsMatchScorer`、`BatchLyricsService` 在模块 10） |
| `tags` | `Core/Tags/`（模块 6：`model` / `service` / `text` / `view`；`service` 对应 `AudioTagService`） |
| `utilities` | `Core/Utilities/`（模块 6：`audio_file_types`；模块 7：`error_text`、`file_name_sanitizer`、`cancel`） |
| `library` | `Core/Library/`（模块 8：`track` / `scanner` / `matcher` / `text` / `view` 对应 `ILocalLibraryService.cs`、`LocalLibraryService`、`SongMatcher`；`manager` 对应 `MainWindowViewModel` 里的扫描缓存与对比） |
| `download` | `Core/Download/`（模块 7：`ncm_import` = `NcmImportService`；下载与同步在模块 9） |
| `sync` | `Core/Sync/` |
| `online` | `Core/Online/` |
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
| 9 | 下载与同步 | 九档音质取链下载、一键同步编排 | 待做 |
| 10 | 在线匹配 | 网易云 / QQ / 酷狗 / LRCLIB、歌词与标签编辑器、批量匹配 | 待做 |
| 11 | 设置与外观 | 设置窗口、主题切换、帧率等 | 待做 |
| 12 | CI 打包、文档、删除 C# | 安装包 / zip、GitHub Actions、更新 AGENTS.md 与 README、删除 C# 代码 | 待做 |

## 待修问题

以下问题已确认、暂不处理；**用户决定等全部模块完成后再统一修复**，不要在中途模块里顺手改。

- **`/weapi/login/token/refresh` 实测返回 `code=400`**：桌面端扫码得到的会话调用续期接口时服务端拒绝（日志 `登录态续期未成功（续期登录状态失败：接口返回 code=400）`），请求与 C# 完全一致，所以 C# v1.3 同样受影响。续期是否真的生效尚未证实；登录态本身有效（`account/get` 成功）。需要长期观察 `MUSIC_U` 是否到期掉线，必要时改查 `eapi` 续期端点。详见「模块 3」的已知问题。

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
- 模块 9 的下载 / 同步完成后需要调用 `scan_library` 刷新本地状态（C# 在同步结束后会重新扫描并对比）。
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

### 在线补全挂钩（待模块 10 接入）

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

- 在线补全未接入（见上），勾选后只显示“将在后续版本提供”。
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
