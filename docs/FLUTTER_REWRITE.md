# Flutter 重写记录

> **状态：进行中（分支 `GUI-Flutter`）。** `main` 仍是 Tauri 版（2.3.x），Flutter 版功能追平之前不合并。
> 本文件记录这次重写的决定、逐步的实现与验证，以及重写期间要注意的事；做法沿用 2.0 重写时的 `docs/TAURI_REWRITE.md`。

## 已定的事

- 界面改用 Flutter（开始时的最新稳定版是 3.50，2026-11-11），经 flutter_rust_bridge 调 `lp-core`。
- 界面、交互、动画与主题**从零设计**：不移植 Tauri 版的液态玻璃（WebGL2 渲染器、对话框的 SVG 位移滤镜）、窗口材质（亚克力 / 云母）与 CSS 动效体系。
- `lp-core` 原样保留；Tauri 外壳（`src-tauri/src/lib.rs`）里命令的编排逻辑改写成 Flutter 的桥接层。
- 前端里带业务规则的纯逻辑先搬进 `lp-core`（第 1 步，下面），Flutter 那边只剩画面。
- 数据目录按「换 Flutter 运行时目录名」的办法解决：exe 旁的 `data\` 继续是 `lp-core` 的用户数据目录（便携模式的判断也不动），Flutter 的 `flutter_assets`、`icudtl.dat`、`app.so` 改放 `runtime\`（详见第 2 步）。
- **设计语言：Windows 11 观感**（用户看过原型后定的：先由我们出一版，再按反馈改）。控件与配色用 `fluent_ui`，标题栏与侧边栏自己画：`fluent_ui` 的 `TitleBar` 必须嵌在 `NavigationView` 里才能用（否则 `NavigationView.dataOf` 的 `!` 直接抛异常、release 下整窗白屏），而 `NavigationView` 的展开 / 紧凑切换也不适合我们要的分组标题与页脚项。
- **Tauri 版的外观设置键全部丢弃，不迁移**：`liquidGlass`、`glassOpacity`、`backdropHaze`、`background`、`gradientPreset`、`backgroundImage`、`renderFrameRate` 在 Flutter 里没有对应物。它们继续原样留在 `settings.json` 里（不读、不写、不删），以免破坏 Tauri 版与将来的合并；Flutter 设置页只保留 `theme` 与 `windowMaterial` 这两个真有效果的键。
- 文件对话框用 `file_selector`（Flutter 官方插件，走 `IFileOpenDialog`，比 file_picker 的依赖面小）；安装包、便携版与「检查更新」沿用 Tauri 版的做法（GitHub Release）。

## 还没定的事

- 安装包与便携版的具体做法（Flutter 产物是一整个文件夹，不再是单个 exe）、CI 怎么接。
- 版本号（建议 3.0.0）。

## 第 1 步：把前端里的业务规则搬进 lp-core

Tauri 版前端（`app/src/state/`、`app/src/api/`）里有一批决定「写进文件的是什么」「确认框里的数字」的规则。从零设计界面也不能丢掉它们，所以先搬进 `lp-core` 并补单测；其中几项原来在前端和 Rust 里各写了一份，现在只剩 Rust 这份。

| 原来的前端函数 | 现在的 Rust | 管的是什么 |
|---|---|---|
| `splitArtists`、`parseOptionalUInt`、编辑器与批量面板的封面状态 | `tags::form`：`split_artists`、`parse_number`、`trimmed_or_none`、`CoverChange`、`FormError` | 多值字段按 `/ ; ； 、` 拆分；年份 / 音轨号 / 碟号只认 ASCII 数字，错误文案「年份必须是数字。」 |
| `fieldsFromInfo`、`multiValuePlan`、`artistListOf`、`save` 里拼请求、`dirty`、`applyEntry`、`applyOnlineTags`、`onlineTagsQuery` | `tags::form::editor` | 编辑器存档写什么：没改过的多值字段进 `keep`（`AC/DC` 不被拆开）、套用在线结果后用来源给的歌手清单；在线结果怎么填进表单；「在线标签」的关键词 |
| `pathKey`、`rowValue`、`commonValues`、`setField`、`fields`、`buildPatch` | `tags::form::batch` | 批量修改的「〈保持〉」、只写动过的字段、空白表示清除 |
| `planTransfer` | `download::plan`：`plan_transfer`、`unique_rows` | 「下载缺失」「一键同步」确认框的计数；去重规则与真正的下载任务（`download::task`）共用同一个函数 |
| `keywordFromFileName`（与 Rust 的 `FileContext::keyword_from_file_name` 重复） | `online::keyword_from_file_name` | 从文件名推搜索关键词 |
| `pickFallbackLyrics`、`fallbackFormatLabel`（与 Rust 的 `pick_lyrics` 重复） | `FallbackLyricsView::pick_lyrics` / `format_label` | 切换「逐字优先」时不联网、从已取回的几种写法里重新挑 |

**不搬**的是纯界面行为与文案，新界面会按新设计重写：Shift 连选（`applyRange`）、日志按 id 合并（`mergeLogs`）、下载进度每帧合并（`applyProgress`）、状态栏与结果行的各种中文文字（`fileInfoText`、`describeEntry`、`summaryText` …）、隐藏字段的本地记忆（`editorDisplay`）。

**重写期间要同步**：Tauri 版前端没有改接这些 Rust 函数，`main` 上仍用前端那份。重写期间 `main` 上如果修改了上表左栏的任何一个函数，要同步改 `GUI-Flutter` 上的 Rust 版本（反过来也一样），否则合并时两边的行为会不一致。

### 与前端的差异（有意为之）

- `download::plan::plan_transfer` 返回需要下载的歌在歌单里的**行号**（`missing_rows`），不是歌曲对象：界面按行号找回自己的歌曲行。
- 批量面板的改动清单是 `HashMap<TagField, String>`（`TagEdits`），共同值与显示用 `Vec`，顺序同面板（`BATCH_FIELDS`）。
- 编辑器「套用在线结果」的两个函数直接返回新的艺术家清单：`apply_search_result` 的结果总是替换界面记住的清单（可能是 `None`）；`apply_online_tags` 在在线歌手为空时原样交回当前的清单。
- 首尾空白按 Rust 的 `str::trim`（Unicode 空白）处理，与 JavaScript 的 `trim` 只在极少数字符上不同（如 U+FEFF）。

### 新增 / 改动的公开项

- `tags::form`（新）：`CoverChange`、`FormError`、`split_artists`、`trimmed_or_none`、`parse_number`；子模块 `editor`、`batch`。
- `download::plan`（新）：`TransferPlan`、`unique_rows`、`plan_transfer`；`download::task` 的去重改为调用 `plan::unique_rows`。
- `online::keyword_from_file_name`（由 `FileContext` 的私有方法改为公开函数）；`FallbackLyricsView::pick_lyrics` / `format_label`（新）。
- `TagField` 增加 `Serialize`；`WriteTagsRequest` 增加 `PartialEq` / `Eq`（单测比较用）。
- 没有新增依赖。

### 验证

- 测试 +61（1177 → 1238，`lp-core` 库内 1103 → 1164）：`tags::form` 共用规则 7、`tags::form::editor` 26、`tags::form::batch` 18、`download::plan` 8、`online` 2（备用词源换格式重新挑、文件名推关键词）。样例数据都是编的。
- `cargo fmt --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 全部通过；`csharp_*` 黄金夹具不受影响（36 个全过）。
- 纯 Rust 改动，Tauri 版界面没有改接，所以不涉及界面验证；`pnpm build` 不受影响。
- 编辑器与批量面板的规则由子代理在独立的 worktree 里实现，合并前逐行对照前端源码审查过。

## 第 2 步：原型（登录、歌单、歌曲表格）

原型的目标是把 `lp-core` 真的接到 Flutter 里跑一次，顺带定下数据目录的方案（原「还没定的事」里那条）。原型只验证「跑得通」，不做设计。

- 桥接层是新 crate `app/crates/lp-bridge/`（包名 `lp_bridge`，`cdylib` + `staticlib`，依赖 `flutter_rust_bridge = "=2.13.0"` 与 `lp-core`）。`lp-core` 自己不引 Flutter，桥接层只做 DTO 转换、tokio 运行时的进出与事件转发：`app`（启动数据、日志、账号事件、关闭）、`login`（校验登录态、二维码开始 / 轮询、退出登录）、`playlists`（歌单列表、歌单详情）。
- Flutter 工程在 `app/flutter/`，界面用 `fluent_ui` 按 Windows 11 观感自己做（自绘标题栏、自绘侧边栏，见「已定的事」）。
- **数据目录**：Flutter 的 Windows 构建默认把 `flutter_assets`、`icudtl.dat`、`app.so` 放进 exe 旁的 `data\`，而 `paths.rs` 把「exe 旁边有 `data\`」当成便携模式，两者会撞车。做法是给 Flutter 的运行时目录改名 `runtime\`：`windows/CMakeLists.txt` 的 `INSTALL_BUNDLE_DATA_DIR` 指向 `.../runtime`，`windows/runner/main.cpp` 里 `DartProject` 的目录名改成 `L"runtime"`。`lp-core` 一行没改，exe 旁的 `data\` 仍只放用户数据（便携模式下就是设置、登录态与 `data\Music`），将来安装 / 卸载钩子也不会删到别的东西。
- 这两个构建文件是 MSVC 用 `/W4 /WX` 编译的、源文件又不带 UTF-8 BOM，注释只能写 ASCII：中文会按代码页 936 读成 `C4819`，进而被 `/WX` 变成构建失败。

### 验证

- `flutter analyze` 无问题；`flutter test` 4 个（`formatDuration`、`mergeLogs`）全过。
- `flutter build windows --release` 通过；产物目录是 exe、`lp_bridge.dll` 与 `runtime\`（`app.so`、`flutter_assets`、`icudtl.dat`），没有 `data\`。
- 实机：把 Release 目录复制到 `D:\claude\lp-flutter-build`，手建空 `data\` 后启动，窗口按 1280×800 打开；`data\` 里只出现 `Music\`（默认下载目录），说明便携模式生效、Flutter 资源没混进用户数据。
- 外观：按 Windows 11 观感改过一版（标题栏、侧边栏选择态、卡片、设置分组），用 DPI-aware 截图确认过概览页与设置页的排版与文案。**截图脚本的第一句必须是 `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`**：本项目开发机是 125% 缩放，PowerShell 默认 DPI-unaware，`GetWindowRect` 与 `CopyFromScreen` 会拿到虚拟化坐标，截出来右侧与下方被裁掉，看起来像「侧边栏只剩图标、页脚项消失」。
- 还没验证：扫码登录全流程（要真扫码）、Tauri 版有而原型没接的功能。

## 第 3 步：设置落盘与本地曲库对比（M1、M2）

这一步开始按「主流程」推进（登录 → 歌单 → 本地对比 → 下载），先把设置做成真的能存，再把本地曲库扫描与对比接上。

- 桥接层新增两组 API。`settings`：`load_settings` / `save_settings(patch)`，返回 `SettingsDto`（当前设置、数据目录、设置文件、是否便携、默认下载目录、读取警告、音质档位选项）。补丁是 JSON 文本，只含改过的键，由 `AppSettings::apply_patch` 校验后落盘——未知键、类型错、越界一律报文中文错误且整份不生效。`library`：`library_summary` / `library_is_scanning` / `library_cancel` / `library_scan` / `library_match(playlist_id, rescan)` 与进度流 `watch_library_progress`。
- 扫描 / 对比的错误要分种类（`directoryMissing` / `cancelled` / `playlistNotLoaded` / `failed`），但 frb 的 `Result` 错误位只能放字符串，所以结果载荷是 `{ summary | error }` / `{ outcome | error }` 的联合体，界面按 `kind` 分支。
- 对比只认「已经从界面加载过」的歌单：`playlist_tracks` 成功后会调用 `LibraryManager::remember_playlist`，否则 `match_playlist` 报「歌单还没有加载完成，请稍后再对比本地曲库。」。
- Flutter 端：`AppController` 增加设置与对比状态（`settings`、`settingsSaving`、`match`、`matchError`、`scanProgress`、`libraryBusy`、`notice`），设置项改一个存一个；歌单页加「对比本地曲库 / 重新扫描 / 刷新」和扫描进度条，歌曲表格加本地状态列（绿=本地已有、橙=NCM 待解密、空心=缺失）与 Ctrl / Shift 连选，表头显示「已选 N 首」。
- 设置页只做四组：外观（材质、主题）、下载（音质、并发、文件名模板、写封面、跳过 / 覆盖）、本地曲库（自动对比、匹配阈值）、目录（下载 / 曲库 / 数据 / 设置文件，可复制）。选目录要等文件对话框（M8）接上，先只显示路径。

### 验证

- 测试 1238 → 1241：`lp-bridge` 新增 3 个（枚举字段取自 `lp-core` 的 serde 名、普通字段原样带过、音质选项覆盖每一档且不重复）。
- `cargo fmt`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`flutter analyze`、`flutter test`、`flutter build windows --release` 全过。
- 实机（`D:\claude\lp-flutter-build`）：设置页改动立即写进 `data\settings.json`；重启后主题与材质保持；歌单页能扫描目录并显示进度，对比后标题行出现「本地 N · NCM N · 缺失 N」。
- 这一轮没有真的登录（要扫码），所以对比用的是「已加载歌单」的路径；下载相关（M3）还没接。

## 下一步

1. M3：下载与同步（音质设置已就位）——`download_tracks`、`download_missing`、`sync_playlist`、`download_cancel` 与下载进度流，以及「下载缺失」「一键同步」两个确认框。
2. 之后依次：M4 标签查看与编辑、M5 批量标签与在线标签、M6 歌词（预览 / 编辑 / 逐字）、M7 NCM 导入与批量整理、M8 打包收尾（文件对话框、拖放、安装包 / CI、3.0.0、大歌单压测）。