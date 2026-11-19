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

## 第 4 步：下载与同步（M3）

主流程的最后一段：确认框、任务、进度与结果。

- 桥接层新增 `app/crates/lp-bridge/src/api/download.rs`：`transfer_plan(playlist_id)`、`download_tracks(playlist_id, track_ids)`、`download_missing(playlist_id, track_ids?)`、`sync_playlist(playlist_id)`、`download_cancel()`、`transfers_running()`，以及两条流 `watch_transfer_started` / `watch_transfer_progress`。
- `transfer_plan` 与真正的任务共用 `lp_core::download::plan::plan_transfer`（第 1 步搬的规则），所以框里的数量和实际执行的一致；曲库目录不存在时按「本地为空」算全部缺失，并把警告一起交回界面。任务结果同样做成 `{ summary | error }` 的二选一载荷：`TransferErrorDto.kind` 是 `busy` / `noSongs` / `playlistNotLoaded` / `cancelled` / `failed`，用户取消不算错误、只提示一句。任务结束若 `library_refreshed` 为真，界面自动重新对比；任务末尾「重新扫描曲库」的进度复用曲库那条流。
- 组合根 `state.rs` 按 Tauri 外壳的顺序装配下载链路：批量歌词服务 → 在线补全器 → NCM 解密 → 下载 → 同步 → `TransferManager`。下载用独立的传输层 `ReqwestTransport::for_downloads(15s 连接 / 60s 读间隔 / 30min 总时长)`，不套 20 秒的接口总超时；备用词源开关、匹配阈值等仍在运行期读设置，保存后立即生效。
- 歌单页工具栏按状态切换：任务进行中显示「取消」、扫描 / 对比中显示「取消」，否则是「下载选中 (N)」「下载缺失」「一键同步」「对比本地」与「更多」（重新扫描曲库 / 刷新歌单）。歌曲表支持 Ctrl 切换、Shift 连选，表头显示「已选 N 首 / 清除」。
- 三个确认框：「下载选中」由界面按 `match.availability` 自己计数（本地已有会跳过、NCM 只能靠一键同步解密），「下载缺失」先向桥接层要计划——去重后没有缺失去只提示「没有需要下载的歌曲…」，否则弹可勾选的缺失清单（全选 / 全不选、Ctrl / Shift 连选），「一键同步」带下载目录、覆盖同名文件的提醒与曲库目录缺失的警告。
- 任务进行中显示阶段（解析中 / 下载中 / 解密中 / 写标签 / 在线匹配）、`已完成 / 总数`、当前歌名与字节进度（百分比优先用 `percent`）；结束后在歌单页顶部出结果条（任务完成 / 有 N 首失败 / 已取消），列出前 5 条失败原因。
- Flutter 侧的传输任务与选择状态都放在 `AppController`，任务的失败与取消统一在 `_runTransfer` 里收口。

### 验证

- Rust 测试仍是 1241 个（桥接层这轮只做 DTO 转换，没有新增单测；下载规则本身的 8 个计划测试在第 1 步）。
- `cargo fmt`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`flutter analyze`、`flutter test`、`flutter build windows --release` 全过。
- 用临时预览入口（`probe_m3.dart`，用完已删）加假数据做了 DPI-aware 截图：歌单页的工具栏四个按钮、「本地 18 · NCM 6 · 缺失 6」、表头「已选 5 首 / 清除」，以及「下载选中的歌曲」确认框（已选中 5 首：本地已有 2 首，2 首是待解密的 NCM，1 首要下载）都渲染正常。
- 还没验证：真实下载 / 解密 / 同步整条链路（要登录态与会员取链）、任务进行中的进度条与结束后的结果条的实机外观（没有真实任务数据）。

## 第 5 步：标签查看与编辑（M4）

单曲的标签窗口与编辑器合并成一个对话框：打开时就把标签读出来，改完原地保存，省掉 Tauri 版「只读窗口 + 另开编辑器」两套界面。

- 桥接层新增 `app/crates/lp-bridge/src/api/tags.rs`:`read_tags(path)`、`tag_editor_open(path)`（读标签并转成表单值）、`tag_editor_save(path, fields, loaded, online, cover)`、`tag_cover_file(path)`（读本地图片做封面，供文件对话框那轮用）。读不出文件时 `view` 与 `error` 同时为空，表示「格式不支持或文件已损坏」；出错时进 `error: { kind, message }`，`kind` 是 `invalid` / `notFound` / `unsupported` / `failed`。
- 写盘规则全部复用第 1 步搬进 `lp_core::tags::form::editor` 的那套：`build_write_request` 校验年份 / 音轨号 / 碟号、`multi_value_plan` 决定没动过的多值字段进 `keep`、封面按 `CoverChange` 映射成 base64 / 移除。保存时先重新读一遍文件再拼请求，避免拿过期的 `TagView` 写盘；写完再读一次，把新值与 `EditorFields` 一起交给界面刷新表单。
- 所有标签读写都走 `tokio::task::spawn_blocking`（lofty 是同步 IO），错误文案沿用 `TagCommandError` 的中文；`run()` 与 `blocking()` 两层 `Result` 要写成 `.await?`（`tag_cover_file` 是 `.await??`）。
- 界面上：歌曲表在本地存在的行尾多一个「编辑标签与歌词」按钮（`availability == 'available'` 且有 `path` 才出现；表头留同样宽的空位保证列对齐），点开是 `tag_editor_dialog.dart` —— 左侧封面与文件信息（格式、时长、码率、大小、路径、网易云 ID），右侧两列表单（标题、艺术家、专辑、专辑艺术家、年份、音轨号、碟号、流派、作曲家、作词家、备注）加多行歌词框。底部「重新读取」「关闭」「保存」；有未保存的修改时关闭会先问一句。封面这轮只能「移除 / 恢复」，换图要等文件对话框（M8）。
- 表单值放在 12 个 `TextEditingController` 里，脏判断是「和打开时读到的 `EditorFields` 不同，或封面标记为移除」；保存失败把服务端文案原样显示在顶部信息条里，成功后显示「已保存：文件名」。

### 验证

- Rust 测试从 1241 变成 1246 个：桥接层新增 5 个（表单值往返、封面三种状态、缺数据的 replace 被拒、`TagView` 的数字转 `i64`、错误类别透传）。
- `cargo fmt`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`flutter analyze`、`flutter test`（6 个，新增 `formatFileSize` 两个用例）、`flutter build windows --release` 全过。
- 还没验证：真的写盘（要挑夹具文件跑一遍）、换封面（等文件对话框）、逐字歌词的编辑（M6 才接）。

## 第 6 步：批量修改标签（M5a）

一次改一批文件的同一组字段：先选目录扫出音频文件，读成一张表，在左边填要改的值，保存时只写选中的、读得出来的那些行。在线获取标签、歌词与封面的整块留给 M5b。

- 桥接层新增 4 个命令（都在 `app/crates/lp-bridge/src/api/tags.rs`）:`batch_scan(folder, recursive)` 返回 `{ folder, recursive, directory_missing, files: [{ path, name, size }] }`（目录为空或不是目录时只把 `directory_missing` 置真，不报错）；`read_tag_rows(paths)` 返回每行的 `TagRowDto`，读不出来的行照旧占一行、带 `readable: false` 与 `error`；`patch_tags(paths, edits, cover)` 把 `[{ field, value }]` 的改动表映射回 `lp_core::tags::form::batch` 的 `TagEdits`，再 `build_patch` + `TagPatch::into_data`，返回 `{ results | error }`（`error.kind` 是 `invalid` / `notFound` / `unsupported` / `failed`）。`batch_scan` 复用 `lp_core::lyrics::batch::BatchLyricsService::scan_audio_files`，不另写目录遍历。
- 「改哪些字段」的规则全部复用第 1 步搬进 `lp_core::tags::form::batch` 的那套：`BATCH_FIELDS` 的 11 个字段、`set_edit`（改回共同值就从改动表里去掉，值不一致的字段任何输入都算改动）、`build_patch`（只带改过的字段，文本 trim 成空即清除，数字按年份 → 音轨号 → 碟号顺序校验，错误文案「年份必须是数字。」）。
- 界面是新文件 `app/flutter/lib/widgets/batch_tags_dialog.dart`（约 1100 行，无状态可复用，全在对话框内部）：上方是目录框 + 「刷新」+「包含子文件夹」；中间左边 304 宽的标签面板（11 个字段，值不同显示「〈保持〉」、改动过显示「〈清空〉」，每项改动后出现「还原」），右边是 14 列的歌曲表（文件名、状态、目录、格式、标题、艺术家、专辑、专辑艺术家、年份、音轨号、碟号、风格、作曲家、作词家，整行横向滚动、表头吸顶）；下方是「已选 X / 共 Y 首」、状态行与「保存修改」「关闭」。
- 读标签按 100 个文件一块、保存按 20 个一块调用桥接层，块之间检查取消标志，所以「取消」只在块边界生效；保存时先把整批的 `TagEdits` 编译成一个 `TagPatch`，每块只换路径列表，同一份补丁写所有行。逐行状态回填「已保存」或失败原因，末尾汇总「已保存 N 首。」/「已保存 S 首，失败 F 首：…」；只有全部成功才清空改动，有失败就保留改动方便再试。
- 选中：单击选中、Ctrl 单击切换、Shift 连选（锚点是上一次点的行），Ctrl+A 全选、↑ / ↓ / PageUp / PageDown / Home / End 移动，双击直接打开单曲编辑器。关窗、换目录、重新扫描、打开编辑器之前若有未保存的改动，先弹「有未保存的修改」确认。
- 入口两处：歌单页「更多」菜单里的「批量修改标签…」（起始目录取设置里的本地音乐目录，没有就用当前歌单第一首本地文件所在目录），以及单曲编辑器底部的「批量修改」（起始目录是当前文件所在目录）。拖放换清单要等 M8 的拖放支持，这一轮只做目录扫描。

### 验证

- Rust 测试从 1246 变成 1251 个：桥接层新增 5 个（批量字段名与 `TagField` 的映射、批量行数字转 `i64`、读不出来的行保留原因、空目录报告缺失、夹具目录能扫出音频文件）。
- `cargo fmt`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`flutter analyze`、`flutter test`（6 个）、`flutter build windows --release`、`build.ps1 -Test` 全过。
- 用临时预览入口（`probe_m5.dart`，用完已删）对着测试夹具目录做了 DPI-aware 截图：标题「批量修改标签」、目录框、左侧字段面板、14 列表格（16 行可见、夹具里 71 个文件）、状态列「无法读取标签：文件不存在、格式不支持或已损坏」、底栏「已 0 / 共 71 首」与「保存修改」都渲染正常。
- 还没验证：真的批量写盘（会改夹具文件，没动）、保存失败/取消的真实分支、拖放换清单。

## 第 7 步：在线标签（M5b）

编辑器里的一键补全：按当前表单的标题与歌手同时搜网易云音乐与 QQ 音乐，挑一条候选，把标签字段与封面填回表单——只填不写，仍然要点「保存」才落盘，歌词不动。

- 桥接层新增 `app/crates/lp-bridge/src/api/online.rs` 三个命令：`online_tag_candidates(query, file_context)`（关键词为空时由 `lp-core` 用文件路径推）、`online_cover_size(song)`、`online_load_tags(song, cover)`。返回同样是「二选一载荷」：成功带数据，失败带 `OnlineErrorDto { kind, message }`，`kind` 沿用 `lp-core` 的 `emptyQuery` / `fallbackSource` / `primarySource` / `noFile` / `unknownSong` / `cancelled` / `failed`。
- 候选与歌曲的 DTO 都把来源写成字符串（`Netease` / `QQMusic`），由桥接层自己映射回 `OnlineMusicSource` 与 `LyricsSourceKind`，界面不需要认识 Rust 枚举；`u32` 的年份 / 音轨号 / 碟号转成 `i64` 给 Dart（Windows 上就是 `int`），反向转换溢出时丢成 `None` 而不是回绕。
- 组合根 `state.rs` 新增 `online: Arc<OnlineManager>`，用与批量歌词服务同一套搜索服务、备用词源与 `settings_provider` 装配，排在 `fallback_sources` 之后——匹配阈值、备用来源顺序等运行时设置改了立即生效。
- 界面是新文件 `app/flutter/lib/widgets/online_tags_dialog.dart`：搜索行（关键词框 + 「搜索」）、状态行、六列候选表（封面、来源与尺寸 / 年份 / 音轨、标题、艺术家、专辑、时长与匹配度），行高 120、选中左侧竖条、双击即确认；底栏是「标签」「封面」两个复选框加一句「歌词不会改动；确定后填进编辑器，保存才写入文件」。
- 封面尺寸单独探测：只对有封面地址的行发 `online_cover_size`，最多 4 个并发，先显示「读取尺寸…」，回来是「宽×高」或「尺寸未知」，没有封面直接「无封面」；重新搜索时旧结果按搜索代数丢弃。
- 编辑器「标签信息」行右端加了「在线标签…」按钮：关键词取当前标题 + 按 `/`、`;`、`；`、`、` 拆开的歌手，标题为空就整个留空交给 `lp-core` 按路径推；`FileContext` 用当前表单的标题 / 艺术家 / 专辑加文件时长与路径。
- 回填只动标题、艺术家、专辑、年份、音轨号、碟号、流派、作曲家；专辑艺术家、作词家、备注、歌词永远不动；在线给的空白字段保持原值、不清空。勾了封面就把封面切成「新封面：来源（保存后写入）」，保存时走 `CoverChange` 的 `replace`。来源给了艺术家名单时记下来，保存时优先用它写多值字段；用户改过艺术家输入框就作废、改回按文字拆分。
- 回填后编辑器顶部出信息条「已载入{来源}的{标签|封面|标签与封面}（歌词没有改动），请确认后保存。」，只勾了标签且没拿到封面时补一句「（没有获取到封面）」；重新读取或保存成功后清掉。
- 批量面板的「在线获取」整块还没做：Tauri 版那里是服务端整批匹配（`batch_run`）并直接写盘，和这里的单曲编辑器取回是两条链路，排进 M7 的批量整理。

### 验证

- Rust 测试从 1251 变成 1255 个：桥接层新增 4 个（歌曲 DTO 往返、未知来源被拒、溢出数字丢成 `None`、错误类别透传）。
- `cargo fmt`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`flutter analyze`、`flutter test`（6 个）、`flutter build windows --release`、`build.ps1 -Test` 全过。
- 用两个临时预览入口（`probe_m5b_editor.dart`、`probe_m5b_online.dart`，用完已删）做了 DPI-aware 截图：编辑器多出「标签信息」与「在线标签…」；在线标签面板真的联网搜到了 20 条候选（网易云音乐 10、QQ 音乐 10），封面尺寸 500×500、匹配度 93% / 97%、QQ 音乐没有封面的行显示「无封面」，底栏复选框与说明都在。
- 还没验证：回填后保存写盘的完整链路、只勾封面 / 只勾标签的分支、真实网络失败时的错误文案（合成点击点不开面板，回填逻辑只做了代码级检查）。

## 第 8 步：歌词预览与歌词转逐字（M6）

两块界面：歌单里查看一首歌的歌词（逐行 / 逐字两种形式），以及把整批文件的逐行歌词换成逐字歌词（先联网检查、确认后才写盘）。歌词编辑器的「在线搜索歌词」面板留下一轮（M6b）。

- 桥接层新增 `app/crates/lp-bridge/src/api/lyrics.rs` 五个命令：`get_song_lyrics(id, mode, title, artist, album, duration_ms)` 返回 `{ data | error }`（`error.kind` 是 `sessionExpired` / `failed`，前者提示重新登录、后者是普通失败）；`word_lyrics_preview(files)`、`word_lyrics_apply(files)` 返回 `{ summary | error }`（`BatchJobErrorDto.kind` 是 `busy` / `noFiles` / `failed`）；`watch_word_lyrics_progress()` 是进度流；`word_lyrics_cancel()` 调批量任务槽的 `cancel()`。
- 两步走原样透传 `lp-core`：`word_preview` 只联网核对、不碰文件，`word_apply` 才写；内嵌歌词写进音频文件，同名 `.lrc` 先备份到所在目录的 `_backup`。已是逐字的、没有歌词的都不动，译文保留——这些规则都在 `lp_core::lyrics::batch` 里，桥接层只做 DTO 转换与进度广播。
- 组合根 `state.rs` 新增 `lyrics: Arc<LyricsService>`（`LyricsService::new(login, logs).with_online(batch_service)`，让预览能回退到 QQ / 酷狗 / LRCLIB）与 `batch: Arc<BatchManager>`（歌词转逐字与 M7 的批量在线获取共用一个任务槽）。`batch_service` 先 clone 给这两位，再 move 进 `BatchEnricher`。
- 状态枚举在桥接层就转成字符串（`WordStatus` → `ready` / `converted` / `alreadyWord` / `noLyrics` / `noMatch` / `noWordLyrics` / `mismatch` / `failed`，`WordTarget` → `embedded` / `sidecar`），界面只按字符串分支，不跟着 Rust 枚举走。
- 歌词预览是新文件 `app/flutter/lib/widgets/lyrics_dialog.dart`：顶部歌名与歌手，工具栏是自绘的「逐行 / 逐字优先」分段控件加一行元信息（来源、逐字 / 逐行、含翻译、行数），正文用 Consolas 只读、可选中。两种形式各自缓存，切回来不重新联网；逐字优先但文件里没有逐字歌词时上方提示「已显示逐行歌词」，空歌词给出「这首歌暂无歌词」与可能的原因，失败时带「重试」。
- 入口是歌曲行尾的「词」按钮，双击整行也打开；表头同样留出宽度保证列对齐。
- 歌词转逐字是新文件 `app/flutter/lib/widgets/word_lyrics_dialog.dart`（目录框 + 「刷新」+「包含子文件夹」+ 三列表格 + 底栏），选择与键盘操作（Ctrl / Shift 连选、Ctrl+A、方向键、PageUp / PageDown、Home / End）沿用 M5a 批量面板那一套。检查完成且确实有歌可以转换时弹一次确认框，确认后立即写盘；检查进度来自进度流，命令返回后以 `summary.results` 为准覆盖每行标记，没处理到的标「已取消」。
- 取消：进行中点「取消」只置取消标志，任务在当前文件处理完后停下，之后的每个块都检查；关窗时若仍在跑也先取消，并作废在途的目录扫描。
- 标题栏新增「歌词转逐字」入口，起始目录取设置里的本地音乐目录，为空就让用户自己选。

### 验证

- Rust 测试从 1255 变成 1260 个：桥接层新增 5 个（歌词形式名往返、预览各标志位原样带过、错误类别稳定、逐字状态与目标名稳定、批量任务错误类别透传）。
- `cargo fmt`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`flutter analyze`、`flutter test`（6 个）、`flutter build windows --release`、`build.ps1 -Test` 全过。
- 用两个临时预览入口（`probe_lyrics.dart`、`probe_words.dart`，用完已删）做了 DPI-aware 截图：歌词预览真的联网拿到了 QQ 音乐的逐行歌词（含翻译、69 行，「逐行 / 逐字优先」分段与元信息都在）；歌词转逐字窗口对夹具目录扫出 71 首，表头、文件名 / 状态 / 所在目录三列与底栏计数正常。
- 还没验证：真的执行转换写盘（会改夹具文件，没动）、逐字歌词的实机显示（没有逐字歌词的本地文件）、任务进度条在真实处理中的样子。

## 下一步

1. M6b：歌词编辑器的在线搜索歌词面板（`online_search`、`online_load_result`、`lyrics_source_fetch`），并把批量面板的「在线获取」一起接上。
2. 之后依次：M7 NCM 导入与批量整理（含批量面板的在线获取 `batch_run`）、M8 打包收尾（文件对话框、拖放、安装包 / CI、3.0.0、大歌单压测）。