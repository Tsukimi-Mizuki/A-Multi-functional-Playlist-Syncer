# 实现状态

> 最后更新：2026-11-07。当前状态：**2.2.0（Rust + Tauri 2 + React 重写版）**：重写模块 1–12 全部完成，
> `cargo test --workspace` **1089 个测试全部通过**，`cargo fmt --check`、`clippy --all-targets -D warnings`、`pnpm build` 通过。
> **已发布**：tag `v2.0`（2.0.0）、`v2.1`（2.1.0，运行日志动画与歌词预览来源修复）、`v2.1.1`（2.1.1，检查更新）、`v2.1.2`（2.1.2，背景雾度、精简设置说明）、`v2.1.3`（2.1.3，下载缺失 / 一键同步二次确认）与 `v2.2`（2.2.0，液态玻璃主题重做），都由 GitHub Actions 自动打包并建 Release（安装包 + 便携版 zip + 校验值）。
>
> 旧的 C# / Avalonia 版（v1.x，含 v1.3，404/404 单测）在 `CSharp-GUI` 分支，只作历史与维护用途。
> 逐模块的实现细节、与 C# 版的有意差异和验证过程见 `docs/TAURI_REWRITE.md`。

## 模块

| 模块 | 状态 | 说明 |
|---|---|---|
| 工程骨架与构建 | ✅ 完成 | Cargo 工作区（`lp-core` + `src-tauri`）、React + TS 前端、`build.ps1`（默认构建 / `-Test` / `-Bundle`）、`ci.yml` + `main.yml`、`scripts/resolve-version.ps1`；Rust 1.99 / edition 2024，clippy `-D warnings` 干净 |
| 基础层 | ✅ 完成 | 数据目录解析（环境变量 → 便携 `data\` → `%LOCALAPPDATA%` → …，含写入探测与旧目录迁移）、`settings.json`（27 个键，与 C# 版互通）、DPAPI `session.dat`（与 C# 版互读） |
| 扫码登录与 Cookie 持久化 | ✅ 完成 | type=3 主路 + type=1 降级；803 Cookie 合并；DPAPI 加密落盘；启动恢复；后续响应 Set-Cookie 合并并自动落盘；eapi `login/token/refresh` 启动及每约 12 小时续期（实测换发新 `MUSIC_U` / `__csrf` / `MUSIC_R_U`，2026-11-04 修复了原先 weapi 端点的 `code=400`）；`CookieMerger` 按 `Path` 处理同名多条 Set-Cookie；仅接口 `code==301` 才清除会话。180 天后的实际掉线情况仍待长期观察 |
| 歌单读取与曲目补齐 | ✅ 完成 | 歌单分页、大歌单 trackIds 按 500 / 批补齐保持顺序、某批重试一次、部分缺失横幅；固定行高虚拟滚动（3000 首只渲染约 24 行） |
| 歌词解析与预览 | ✅ 完成 | LRC / 增强 LRC / YRC / QQ QRC（3DES 变体）/ 酷狗 KRC；译文对齐；逐行 / 逐字序列化；网易云歌词三级兜底；歌词预览对话框（来源跟随设置里的「歌词来源」，网易云 / QQ / 备用词源依次尝试） |
| 标签读写 | ✅ 完成 | MP3 / FLAC / M4A / OGG / OPUS / WAV（lofty）；封面、歌词、网易云 ID；原子写入；标签查看对话框 |
| NCM 解密与导入 | ✅ 完成 | 流式解密（音频不整文件入内存）、元数据 / 封面 / 网易云 ID 回写、导入对话框（文件夹记忆、取消保留已完成结果、解密后自动在线补全） |
| 本地曲库与对比 | ✅ 完成 | 并行扫描、网易云 ID 精确匹配 + 标题 / 歌手 / 时长评分、一对一、「已存在 / NCM 待解密 / 需下载」状态列与统计 |
| 音质选择下载与一键同步 | ✅ 完成（真实下载待用户真机验证） | 九档：标准 / 极高 (HQ) / 无损 (SQ) / 高解析度无损 (Hi-Res) / 高清臻音 / 沉浸环绕声 / 超清母带 / 臻音全景声 (Audio Vivid) / 杜比全景声 (Dolby Atmos)；逐级降档并提示实际音质；取链带桌面客户端 Cookie；试听片段拒收；NCM 自动解密；同步三分支；多选 + 工具栏 + 逐行进度；并发任务池；取消保留已完成结果；「下载缺失」「一键同步」二次确认（缺失清单可取消勾选） |
| 在线匹配（歌词 / 标签 / 封面） | ✅ 完成 | 网易云（匿名 eapi 搜索）/ QQ 主源 + 酷狗 / LRCLIB 备用词源；多变体检索与打分（阈值 0.30–1.00，默认 0.60）；歌词与标签编辑器、批量获取窗口、拖放、`--edit` / `--lyrics` 启动参数 |
| 设置与外观 | ✅ 完成 | 设置面板（外观 / 下载 / 本地曲库 / 在线匹配 / 关于）、深色 / 浅色 / 跟随系统、背景（渐变光斑 / 自选图片 / 歌单封面 / 系统材质）、窗口材质（亚克力 / 云母 / 无）、玻璃浓度、背景雾度（对话框下层界面的虚化与遮暗，0–100%）、减少动效、液态玻璃高级设置（参考 liquid-glass-studio 的折射 / 色散 / 菲涅尔 / 高光 / 模糊 / 色调 / 阴影 / 形状参数 + 即时预览框 + 重置为默认）、实时预览与取消还原 |
| 界面（液态玻璃） | ✅ 完成 | 透明无边框窗口 + 自绘标题栏；内置背景下 WebGL2 画背景并把主面板画成真正折射的液态玻璃；对话框边缘用 SVG 位移滤镜折射下层界面；按钮、对话框、卡片与系统材质下的面板用同一组参数的 CSS 近似（按角度的亮环、菲涅尔、色散彩边、超椭圆圆角）；景深虚化；统一动效体系；所有可交互控件有中文 `aria-label` |
| 错误提示与运行日志 | ✅ 完成 | `error_text` 识别权限不足 / 空间不足 / 文件被占用 / 路径过长并给出处理建议；运行日志 400 条（内存），可收起 |
| Windows 打包与发布 | ✅ 完成（见验证记录里的环境说明） | `build.ps1 -Bundle` 生成便携版 zip（exe + 空的 `data\`）+ NSIS 安装包 + `SHA256SUMS.txt` 到 `dist-release\`；安装时可选目录，安装钩子在安装目录建 `data\`，设置 / 登录态 / 缓存 / 默认下载目录都跟着安装目录走；`main.yml` 在 `v*` tag 上自动检查、打包并建 Release |
| 其他平台 | ➖ 不做 | `lp-core` 与外壳分离，理论上可复用，目前没有任何实现 |

## 验证记录

### 自动化测试

`cargo test --workspace`（离线，不依赖真实音频）：**1089 个，全部通过，0 ignored**。

| 范围 | 数量 | 说明 |
|---|---|---|
| `lp-core` 库内 | 1015 | `netease` 176（weapi / eapi 向量、Cookie 合并、登录 / 续期 / 扫码、歌单、歌曲、取链、歌词、搜索）；`lyrics` 192（解析、QRC、KRC、zlib、打分、批量、歌词服务与按来源预览）；`download` 124（单曲下载、媒体辅助、并发池、任务编排含确认后的下载缺失、NCM 导入）；`library` 66；`tags` 65；`ncm` 49；`online` 88（QQ / 酷狗 / LRCLIB / 编排）；`auth` 30；`session` 28；`settings` 39（含液态玻璃参数表 9）；`sync` 27；`utilities` 33（错误文案、文件名、音频类型、取消）；`paths` 20；`editor` 21；`update` 17（tag 归一与 SemVer 比较、忽略版本、Release 解析与说明整理、发布页白名单、限流 / 404 / 网络错误、自动 / 手动检查与日志）；`playlist` 15；`models` 13；`error` 5；`logs` 3；`qr` 3；其他 1 |
| `lp-core` 集成测试 | 36 | `csharp_compat` 5、`csharp_download` 2、`csharp_library` 10、`csharp_lyrics` 4、`csharp_ncm` 4、`csharp_online` 7、`csharp_tags` 4——与 C# 版生成的「黄金」夹具逐字节 / 逐字段对照 |
| `src-tauri` 外壳 | 38 | 请求结构、设置只写补丁并保留其他字段、设置不可信时不写盘（含忽略的更新版本）、任务槽、事件名等 |

- `cargo fmt --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`pnpm build`（tsc + vite）通过，`.\build.ps1 -Test` 退出码 0。
- 前端没有自动化测试：逻辑集中在 `lp-core`，界面靠浏览器假后端 + 实机验证。

### 与 C# 版的交叉验证（重写期间完成）

用临时 C# 探针（直接引用 C# 版 `LunaticPrincess.Core`，已删除）在同样输入上跑出结果并固化为夹具，Rust 版必须一致：

- 设置 / 登录态文件互读（C# 写 → Rust 读、Rust 写 → C# 读）。
- 歌词：92 个解析用例、22 个 KRC、31 个解密、5 组 3DES 随机向量逐字节一致；10 首真实歌曲实网对比一致。
- 标签：六种容器的写 / 读、局部更新、`replace_all` 清空，双向互读一致。
- NCM：32 个合成样本的解密输出长度与 SHA-256 完全一致，17 种损坏样本的错误文案逐字相同。
- 曲库：385 首随机歌单 + 79 个文件的合成曲库，匹配结果完全一致，分数最大差 1.1e-16；全部 65536 个 BMP 码元的字符类别对比（仅 .NET 8 之后才分配的字符有已知差异）。
- 下载 / 同步：12 个场景完全一致。
- 在线匹配：打分 1156 组、批量选择 30 个场景、词源解析 31 个场景全部一致。

### 实机验证

- 重写期间每个模块都用「exe 复制到独立目录 + 独立数据目录」的方式实机跑过（WebView2 远程调试端口驱动界面 / 调命令，Windows UI 自动化读控件，截图确认）：登录（含**真实 803 扫码确认流程**、重启后 DPAPI 恢复、退出登录）、歌单（真实账号 16 个歌单、1235 首大歌单分批补齐）、标签、NCM 导入、本地曲库、编辑器与批量、设置与材质切换。细节见 `docs/TAURI_REWRITE.md` 各模块「验证」。
- **模块 12（本次）**：
  - `.\build.ps1 -Test` 全过（上面的 1014，网易云搜索修复后为 1027，登录续期修复后为 1038）；`.\build.ps1` 默认构建成功，`app\target\release\LunaticPrincess.exe` 约 6.8 MB。
  - `.\build.ps1 -Bundle -Version v2.0.0-manual.1`：版本号写入 `Cargo.toml` / `tauri.conf.json` / `package.json` 并参与编译（构建输出为 `lp-core v2.0.0-manual.1`），**便携版 zip 生成成功**（`LunaticPrincess-2.0.0-manual.1-win-x64-portable.zip`，3.0 MB，里面只有 `LunaticPrincess.exe`，没有 `data`）。解压到 `D:\claude\lp-portable\`、`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data` 启动：窗口标题 `Lunatic Princess`、进程 `Responding`，数据落在指定目录的 `webview\EBWebView`；测完已清空 `D:\claude`。
  - **NSIS 安装包在本次自动化环境里没能生成**：Tauri 打包器把下载的 NSIS 工具解压到 `%LOCALAPPDATA%\tauri\nsis-3.13` 后要把目录改名为 `NSIS`，而这台机器上 Claude 桌面版的子进程对 `%LOCALAPPDATA%` 的写入被 MSIX 文件系统虚拟化到包私有目录（`C:\WpSystem\…\Packages\AnthropicPBC.Claude_…\LocalCache\Local\tauri`），改名时报 `os error 17`（无法将文件移到不同的驱动器）；另有一次下载中途 `unexpected end of file`（网络瞬时故障）。这是运行环境的限制，不是工程配置问题（打包配置已被 Tauri 解析通过，流程走到了「Verifying NSIS package」）。**需要在普通 PowerShell 里执行 `.\build.ps1 -Bundle` 确认安装包能生成**；GitHub Actions 的 windows-latest 上没有这个虚拟化。
  - 版本解析（`scripts/resolve-version.ps1` 与 `main.yml` 里的步骤，本地用 Windows PowerShell 5.1 模拟）：`v2.0.0`→`2.0.0`、`v1.3`→`1.3.0`、`v.1.0.0`→`1.0.0`、`V1.0.0`→`1.0.0`、`v2.0.0-rc.1`→`2.0.0-rc.1`、`v0.0.1-manual`→`0.0.1-manual`、`v1`→`1.0.0`；分支手动运行 → `2.0.0-manual.<运行序号>`；`v1.2.3.4`、`vfoo` 等非 SemVer 直接失败。两个 workflow 用 PyYAML 解析通过（结构、触发条件、权限、步骤顺序已核对）；**workflow 本身没有真正在 GitHub 上跑过**，首次推送后要看一次 Actions。
- **发布 v2.0（2026-11-04）**：
  - 上面「NSIS 安装包没能生成」已解决：`tauri.conf.json` 加 `bundle.useLocalToolsDir = true`，NSIS 工具缓存在 `app\target\.tauri\`，不再碰被虚拟化的 `%LOCALAPPDATA%`；本机（Claude 桌面版子进程里）`.\build.ps1 -Bundle` 成功生成安装包（2.3 MB）、便携版 zip、`SHA256SUMS.txt`。
  - 便携版 zip 的条目是 `LunaticPrincess.exe` 与空目录 `data/`（Windows PowerShell 5.1 的 `CreateFromDirectory` 会写成反斜杠 `data\`，不合 zip 规范，所以改为逐条写入）。解压到 `D:\claude\lp-zip\`、不设 `LUNATIC_PRINCESS_DATA_DIR` 启动：`data\Music`、`data\webview` 建在解压目录里。
  - 安装包：`/S /NS /D=D:\claude\lp-install` 静默安装（不建快捷方式）→ 目录里是 `LunaticPrincess.exe`、`uninstall.exe`、`data\`；启动后 `data\Music`、`data\webview` 建在安装目录里。卸载（界面，**不**勾「删除应用程序数据」）→ exe 与卸载程序删除，`data\` 原样保留；覆盖重装 → 原有的 `session.dat` 等仍在；卸载（勾选）→ `settings.json`、`session.dat`、`webview\`、`logs\` 删除，`data\Music\` 里的测试文件保留（`data\` 因不为空而保留）。测试用的设置 / 登录态都是假文件；测完 `D:\claude` 已清空，卸载注册表项、快捷方式无残留。
- **运行日志动画与歌词预览来源（2026-11-05）**：
  - 运行日志收起 / 展开原先没有过渡（收起时列表被卸载、面板高度从 168 px 直接跳到 `auto`）。改为列表常驻、面板在 168 px 与 33 px 之间过渡并淡出 / 淡入，收起时列表 `inert`。浏览器假后端与发布版 exe（WebView2 远程调试逐帧读高度）都确认两个方向连贯。
  - 主界面双击的歌词预览原先只调网易云（`LyricsService::get`），设置里的「歌词来源」不起作用。改为 `LyricsService::preview` 按来源次序依次尝试（网易云按 ID、QQ 按检索打分、再到备用词源），每次调用时读设置。发布版 exe（独立数据目录，匿名）实网测《晴天》：四种「歌词来源」依次保存后，预览来源分别是网易云音乐 / QQ 音乐 / QQ 音乐 / 网易云音乐，QQ 逐字形式拿到逐字歌词、没有译文时不标「含翻译」（只读了来源与统计，没有输出歌词正文）；测完已删除 `D:\claude\lp-run`、`D:\claude\lp-data`。
- **检查更新（2026-11-06）**：
  - 浏览器假后端（`?mock=update` / `updatefail`，深色与浅色）：启动 3 秒后弹窗；忽略后「立即检查」照样弹出并注明忽略过；弹窗叠在设置面板上，Esc 只关弹窗；跳转、失败文案都正常。
  - 发布版 exe 临时把版本号改成 2.0.0 编译（编译后清单已改回 2.1.0），复制到 `D:\claude\lp-run`、独立数据目录，实网请求 GitHub：启动后弹出 2.1.0（发布日期 2026-11-05，说明就是 v2.1 的 tag 正文，Full Changelog 行已去掉）→「忽略本次更新」后 `settings.json` 写入 `ignoredUpdateVersion: "2.1.0"`，重启不再弹窗、日志记「已被忽略」→ 设置里「立即检查」照样弹窗并注明忽略过 →「跳转至下载页面」在默认浏览器（Edge）打开 v2.1 发布页、弹窗关闭 → 关掉「自动检查更新」并保存，重启后没有任何检查日志。测完已删除 `D:\claude\lp-run`、`D:\claude\lp-data`。
- **背景雾度（2026-11-06）**：
  - 浏览器假后端（深色 / 浅色）：设置里拖动「背景雾度」时身后的主界面实时变化，0% 时 `#root` 不虚化、遮罩透明，100% 时虚化 30 px、遮罩 92%；默认 30% 算出 9 px 与 0.26，与原来的固定效果（9 px、0.255）基本一致；示意图的「下一层」同步虚化。
  - 发布版 exe（独立数据目录 `D:\claude\lp-data`）：WebView2 里 0% / 100% 的计算值与截图都符合预期（0% 能透过设置面板看清主界面，100% 主界面完全看不见）；保存 70% 后 `settings.json` 写入 `backdropHaze: 0.7`，改成 10% 再点取消会还原到 70%；重启后打开「导入 NCM」，主界面虚化 21 px、遮罩 0.63。测完已删除 `D:\claude\lp-run`、`D:\claude\lp-data`。

- **下载缺失 / 一键同步的二次确认（2026-11-07）**：
  - 浏览器假后端（深色 / 浅色）：默认场景「下载缺失」列出 1200 首（与工具条「需下载 1200」一致）、默认全选；点行取消一首、Shift 再点连取一段，表头复选框随之变成半选，全选 / 全不选后按钮在「下载 1200 首」与不可点的「下载 0 首」之间切换；排除 2 首后确认，任务以 1198 首开始。「一键同步」显示跳过 1500 / NCM 解密 300 / 需要下载 1200（合计 3000），开了「覆盖已存在文件」时第一行改为「重新下载」；确认后开始同步，Esc / 取消不启动任务。`libmanual`（未对比）点按钮先扫描、表格状态列填上后才弹窗，扫描中途取消则不弹窗、按钮恢复可用；`libmissing` 两个确认框都按 3000 首缺失并显示曲库目录不存在的提示。`planTransfer` 对重复歌曲、NCM、全部已存在、曲库目录不存在四种输入的结果正确。控制台无错误。
  - 发布版 exe（`D:\claude\lp-run` + 独立数据目录 `D:\claude\lp-data`，未登录）：窗口正常打开，深色 / 浅色都不崩；经 WebView2 远程调试调用 `download_missing`：`trackIds: []` 返回 `noSongs`「没有选择要下载的歌曲，请至少勾选一首。」，带 ID 与不带 `trackIds` 都正常走到「歌单还没有加载完成」，说明新参数已接通。真实账号下两个确认框的实际下载 / 同步需要用户真机确认。测完已删除 `D:\claude\lp-run`、`D:\claude\lp-data`。
- **液态玻璃主题（2026-11-07）**：
  - 参考：用户指定的 liquid-glass-studio（GitHub 仓库与在线演示）。浅克隆到临时目录只读源码（不安装、不运行），确认 MIT 许可证、各参数含义与 uniform 换算；在线演示核对效果。
  - 平台探测（发布版 exe，WebView2 156 / D3D11 ANGLE）：WebGL2、浮点渲染目标、CSS `corner-shape: superellipse()` 都可用；`backdrop-filter` 对页面内容已经生效（与早期结论不同）；SVG `feDisplacementMap` 套在 `#root` 上能正常位移。网易云图床对跨域请求返回 `Access-Control-Allow-Origin: *`，歌单封面可以直接当 WebGL 纹理。
  - 浏览器假后端：默认内置背景时主界面四块面板由 WebGL 画成玻璃、控制台无错误；设置里拖动滑块时 `--lg-*` 变量、侧栏的 `border-radius` 与 `corner-shape` 立即变化，预览框同步重画（圆角 0 / 20、折射厚度 40、色散 30 等极端值逐一看过）；切到「系统材质」时 WebGL 画布卸载、出现窗口材质选项，切回再挂上；「自选图片」显示路径与选择按钮；「重置为默认」后按钮变灰；保存后 `get_settings` 里的背景、配色、图片路径与玻璃参数都是新值；取消会还原圆角与背景。预览框背景是渐变时叠细斜纹，「模糊边缘」关掉后能看到斜纹在玻璃块边缘弯成弧线，据此把「模糊边缘」默认改为关（默认 24 px 模糊下开着时几乎看不出折射）。
  - 发布版 exe（`D:\claude\lp-run` + 独立数据目录 `D:\claude\lp-data`，未登录）：深色 / 浅色的内置背景截图与 2× 局部截图里，面板有超椭圆圆角、菲涅尔亮边、按角度的高光与边缘折射带；打开对话框后 `#root` 套上折射滤镜、`.app-shell` 只做雾度虚化。发现对话框自身的 `backdrop-filter`（现在会生效）把折射完全抹掉——去掉它、并把雾度模糊移到 `.app-shell`、折射移到 `#root` 后，雾度 0 时对话框左边与下边的日志文字被明显折射并带彩边。浅色主题 + 系统材质下普通按钮几乎看不见（白亮环落在白玻璃上），改为保留 1 px 边框、亮环的非高光部分在浅色主题用淡深色。测完已删除 `D:\claude\lp-run`、`D:\claude\lp-data`。
  - 未验证：登录后带真实歌单（大量行、滚动）时的帧率与观感、歌单封面背景（需要真实账号）、无 WebGL2 环境的回退（本机没有这种环境，逻辑走 `data-webgl="off"`）。

## 已知限制

1. ~~**`/weapi/login/token/refresh` 返回 `code=400`**~~ **已修复（2026-11-04）**：续期改走 eapi `/eapi/login/token/refresh`，实测换发新的 `MUSIC_U`（约 180 天）/ `__csrf` / `MUSIC_R_U`，旧令牌换发后仍有效；原问题（桌面端扫码会话在 weapi 同名端点返回 `code=400`，C# v1.3 同样受影响）与调查见 `docs/TAURI_REWRITE.md`「待修问题」与「登录续期修复」。仍需长期观察换发的 `MUSIC_U` 到期前是否稳定续上。
2. ~~网易云搜索匿名不可用~~ **已修复（2026-11-04）**：网易云搜索改走匿名 eapi `/eapi/cloudsearch/pc`（不需要登录，海外 IP 下也是明文），原网页接口 `/api/search/get/web`（海外 IP 下返回密文）只作回退；编辑器默认搜索来源仍是 QQ 音乐，是否改回网易云留给用户决定。详见 `docs/TAURI_REWRITE.md`「待修问题」与「网易云搜索修复」。
3. 扫码登录的 803 确认段需要真人手机扫码，自动化只能验证到出码与 801 轮询。
4. 真实账号下的下载 / 同步 / 状态列 / 歌词预览 / 歌曲行「编」按钮、以及从资源管理器真实拖拽，没有在无人值守环境验证，需要用户真机确认。
5. 无损 / Hi-Res 以上档位受账号会员权益限制：无权限时服务端静默降档或失败，程序不绕过（会提示实际音质）。
6. 逐字歌词取决于来源是否提供 YRC / QRC / KRC；都没有时回退逐行 LRC。
7. 不支持 WMA 标签读写（lofty 不支持 ASF）；FLAC 封面是原生 PICTURE 块；Windows 资源管理器只认 RIFF INFO 的 WAV 标签，RIFF INFO 没有总数、歌词、封面。
8. 网易云 / QQ / 酷狗接口均为非公开接口，随时可能变更；酷狗在部分网络下报证书域名不匹配或握手 EOF，异常吞掉后回落下一个词源。
9. 没有本地播放功能；单账号；`session.dat` 绑定 Windows 用户，换用户或换机器需要重新扫码。
10. 没有致命异常落盘日志（`panic = "abort"`，运行日志只在内存）。
11. 管理员身份运行时 Windows 阻断资源管理器向程序拖放（UIPI），界面只能提示。
12. 真正的折射只发生在界面自己画的内容上（内置背景下的主面板、对话框边缘对下层界面）；「系统材质」透出的桌面网页读不到，那时面板与按钮是 CSS 近似。内置背景需要 WebGL2。系统材质的完整效果只在 Windows 11 上；亚克力在拖动 / 缩放窗口时比云母更耗电。
13. 仓库放在带低完整性标签的目录（如 Edge 的下载文件夹）时，直接运行的 exe 会把数据落在程序目录 `data\`，见 `docs/TAURI_REWRITE.md` 的「已知环境问题」。
14. 安装包与便携版没有代码签名（SmartScreen 可能提示未知发布者）；更新只提醒并打开发布页，不会自动下载安装。
15. 前端没有自动化测试。
