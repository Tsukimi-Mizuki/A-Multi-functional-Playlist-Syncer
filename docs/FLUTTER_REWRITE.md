# Flutter 重写记录

> **状态：进行中（分支 `GUI-Flutter`）。** `main` 仍是 Tauri 版（2.3.x），Flutter 版功能追平之前不合并。
> 本文件记录这次重写的决定、逐步的实现与验证，以及重写期间要注意的事；做法沿用 2.0 重写时的 `docs/TAURI_REWRITE.md`。

## 已定的事

- 界面改用 Flutter（开始时的最新稳定版是 3.50，2026-11-11），经 flutter_rust_bridge 调 `lp-core`。
- 界面、交互、动画与主题**从零设计**：不移植 Tauri 版的液态玻璃（WebGL2 渲染器、对话框的 SVG 位移滤镜）、窗口材质（亚克力 / 云母）与 CSS 动效体系。
- `lp-core` 原样保留；Tauri 外壳（`src-tauri/src/lib.rs`）里命令的编排逻辑改写成 Flutter 的桥接层。
- 前端里带业务规则的纯逻辑先搬进 `lp-core`（第 1 步，下面），Flutter 那边只剩画面。
- 数据目录按「换 Flutter 运行时目录名」的办法解决：exe 旁的 `data\` 继续是 `lp-core` 的用户数据目录（便携模式的判断也不动），Flutter 的 `flutter_assets`、`icudtl.dat`、`app.so` 改放 `runtime\`（详见第 2 步）。

## 还没定的事

- 设计语言：Flutter 内置的 Material 3、`fluent_ui`（贴近 Windows 11），或用户另给设计稿（`AGENTS.md` §11：界面以用户给的设计参考为准）。
- 外观相关的设置键怎么处理：`windowMaterial`、`glassOpacity`、`backdropHaze`、`background`、`gradientPreset`、`backgroundImage`、`liquidGlass`，以及本来就没用的 `renderFrameRate`；`theme`、`reduceMotion` 看新设计要不要。
- 安装包与便携版的做法（Flutter 产物是一整个文件夹，不再是单个 exe）、CI。
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
- Flutter 工程在 `app/flutter/`，界面先用 `fluent_ui` 顶着，等设计语言定下来再换。目前已画：概览、歌单详情（歌曲表格）、运行日志、设置（只有窗口材质与主题两项，不持久化），以及扫码登录对话框、自绘标题栏和窗口按钮。
- **数据目录**：Flutter 的 Windows 构建默认把 `flutter_assets`、`icudtl.dat`、`app.so` 放进 exe 旁的 `data\`，而 `paths.rs` 把「exe 旁边有 `data\`」当成便携模式，两者会撞车。做法是给 Flutter 的运行时目录改名 `runtime\`：`windows/CMakeLists.txt` 的 `INSTALL_BUNDLE_DATA_DIR` 指向 `.../runtime`，`windows/runner/main.cpp` 里 `DartProject` 的目录名改成 `L"runtime"`。`lp-core` 一行没改，exe 旁的 `data\` 仍只放用户数据（便携模式下就是设置、登录态与 `data\Music`），将来安装 / 卸载钩子也不会删到别的东西。
- 这两个构建文件是 MSVC 用 `/W4 /WX` 编译的、源文件又不带 UTF-8 BOM，注释只能写 ASCII：中文会按代码页 936 读成 `C4819`，进而被 `/WX` 变成构建失败。

### 验证

- `flutter analyze` 无问题；`flutter test` 4 个（`formatDuration`、`mergeLogs`）全过。
- `flutter build windows --release` 通过；产物目录是 exe、`lp_bridge.dll` 与 `runtime\`（`app.so`、`flutter_assets`、`icudtl.dat`），没有 `data\`。
- 实机：把 Release 目录复制到 `D:\claude\lp-flutter-build`，手建空 `data\` 后启动，窗口按 1280×800 打开；`data\` 里只出现 `Music\`（默认下载目录），说明便携模式生效、Flutter 资源没混进用户数据。
- 还没验证：外观（截图在 `D:\claude\shot-overview.png`，等设计语言定下来再细看）、扫码登录全流程（要真扫码）、Tauri 版有而原型没接的功能。

## 下一步

1. 定设计语言（原型已经能登录、读歌单、画表格并打包运行，见上）。
2. 逐个画面实现：先主流程（登录 → 歌单 → 本地对比 → 下载），再 NCM 导入、编辑器、批量修改标签、歌词转逐字、设置。
