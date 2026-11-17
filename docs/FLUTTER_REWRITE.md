# Flutter 重写记录

> **状态：进行中（分支 `GUI-Flutter`）。** `main` 仍是 Tauri 版（2.3.x），Flutter 版功能追平之前不合并。
> 本文件记录这次重写的决定、逐步的实现与验证，以及重写期间要注意的事；做法沿用 2.0 重写时的 `docs/TAURI_REWRITE.md`。

## 已定的事

- 界面改用 Flutter（开始时的最新稳定版是 3.50，2026-11-11），经 flutter_rust_bridge 调 `lp-core`。
- 界面、交互、动画与主题**从零设计**：不移植 Tauri 版的液态玻璃（WebGL2 渲染器、对话框的 SVG 位移滤镜）、窗口材质（亚克力 / 云母）与 CSS 动效体系。
- `lp-core` 原样保留；Tauri 外壳（`src-tauri/src/lib.rs`）里命令的编排逻辑改写成 Flutter 的桥接层。
- 前端里带业务规则的纯逻辑先搬进 `lp-core`（第 1 步，下面），Flutter 那边只剩画面。

## 还没定的事

- 设计语言：Flutter 内置的 Material 3、`fluent_ui`（贴近 Windows 11），或用户另给设计稿（`AGENTS.md` §11：界面以用户给的设计参考为准）。
- 外观相关的设置键怎么处理：`windowMaterial`、`glassOpacity`、`backdropHaze`、`background`、`gradientPreset`、`backgroundImage`、`liquidGlass`，以及本来就没用的 `renderFrameRate`；`theme`、`reduceMotion` 看新设计要不要。
- 数据目录：Flutter 的 Windows 构建在 exe 旁边放 `data\`（`flutter_assets`、`icudtl.dat`、`app.so`），而 `paths.rs` 把「exe 旁边有 `data\`」当成便携模式、设置 / 登录态 / 默认下载目录 `data\Music` 都在那里。要么换目录名并做迁移，要么让两者共存并重写卸载逻辑（绝不能删到 `data\Music`）。
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

## 下一步

1. **原型**：flutter_rust_bridge 调 `lp-core` 完成登录（扫码）与读歌单、画一张歌曲表格，并实际打包一次，定下 `data\` 目录的方案。
2. 定设计语言，然后逐个画面实现：先主流程（登录 → 歌单 → 本地对比 → 下载），再 NCM 导入、编辑器、批量修改标签、歌词转逐字、设置。
