# AGENTS.md — 协作约定与项目交接

> 本文件既是**长期协作约定**，也是**交接说明书**。接手本项目的人或 AI 助手请先通读，再按 §2 做一遍自检。
>
> 配套文档：`docs/ARCHITECTURE.md`（分层、关键流程、窗口效果细节）、`docs/IMPLEMENTATION_STATUS.md`（逐项实现与验证记录、已知限制）、
> `README.md`（面向用户的说明）、`THIRD_PARTY_NOTICES.md`（第三方组件与许可证）。

## 1. 项目概况

| 项目 | 内容 |
|---|---|
| 名称 | **Lunatic Princess**（旧名 NeteaseMusicManager，已整体更名；设置/登录态目录会自动迁移） |
| 定位 | Windows 桌面端网易云音乐管理器：扫码登录 → 歌单浏览 → 本地曲库对比 → 按音质下载 → NCM 解密 → 歌词/标签/封面写入；另带在线歌词与标签匹配（网易云 / QQ 音乐 / 酷狗 / LRCLIB） |
| 平台 | Windows 10/11 x64（Avalonia 桌面）。Android 只在 Core 层预留接口，**未实现** |
| 技术栈 | .NET 8 / C# 12、Avalonia 11.3、CommunityToolkit.Mvvm、TagLibSharp、xUnit |
| 许可证 | AGPL-3.0（见 `LICENSE`）。仅用于个人备份自己有权访问的音乐，不分发音频、不绕过版权与会员限制 |
| 仓库 | https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer |
| 当前状态 | v1.2 已发布（GitHub Release 自包含单文件 zip）；桌面端功能完整；**404/404 单测通过**，0 warning / 0 error |
| 分支 | `main` 为开发与发布主分支；`tauri-experiment` 已删除；`GUI-Tauri` 为 Rust + Tauri 2 + React 的彻底重写分支，进度见 `docs/TAURI_REWRITE.md` |

## 2. 接手后先做的自检（约 15 分钟）

1. **环境**：Windows 10/11 x64 + PowerShell 5.1 以上。SDK 版本由 `global.json` 锁定（8.0.426，rollForward = latestFeature）；仓库自带 `.dotnet\dotnet.exe`（8.0.426），`build.ps1` 会优先使用它，无需另外安装。
2. **跑基线**：`.\build.ps1 -Test` → 预期 0 warning / 0 error、**404/404 通过**（1–2 分钟）。
3. **出包并试用**：`.\build.ps1 -Publish` → `publish\win-x64\LunaticPrincess.exe` 与 `publish\LunaticPrincess-win-x64.zip`；双击 exe，用**自己的**账号扫码登录，确认歌单加载、本地对比、下载、NCM 导入四个链路可用。
4. **读文档**：`docs/ARCHITECTURE.md` 的「关键流程」「外观与窗口效果」，再看 `docs/IMPLEMENTATION_STATUS.md` 的「验证记录 / 已知限制」。
5. **确认干净**：`git status` 应无改动；`publish/`、`.tmp/`、`.dotnet/` 等都已 gitignore，不要在仓库里留临时脚本或登录态。

## 3. 技术栈与版本（改依赖前先看这里）

| 项 | 版本 / 说明 |
|---|---|
| .NET SDK | 8.0.426（`global.json`，rollForward latestFeature，allowPrerelease false） |
| 目标框架 | `net8.0`（Core / Desktop / Tests 一致） |
| C# | `LangVersion=latest`；`Nullable` 与 `ImplicitUsings` 开启（`Directory.Build.props`） |
| Avalonia | 11.3.22（`Avalonia.Controls.DataGrid` 11.3.13；`Avalonia.Diagnostics` 仅在 Debug 引入） |
| MVVM | CommunityToolkit.Mvvm 8.4.2（源生成器：`[ObservableProperty]` / `[RelayCommand]`） |
| 标签读写 | TagLibSharp 2.3.0 |
| 二维码 | QRCoder 1.8.1 |
| 登录态加密 | System.Security.Cryptography.ProtectedData 8.0.0（DPAPI，CurrentUser） |
| 测试 | xUnit 2.9.2 + Microsoft.NET.Test.Sdk 17.11.1 + coverlet.collector 6.0.2 |
| 其他全局设置 | `Deterministic=true`、`NuGetAudit=false`、`GenerateDocumentationFile=false`、单一 NuGet 源（`NuGet.Config` 清空了默认源） |

## 4. 目录结构与职责

```
LunaticPrincess.sln
global.json / Directory.Build.props / NuGet.Config / build.ps1
src/
  LunaticPrincess.Core/          # 与 UI 无关的业务层，必须可单测
    Netease/                     # 登录、weapi 加密、歌单/歌曲/歌词/专辑 API
    Ncm/                         # NCM 解密、客户端注释里的 musicId 解析
    Lyrics/                      # LRC / 增强 LRC / YRC / QRC / KRC 解析、匹配打分、批量匹配
    Tags/                        # TagLib 标签读写
    Library/                     # 本地扫描、歌单差异匹配
    Download/                    # 下载、NCM 导入
    Sync/                        # 一键同步编排（本地命中 / NCM 解密 / 下载）
    Online/                      # 网易云 + QQ 搜索、酷狗与 LRCLIB 备用词源
    Storage/ Settings/ Models/   # DPAPI 登录态、设置、领域模型与枚举
    Utilities/                   # 错误文案、文件名净化、音频类型判定
  LunaticPrincess.Desktop/       # Avalonia GUI（View + ViewModel + Services）
tests/
  LunaticPrincess.Core.Tests/    # Core 单测，不依赖网络
.github/workflows/main.yml       # tag 触发的自动构建与 Release
docs/                            # ARCHITECTURE / IMPLEMENTATION_STATUS / design
publish/                         # 发布产物（gitignore；data 子目录是运行时登录态，勿删勿传）
```

Core 各命名空间的关键类型：

| 目录 | 主要类型 | 说明 |
|---|---|---|
| Netease | `NeteaseApiClient`（分文件：Login/Playlists/Songs/Lyrics/Albums/Search）、`WeapiCrypto`、`NeteaseMappers`、`NeteaseJson` | 非公开接口，按端点分 partial 文件维护 |
| Ncm | `NcmDecoder`、`INcmDecoder`、`NeteaseClientComment` | AES-128-ECB 密钥盒 + 元数据，32 KB 分块 XOR |
| Lyrics | `LyricParser`、`KrcLyricsParser`、`QrcDecryptor`、`QqTripleDes`、`LyricsMatchScorer`、`BatchLyricsService`、`ILyricsSource` | 逐行/逐字、双语、打分与批量匹配 |
| Tags | `AudioTagService`、`IAudioTagService` | MP3/FLAC/M4A/OGG 等容器 |
| Library | `LocalLibraryService`、`SongMatcher` | 扫描 + 一对一匹配 |
| Download | `DownloadService`、`NcmImportService`、`MediaFileHelper` | 取链、写标签、NCM 导入 |
| Sync | `MusicSyncService` | 按本地状态编排三种分支 |
| Online | `OnlineMusicSearchService`、`KugouLyricsSource`、`LrclibLyricsSource`、`LyricsSourceKind` | 主源 + 备用词源 |
| Storage / Settings | `ProtectedSessionStore`、`JsonSettingsStore`、`AppSettings`、`AppPaths` | 登录态、设置、路径解析 |

Desktop 侧重点文件：

| 文件 | 职责 |
|---|---|
| `Services/AppServices.cs` | **组合根**：唯一构造真实实现的地方，ViewModel 只拿 Core 接口（新建服务的顺序在这里，见 §13） |
| `ViewModels/MainWindowViewModel.cs` | 登录/歌单/对比/下载同步/拖放载入/日志/工具栏命令 |
| `ViewModels/LyricsEditorViewModel.cs` | 歌词与标签编辑器、在线搜索、保存 |
| `ViewModels/BatchLyricsViewModel.cs` | 批量获取标签/封面/歌词窗口 |
| `ViewModels/NcmImportViewModel.cs` | NCM 导入窗口（文件夹记忆、解密后自动匹配） |
| `ViewModels/SettingsViewModel.cs` | 设置窗口（**保存时构造新的 AppSettings，必须显式带回不在界面上的字段**） |
| `Services/ThemeService.cs`、`ThemeBrushes.cs`、`VisualThemeManager.cs` | 主题切换、DWM 客户区颜色、二级窗口 Acrylic |
| `Services/RenderTimerInstaller.cs` | 帧率上限（Win32 渲染计时器接管） |
| `Services/DropOverlayController.cs`、`ElevationHelper.cs` | 拖放遮罩延迟隐藏、管理员运行时的拖放限制提示 |
| `Services/DesktopUiService.cs`、`ErrorFormatter.cs`、`CoverImageService.cs` | 文件选择器、异常转中文、封面下载 |
| `MainWindow.axaml(.cs)`、`App.axaml(.cs)` | 主窗口与全局资源（两套主题字典）、启动流程、`--lyrics/--edit` 参数 |

## 5. 构建 / 测试 / 发布

### build.ps1（推荐入口）

| 参数 | 作用 |
|---|---|
| `-Configuration` | `Debug`（默认）或 `Release` |
| `-Test` | 构建后跑全量单测 |
| `-Publish` | 发布到 `publish\win-x64` 并打包 zip |
| `-SelfContained` | 与 `-Publish` 合用；默认是框架依赖包（zip 约 11 MB，目标机需 .NET 8 Desktop Runtime），自包含包约 45 MB |

脚本做的事：把 `DOTNET_CLI_HOME / TMP / TEMP / NUGET_PACKAGES / NUGET_HTTP_CACHE_PATH / NUGET_PLUGINS_CACHE_PATH` 全部指到工作区目录（用户临时目录在受限环境可能不可写）→
`dotnet restore --configfile NuGet.Config` → `dotnet build --no-restore` → 可选 `test --no-build` → 可选 `publish -c Release -r win-x64` 到 `publish\win-x64`
（先清空除 `data` 外的旧文件，再裁掉 `Avalonia.FreeDesktop/Native/X11`、`Tmds.DBus.Protocol`、`Avalonia.DesignerSupport` 等非 Windows 程序集）→ `Compress-Archive` 生成 `publish\LunaticPrincess-win-x64.zip`（**排除 data**，避免把登录态打进包）。

注意事项：
- 发布前先停掉正在运行的程序：`Get-Process LunaticPrincess | Stop-Process -Force`（否则 DLL 被占用，publish 失败）。
- `publish\win-x64\data`（`session.dat`、`settings.json`）是**真实用户的登录态与设置**：脚本会保留它，不要手删；也不要提交或上传。
- 本地发布不设置 `Version`（程序集版本仍是 1.0.0.0），只有 CI 用 `-p:Version` 打版本号。

### 手动 dotnet 命令

必须先把 `DOTNET_CLI_HOME / TMP / TEMP / NUGET_PACKAGES / NUGET_HTTP_CACHE_PATH / NUGET_PLUGINS_CACHE_PATH` 指到工作区目录，并使用仓库内 SDK：

```powershell
$env:DOTNET_CLI_HOME = (Join-Path (Get-Location) '.dotnet-home')
$env:TMP = (Join-Path (Get-Location) '.tmp'); $env:TEMP = $env:TMP
$env:NUGET_PACKAGES = (Join-Path (Get-Location) '.nuget-packages')
$env:NUGET_HTTP_CACHE_PATH = (Join-Path (Get-Location) '.nuget-http-cache')
$env:NUGET_PLUGINS_CACHE_PATH = (Join-Path (Get-Location) '.nuget-plugins-cache')
.\.dotnet\dotnet.exe test tests\LunaticPrincess.Core.Tests\LunaticPrincess.Core.Tests.csproj -c Debug --filter "FullyQualifiedName~LyricsMatchScorer"
```

### 运行与调试

- Debug 可执行文件：`src\LunaticPrincess.Desktop\bin\Debug\net8.0\LunaticPrincess.exe`。
- 支持 `--lyrics <文件>` / `--edit <文件>` 启动后直接打开歌词编辑器（`Program.GetStartupLyricsFile`）。
- 运行日志只在内存（最多 400 条，不落盘）；只有未处理异常会写入 `<数据目录>\logs\desktop-fatal.log`。

## 6. 运行时数据、路径与隐私

数据目录解析顺序（`Core/AppPaths.cs`）：

1. 环境变量 `LUNATIC_PRINCESS_DATA_DIR`（旧名 `NETEASE_MUSIC_MANAGER_DATA_DIR` 仍兼容，新变量为空时才看旧变量）；
2. 程序目录下已存在 `data\` → 便携模式，固定使用它（保证换启动方式后登录态不变）；
3. `%LOCALAPPDATA%\LunaticPrincess`；若只有旧目录 `%LOCALAPPDATA%\NeteaseMusicManager` 会先整体迁移；
4. 程序目录 `data\`；
5. `%TEMP%\LunaticPrincess`。

数据目录里的文件：

| 文件 | 说明 |
|---|---|
| `settings.json` | 明文 JSON（`JsonSerializerDefaults.Web` 即 camelCase + 枚举按名字序列化） |
| `session.dat` | 登录 Cookie，DPAPI（CurrentUser）加密；换 Windows 账号或换机器**无法解密**，需重新扫码 |
| `logs\desktop-fatal.log` | 仅致命异常追加日志 |

默认下载/曲库目录：便携模式为 `data\Music`；否则首选 `%MyMusic%\LunaticPrincess`（旧目录 `%MyMusic%\NeteaseMusicManager` 已存在则沿用，避免文件分裂）；再退到 `data\Music`；最后 `%TEMP%\LunaticPrincess\Music`。

`settings.json` 键（全部有默认值，缺字段就取默认；未知枚举值会被 `Normalize()` 归位）：

| 键 | 含义 | 默认 |
|---|---|---|
| downloadDirectory / libraryDirectory | 下载 / 曲库目录 | 见上 |
| quality | 音质档（`Standard/ExHigh/Lossless/HiRes/SpatialAudio/SurroundAudio/Master/AudioVivid/DolbyAtmos`） | ExHigh |
| lyricsMode | `None/Line/Word` | Line |
| writeCover / overwriteExisting / skipExisting | 封面与覆盖策略 | true / false / true |
| fileNameTemplate | 文件名模板 | `{artist} - {title}` |
| maxConcurrency | 下载并发（1–8） | 3 |
| autoMatchLocal | 启动后自动对比本地曲库 | true |
| tagSourcePriority / lyricsSourcePriority | `NeteaseFirst/QQMusicFirst/NeteaseOnly/QQMusicOnly` | NeteaseFirst |
| ncmImportFolders / ncmImportOutputDirectory | NCM 导入窗口记忆 | [] / 空（回退下载目录） |
| lyricsEditorSearchSource / lyricsEditorPreferWordLyrics | 编辑器记忆的搜索来源与歌词格式 | Netease / false |
| fallbackKugouLyrics / fallbackLrclibLyrics | 备用词源开关 | true / true |
| matchThreshold | 在线匹配度阈值（0.30–1.00） | 0.60 |
| theme | `Dark/Light/System` | Dark |
| renderFrameRate | `VSync` 或固定帧率 | VSync |

隐私与交接注意：`publish\win-x64\data` 里有本人账号的 `session.dat` 与个人路径，**转交仓库/机器前请删除或更换**；下载目录里可能有受版权保护的文件，同样不要随项目一起交付。

## 7. 架构速查（详细见 docs/ARCHITECTURE.md）

### 分层与组合根

`Desktop（Avalonia + MVVM）→ Core 接口 → Core 实现`。所有真实依赖在 `Services/AppServices.Create()` 里构造一次（HttpClient 由它持有并释放），ViewModel 只接收接口；Core 不引用 Avalonia（唯一的 Windows 相关依赖是 DPAPI 的 ProtectedData，Android 需替换 `ISessionStore`）。

### 关键流程（改代码前先确认这些不变量）

- **扫码登录**：`unikey(type=3)` → 二维码 → 每约 2 秒轮询 `801/802/803/800` → 收集 Cookie → `NeteaseSession` → DPAPI 落盘；启动时用 `/weapi/nuser/account/get` 校验。**登录续期与 Cookie 合并**：`NeteaseApiClient` 对每个带会话的 weapi/GET 响应调用 `CookieMerger` 合并 Set-Cookie（按名覆盖、Max-Age<=0/过期即删除、空值不覆盖；合并后会丢 MUSIC_U 则忽略），变化时触发 `SessionChanged`，由 `SessionAutoSaver`（`AppServices.Create()` 装配）串行落盘；`RefreshLoginAsync` 调 `/weapi/login/token/refresh`，启动校验成功后调用一次，运行期每约 12 小时一次。**「登录失效」只认接口 `code==301` 或 `account/get` 返回 200 但 account/profile 为空**（`NeteaseApiException.IsNotLoggedIn`；HTTP 401/302/5xx、406、-460、超时一律不清除登录态）；清除前先续期并复核一次。
- **歌单**：大歌单先取 `trackIds`，再按 500 首/批调用 `/weapi/v3/song/detail` 补齐并保持原顺序。
- **本地对比**：优先用网易云 ID（本程序写入的 `LunaticPrincess:<id>` 注释，或客户端文件里的 `163 key(Don't modify)` 解出的 `musicId`）精确匹配；否则标题/歌手/时长评分；再否则文件名猜测。每首本地文件最多匹配一首歌单曲。
- **下载**：九档音质 → `/api/song/enhance/player/url/v1`（取链请求带桌面客户端 Cookie，否则 Hi-Res 会被静默降级）→ 服务端降档时提示「实际音质」→ 临时文件 → 文件头是 NCM 就解密 → 歌词/翻译/封面/元数据 → TagLib 写入 → 原子移动 → 按 `BatchLyricsOptions.AutoEnrich` 在线补全（QQ 逐字优先、网易云兜底）。失败只作提示，不影响下载成功。
- **NCM 解密**：输出格式看首块（ID3 → mp3，fLaC → flac）；先写回内嵌元数据/封面/网易云 ID；导入窗口默认勾选「解密后自动获取在线标签、封面与歌词」。
- **在线匹配**：主源 `OnlineMusicSearchService`（网易云 `/api/search/get/web`、QQ `/soso/fcgi-bin/search_cp`）；都拿不到歌词时按 `ILyricsSource` 顺序回落酷狗 KRC、LRCLIB。打分与候选筛选集中在 `LyricsMatchScorer`，阈值来自设置。
- **同步**：`MusicSyncService` 按本地状态分三支（已存在且无覆盖 → 跳过；NCM → 解密 + 写标签 + 在线补全；缺失 → 下载）。
- **错误文案**：Core 的 `ErrorText` 识别权限不足/空间不足/文件占用/路径过长，Desktop 的 `ErrorFormatter` 统一转成中文说明与建议，**不把英文异常抛给用户**。

### 关键常量

| 项 | 值 | 位置 |
|---|---|---|
| 匹配度阈值 | 默认 0.60，范围 0.30–1.00 | `LyricsMatchScorer.DefaultMinScore/MinThreshold/MaxThreshold` |
| 打分权重 | 有歌手：标题 0.55 / 歌手 0.25 / 时长 0.20；无歌手：标题 0.72 / 时长 0.20 / 专辑 0.08；歌手相似度 < 0.30 整体 ×0.75 | `LyricsMatchScorer` |
| 单源候选数 | 每来源前 10 条、最多 3 组查询变体 | `LyricsMatchScorer` |
| 批量匹配并发 | 3 | `BatchLyricsService` |
| 下载并发 | 默认 3，clamp 1–8 | `AppSettings.Normalize` |
| 运行日志条数 | 400（只在内存） | `MainWindowViewModel` |
| NCM 分块 | 32 KB | `NcmDecoder` |
| 歌单详情批量 | 500 首/批 | `NeteaseApiClient.Playlists` |
| 二维码轮询 | 约 2 秒 | `QrLoginViewModel` / 登录服务 |

## 8. 代码约定

- **能力先落 Core，并带单测**；Desktop 只做编排与展示。新业务逻辑写在 ViewModel 里通常说明位置放错了。
- **依赖注入**：Core 构造函数接收接口（`HttpClient`、`ISettingsStore` 等），不 new 单例、不读环境变量（路径由调用方传入或走 `AppPaths`）；新服务必须同时加进 `AppServices.Create()`。
- **异步与取消**：I/O 一律 `async/await` + `CancellationToken`，进度用 `IProgress<T>`；不要使用 `.Result/.Wait()`；UI 更新走 `UiThread` / Avalonia Dispatcher。
- **中文文案**：所有用户可见文字用中文；异常经 `ErrorText`（Core）/ `ErrorFormatter`（Desktop）转换。
- **MVVM**：CommunityToolkit 源生成器（`[ObservableProperty]` 字段名小写、`[RelayCommand]` 生成 `XxxAsync` 命令）；可空引用保持开启，尽量不用 `null!` 压警告。
- **注释与文档**：XML 注释用中文摘要，解释「为什么」而不是复述代码；不提交注释掉的旧代码。
- **依赖**：不轻易加包。测试不引入 mock 框架（手写 Fake 够了），UI 不引入第二套控件库。
- **可访问性**：新增的可交互控件请补 `AutomationProperties.Name`：既是无障碍要求，也是本项目历史验证（UIA 自动化读控件/文案）依赖的锚点。
- **提交前**：跑 `.\build.ps1 -Test`；工作区不留临时脚本、探针、日志。

## 9. 测试约定

- 位置：`tests/LunaticPrincess.Core.Tests`（xUnit，只测 Core）。**不依赖网络、不依赖真实音频文件**。
- 测试夹具（都在测试项目内）：
  - `Tags/Mp3Fixture.cs`：生成 TagLib 可解析的最小合法 MP3（真实帧头）与 1×1 PNG 封面；
  - `Ncm/SyntheticNcmFile.cs`：按格式规范做**逆运算**生成真实可解的 .ncm（AES + RC4 变体），与生产实现互相独立；
  - `Lyrics/KrcFixture.cs`、`Netease/TestHttp.cs`（假 HttpMessageHandler 响应）、`Ncm/TempDirectory.cs`（临时目录帮手）。
- 双打：`Download/TestDoubles.cs`、`Sync/TestDoubles.cs`、`Library/TestDoubles.cs` 手写 Fake，记录调用参数用于断言。
- 当前分布（2026-10 记录，合计 404）：Netease API 101（含 Cookie 合并、续期、登录判定）/ NCM·歌词·标签 127 / 本地扫描·下载·同步 71 / 存储·设置·文件名 27（含 `SessionAutoSaver`、会话读取失败区分） / 音质映射 30 / 在线搜索 23 / 工具类 25。
- 加测试时同步更新 `docs/IMPLEMENTATION_STATUS.md` 的「自动化测试」总数与分项，以及本文件 §2 的数字。
- 真实网络验证：可以临时写探针跑一次（本项目历次都是这么验证的），**验证完立刻删除**，不要把依赖网络的测试提交进仓库，也不要放进会被 `dotnet test` 全量执行的路径。
- 跑单个测试：`--filter "FullyQualifiedName~ClassName"`（完整环境变量见 §5）。

## 10. 外部接口与实网经验（最容易失效的部分）

这些接口都不是公开 API，随时可能变；出问题优先怀疑接口而不是本地逻辑。

- **网易云**：`weapi` 需要 AES-CBC 双重加密 + 无填充 RSA 的 `encSecKey`；搜索接口连续多变体请求会返回 `code=406` 限流（匿名与登录态都可能）；取链 `/api/song/enhance/player/url/v1` 需要桌面客户端 Cookie 才能拿到 Hi-Res 及以上，无会员特权时服务端**静默降档**，所以要读响应里的 `level` 并提示实际音质。
- **QQ 音乐**：搜索旧接口 `client_search_cp` 已下线（两个域名都 500），现用 `/soso/fcgi-bin/search_cp` + `new_json=1`；`c.y.qq.com` 失败要自动切 `c6.y.qq.com`。歌词用 `music.musichallSong.PlayLyricInfo`（`qrc=1&crypt=1&roma=1`），QRC 是 3DES 变体：字节编号/位序与标准 DES 不同（已知向量：key `133457799BBCDFF1`、plain `0123456789ABCDEF` → `F63A972CE52AA327`，而非教科书的 `85E813540F0AB405`），解出后 zlib 解压再按 QRC 行/词时间戳解析。
- **酷狗**：三段链路 `mobilecdn.kugou.com/api/v3/search/song` → `krcs.kugou.com/search` → `lyrics.kugou.com/download?fmt=krc`；KRC 前 4 字节明文 `krc1`，之后 16 字节固定密钥 XOR 再 zlib；本机偶发证书域名不匹配，异常吞掉后回落下一个词源。
- **LRCLIB**：只有逐行 `syncedLyrics`；先用 `?q=` 多变体检索，落空再用 `track_name/artist_name` 兜底；429/503 属瞬时失败，退避后重试一次。
- **登录**：803 确认段必须真人手机扫码，自动化只能验证到 801；登录态靠 `login/token/refresh` + 合并后续响应的 Set-Cookie 续期（实际续期效果需真机长期观察）；若 `MUSIC_U` 真正过期或被服务端作废，仍需重新扫码。
- **版权**：不绕过会员/版权限制；试听片段直接判失败。
- 所有在线请求都应有独立超时与可读的中文错误；封面下载限制大小；解析失败保留本地已有内容。

## 11. 界面设计与视觉基线

- **界面设计一律以用户提供的设计参考为准**：对话中给出的设计稿/截图就是视觉基准，改配色、圆角、间距、控件形态、排版前先对照，不要自行更换视觉语言。
- 设计稿存 `docs/design/`（命名 `ui-<窗口或场景>.png`，如 `ui-main.png`），并在 `docs/design/README.md` 登记文件名与文字要点——无法看图的人（包括 AI）靠这些文字实现。
- 参考没覆盖到的细节沿用现有基线：Salt UI 风格卡片 + 胶囊控件 + 紧凑列表行；**液态玻璃**主题（深色 / 浅色 / 跟随系统，`App.axaml` 的 `ThemeDictionaries`，界面一律 `{DynamicResource *Brush}`，转换器资源才用 `StaticResource`）；无边框自绘标题栏（68px 顶栏即标题栏，自绘最小化/最大化/关闭按钮，DPI 感知）。
- 二级窗口统一走 `VisualThemeManager.ApplyDialog`（Acrylic + 半透明 `DialogGlassBrush`）与 `ScheduleClamp`（高 DPI 下夹回工作区）；受限环境（远程桌面/关闭透明效果）自动回退不透明背景。
- **改 UI 必须实机验证**：本项目历史做法是启动发布版，用 Windows UI 自动化读控件名称/文本/数值、必要时截图像素采样；至少确认窗口能开、关键文案正确、设置能保存、深色/浅色都不崩。
- 管理员身份运行时 Windows 会用 UIPI 阻断「资源管理器 → 应用」的拖放，这是系统限制，客户端只能提示（`ElevationHelper`），不要试图绕过。

## 12. Git、版本与 CI 发布

- 提交信息：**英文祈使句、一次提交一件事**（例：`Prefer QQ word lyrics for automatic online matching`、`Move the match threshold into settings as a text input`）。
- 提交身份：`Tsukimi-Mizuki <Tsukimi_Mizuki@outlook.com>`（仓库级 `git config`）。**交接后由新维护者改成自己的身份**，并考虑是否同步改 `LICENSE`/README 与仓库地址。
- 提交前：`.\build.ps1 -Test` 通过；`git status` 干净；不提交 `publish/`、`.tmp/`、`.dotnet/`、缓存与临时脚本（`.gitignore` 已覆盖）。
- **本地发版流程**：`git push origin main` → `git tag -a v1.3 -m "Lunatic Princess v1.3"` → `git push origin v1.3`。
  推送 `v*` tag 会触发 `.github/workflows/main.yml`（windows-latest）：`dotnet publish -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true -p:Version=<tag 去掉 v/V 与前导点>` → 打包 `Lunatic Princess-v<ver>.zip` → `softprops/action-gh-release@v2` 建 Release（自动生成 release notes）。
  手动 `workflow_dispatch` 没有 tag 时版本号固定为 `v0.0.1-manual`。
- Release 资产名里的空格在下载 URL 中会变成点：`Lunatic.Princess-v1.2.zip`（按 `/releases/expanded_assets/<tag>` 查真实资产名，直接拼 `%20` 会 404）。
- 历史 tag：`v0.0.1`、`v0.0.1-manual`、`v.1.0.0`（带前导点，CI 已兼容去点）、`v1.1`、`v1.2`。CI 只跑构建与发布，**不跑单元测试**；打 tag 前务必本地 `-Test`。
- 不要 force push 主分支。

## 13. 常见修改配方

1. **新增/调整音质档**：改 `Core/Models/AudioQuality.cs`（枚举 + `ToApiLevel()/ToApiLevels()/ToDisplayName()/ToBitrate()/FromApiLevel()`）→ 设置下拉自动枚举 → 补 `tests/.../Models/AudioQualityTests.cs` → 更新 `docs/ARCHITECTURE.md` 音质映射表。
2. **新增设置项**：`AppSettings` 加属性 + `Normalize()` 校验（非法值归默认）→ `SettingsViewModel` 读入、以及在 `SaveAsync` 构造新 `AppSettings` 时**显式带回**（不写就等于每次保存都重置）→ `SettingsWindow.axaml` 加卡片 → 若是枚举改名，在 `JsonSettingsStore.MigrateLegacyValues` 里做旧值迁移 → 补设置往返测试。
3. **新增在线词源**：实现 `ILyricsSource`（`Label` + 获取方法）→ 加进 `AppServices.Create()` 的 `fallbackLyrics` 数组 → 若要可开关，加 `AppSettings` 布尔项并在 `AppServices.ApplyFallbackLyricsSettings` 里映射 → 若要出现在编辑器下拉，扩展 `LyricsSourceKind` 与 `LyricsEditorViewModel`。
4. **新增窗口**：`Views/XxxWindow.axaml(.cs)` + `ViewModels/XxxViewModel.cs` + 在组合根注入依赖 → 打开时设置 Owner 并调用 `VisualThemeManager.ApplyDialog` → 可交互控件补 `AutomationProperties.Name` → 实机验证深色/浅色。
5. **改错误提示**：优先扩 `Core/Utilities/ErrorText.cs`（把异常映射成「是什么 + 怎么办」的中文），Desktop 侧只经 `ErrorFormatter` 转发；补 `Utilities/ErrorTextTests.cs`。
6. **动 NCM / 歌词写入**：先读 `docs/ARCHITECTURE.md` 对应章节与现有测试；`SyntheticNcmFile` + `Mp3Fixture` 可以造出真实可用的样本做端到端验证。

## 14. 已知限制与技术债

- 没有本地播放功能：定位是「歌单同步 / 下载 / 解密 / 标签管理」。
- Android 只做了接口预留（见 ARCHITECTURE「Android 复用边界」），没有任何实现。
- 单测只覆盖 Core；Desktop 的 ViewModel/窗口**没有自动化测试**，历史验证都是临时 UIA 脚本，未入库。
- 文档引用了两个仓库里不存在的文件：`docs/IMPLEMENTATION_STATUS.md` 提到 `docs/screenshots/main-window.png`，`THIRD_PARTY_NOTICES.md` 提到开发期参考 `.research/netease-integration-research.md`（`.research/` 已 gitignore）。补图或改文档二者取一。
- 网易云 / QQ / 酷狗接口随时可能变（见 §10）；匹配成功率依赖第三方可用性，网易云 406 限流时以 QQ 为主。
- 单账号：没有账号切换/多账号；`session.dat` 绑定 Windows 用户，换用户或换机器必须重新扫码（启动时读取失败会在日志里给出原因与数据目录）；账号 Cookie 的续期依赖非公开接口，长期有效性尚待真机观察。
- 本地发布产物不带版本号（1.0.0.0），只有 CI 包带 `-p:Version`。
- 逐字歌词取决于源是否提供 YRC/QRC/KRC；都没有时回退逐行 LRC。
- 无损/ Hi-Res 以上档位受会员权益限制：无权限时降档或失败，程序不绕过。
- FLAC 封面走 VorbisComment `METADATA_BLOCK_PICTURE`（TagLibSharp 2.3.0 无原生 PICTURE block API），主流播放器兼容。

## 15. 交接清单（转交前逐项确认）

- [ ] 仓库所有权 / 协作者权限、GitHub Actions 与 Releases 权限已移交（CI 只用默认 `GITHUB_TOKEN`，无额外 secret）。
- [ ] 新维护者的 `git config user.name/user.email` 已设置；`LICENSE`、README 徽章与仓库链接是否要改。
- [ ] 删除个人数据：`publish\win-x64\data`（含 `session.dat`）、`%LOCALAPPDATA%\LunaticPrincess`、`%MyMusic%\LunaticPrincess` 里的下载内容；检查 `settings.json` 里有没有个人路径。
- [ ] `git status` 干净，仓库里没有临时脚本/日志/发布产物；`git log` 与远端一致。
- [ ] 在干净环境重新跑 `.\build.ps1 -Test`（404/404）与 `.\build.ps1 -Publish`，并在一台**没装 .NET** 的机器上验证 CI 风格自包含 zip 能直接运行。
- [ ] 与接手人当面过一遍 §2 自检、§10 外部接口风险、§12 发版流程，并确认谁负责后续 tag/Release。
- [ ] 约定后续节奏：分支策略（是否引入 develop/PR 流程）、发版频率、以及是否继续推进 Android 与本地播放。
