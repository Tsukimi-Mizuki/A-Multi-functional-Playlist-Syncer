# 实现状态

> 最后更新：2026-11-04。当前状态：**2.0.0（Rust + Tauri 2 + React 重写版）功能完整**：重写模块 1–12 全部完成，
> `cargo test --workspace` **1014 个测试全部通过**，`cargo fmt --check`、`clippy --all-targets -D warnings`、`pnpm build` 通过。
> **2.0.0 尚未打 tag、未发 Release**（用户决定全部「待修问题」处理完后再发版）。
>
> 旧的 C# / Avalonia 版（v1.x，含 v1.3，404/404 单测）在 `CSharp-GUI` 分支，只作历史与维护用途。
> 逐模块的实现细节、与 C# 版的有意差异和验证过程见 `docs/TAURI_REWRITE.md`。

## 模块

| 模块 | 状态 | 说明 |
|---|---|---|
| 工程骨架与构建 | ✅ 完成 | Cargo 工作区（`lp-core` + `src-tauri`）、React + TS 前端、`build.ps1`（默认构建 / `-Test` / `-Bundle`）、`ci.yml` + `main.yml`、`scripts/resolve-version.ps1`；Rust 1.99 / edition 2024，clippy `-D warnings` 干净 |
| 基础层 | ✅ 完成 | 数据目录解析（环境变量 → 便携 `data\` → `%LOCALAPPDATA%` → …，含写入探测与旧目录迁移）、`settings.json`（24 个键，与 C# 版互通）、DPAPI `session.dat`（与 C# 版互读） |
| 扫码登录与 Cookie 持久化 | ✅ 完成（续期效果待观察） | type=3 主路 + type=1 降级；803 Cookie 合并；DPAPI 加密落盘；启动恢复；后续响应 Set-Cookie 合并并自动落盘；`login/token/refresh` 启动及每约 12 小时续期；仅接口 `code==301` 才清除会话。实测 refresh 返回 `code=400`，见下 |
| 歌单读取与曲目补齐 | ✅ 完成 | 歌单分页、大歌单 trackIds 按 500 / 批补齐保持顺序、某批重试一次、部分缺失横幅；固定行高虚拟滚动（3000 首只渲染约 24 行） |
| 歌词解析与预览 | ✅ 完成 | LRC / 增强 LRC / YRC / QQ QRC（3DES 变体）/ 酷狗 KRC；译文对齐；逐行 / 逐字序列化；网易云歌词三级兜底；歌词预览对话框 |
| 标签读写 | ✅ 完成 | MP3 / FLAC / M4A / OGG / OPUS / WAV（lofty）；封面、歌词、网易云 ID；原子写入；标签查看对话框 |
| NCM 解密与导入 | ✅ 完成 | 流式解密（音频不整文件入内存）、元数据 / 封面 / 网易云 ID 回写、导入对话框（文件夹记忆、取消保留已完成结果、解密后自动在线补全） |
| 本地曲库与对比 | ✅ 完成 | 并行扫描、网易云 ID 精确匹配 + 标题 / 歌手 / 时长评分、一对一、「已存在 / NCM 待解密 / 需下载」状态列与统计 |
| 音质选择下载与一键同步 | ✅ 完成（真实下载待用户真机验证） | 九档：标准 / 极高 (HQ) / 无损 (SQ) / 高解析度无损 (Hi-Res) / 高清臻音 / 沉浸环绕声 / 超清母带 / 臻音全景声 (Audio Vivid) / 杜比全景声 (Dolby Atmos)；逐级降档并提示实际音质；取链带桌面客户端 Cookie；试听片段拒收；NCM 自动解密；同步三分支；多选 + 工具栏 + 逐行进度；并发任务池；取消保留已完成结果 |
| 在线匹配（歌词 / 标签 / 封面） | ✅ 完成（网易云搜索匿名不可用） | 网易云 / QQ 主源 + 酷狗 / LRCLIB 备用词源；多变体检索与打分（阈值 0.30–1.00，默认 0.60）；歌词与标签编辑器、批量获取窗口、拖放、`--edit` / `--lyrics` 启动参数 |
| 设置与外观 | ✅ 完成 | 设置面板（外观 / 下载 / 本地曲库 / 在线匹配 / 关于）、深色 / 浅色 / 跟随系统、窗口材质（亚克力 / 云母 / 无）、玻璃浓度、减少动效、实时预览与取消还原 |
| 界面（液态玻璃） | ✅ 完成 | 透明无边框窗口 + 自绘标题栏；玻璃层叠 + 景深虚化（WebView2 不支持 `backdrop-filter` 的替代方案）；统一动效体系；所有可交互控件有中文 `aria-label` |
| 错误提示与运行日志 | ✅ 完成 | `error_text` 识别权限不足 / 空间不足 / 文件被占用 / 路径过长并给出处理建议；运行日志 400 条（内存），可收起 |
| Windows 打包与发布 | ✅ 完成（见验证记录里的环境说明） | `build.ps1 -Bundle` 生成便携版 zip（只含 exe）+ NSIS 安装包 + `SHA256SUMS.txt` 到 `dist-release\`；`main.yml` 在 `v*` tag 上自动检查、打包并建 Release |
| 其他平台 | ➖ 不做 | `lp-core` 与外壳分离，理论上可复用，目前没有任何实现 |

## 验证记录

### 自动化测试

`cargo test --workspace`（离线，不依赖真实音频）：**1014 个，全部通过，0 ignored**。

| 范围 | 数量 | 说明 |
|---|---|---|
| `lp-core` 库内 | 945 | `netease` 152（weapi 向量、Cookie 合并、登录 / 续期 / 扫码、歌单、歌曲、取链、歌词、搜索）；`lyrics` 177（解析、QRC、KRC、zlib、打分、批量、歌词服务）；`download` 122（单曲下载、媒体辅助、并发池、任务编排、NCM 导入）；`library` 66；`tags` 65；`ncm` 49；`online` 88（QQ / 酷狗 / LRCLIB / 编排）；`auth` 30；`session` 28；`settings` 28；`sync` 27；`utilities` 33（错误文案、文件名、音频类型、取消）；`paths` 20；`editor` 21；`playlist` 15；`models` 12；`error` 5；`logs` 3；`qr` 3；其他 1 |
| `lp-core` 集成测试 | 36 | `csharp_compat` 5、`csharp_download` 2、`csharp_library` 10、`csharp_lyrics` 4、`csharp_ncm` 4、`csharp_online` 7、`csharp_tags` 4——与 C# 版生成的「黄金」夹具逐字节 / 逐字段对照 |
| `src-tauri` 外壳 | 33 | 请求结构、设置只写补丁并保留其他字段、设置不可信时不写盘、任务槽、事件名等 |

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
  - `.\build.ps1 -Test` 全过（上面的 1014）；`.\build.ps1` 默认构建成功，`app\target\release\LunaticPrincess.exe` 约 6.8 MB。
  - `.\build.ps1 -Bundle -Version v2.0.0-manual.1`：版本号写入 `Cargo.toml` / `tauri.conf.json` / `package.json` 并参与编译（构建输出为 `lp-core v2.0.0-manual.1`），**便携版 zip 生成成功**（`LunaticPrincess-2.0.0-manual.1-win-x64-portable.zip`，3.0 MB，里面只有 `LunaticPrincess.exe`，没有 `data`）。解压到 `D:\claude\lp-portable\`、`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data` 启动：窗口标题 `Lunatic Princess`、进程 `Responding`，数据落在指定目录的 `webview\EBWebView`；测完已清空 `D:\claude`。
  - **NSIS 安装包在本次自动化环境里没能生成**：Tauri 打包器把下载的 NSIS 工具解压到 `%LOCALAPPDATA%\tauri\nsis-3.13` 后要把目录改名为 `NSIS`，而这台机器上 Claude 桌面版的子进程对 `%LOCALAPPDATA%` 的写入被 MSIX 文件系统虚拟化到包私有目录（`C:\WpSystem\…\Packages\AnthropicPBC.Claude_…\LocalCache\Local\tauri`），改名时报 `os error 17`（无法将文件移到不同的驱动器）；另有一次下载中途 `unexpected end of file`（网络瞬时故障）。这是运行环境的限制，不是工程配置问题（打包配置已被 Tauri 解析通过，流程走到了「Verifying NSIS package」）。**需要在普通 PowerShell 里执行 `.\build.ps1 -Bundle` 确认安装包能生成**；GitHub Actions 的 windows-latest 上没有这个虚拟化。
  - 版本解析（`scripts/resolve-version.ps1` 与 `main.yml` 里的步骤，本地用 Windows PowerShell 5.1 模拟）：`v2.0.0`→`2.0.0`、`v1.3`→`1.3.0`、`v.1.0.0`→`1.0.0`、`V1.0.0`→`1.0.0`、`v2.0.0-rc.1`→`2.0.0-rc.1`、`v0.0.1-manual`→`0.0.1-manual`、`v1`→`1.0.0`；分支手动运行 → `2.0.0-manual.<运行序号>`；`v1.2.3.4`、`vfoo` 等非 SemVer 直接失败。两个 workflow 用 PyYAML 解析通过（结构、触发条件、权限、步骤顺序已核对）；**workflow 本身没有真正在 GitHub 上跑过**，首次推送后要看一次 Actions。

## 已知限制

1. **`/weapi/login/token/refresh` 返回 `code=400`**（桌面端扫码得到的会话；请求与 C# 版完全一致，C# v1.3 同样受影响）：续期是否真的延长登录有效期尚未证实；登录态本身有效。需要长期观察 `MUSIC_U` 是否到期掉线，必要时改查 `eapi` 续期端点。（待修问题，见 `docs/TAURI_REWRITE.md`）
2. **网易云搜索匿名不可用**：`/api/search/get/web` 现在返回十六进制密文；旧的 `/api/search/get`、`/weapi/search/get` 返回与关键词无关的结果；`/weapi/cloudsearch/get/web` 匿名返回 `code=50000005`。登录态下 `cloudsearch` 是否可用、是否要改走 `eapi`，需要真实账号验证。目前 `search_songs` 遇到密文会报明确错误，QQ 音乐不受影响（当前真正可用的主词源），编辑器记忆的默认搜索来源也改成了 QQ 音乐。（待修问题）
3. 扫码登录的 803 确认段需要真人手机扫码，自动化只能验证到出码与 801 轮询。
4. 真实账号下的下载 / 同步 / 状态列 / 歌词预览 / 歌曲行「编」按钮、以及从资源管理器真实拖拽，没有在无人值守环境验证，需要用户真机确认。
5. 无损 / Hi-Res 以上档位受账号会员权益限制：无权限时服务端静默降档或失败，程序不绕过（会提示实际音质）。
6. 逐字歌词取决于来源是否提供 YRC / QRC / KRC；都没有时回退逐行 LRC。
7. 不支持 WMA 标签读写（lofty 不支持 ASF）；FLAC 封面是原生 PICTURE 块；Windows 资源管理器只认 RIFF INFO 的 WAV 标签，RIFF INFO 没有总数、歌词、封面。
8. 网易云 / QQ / 酷狗接口均为非公开接口，随时可能变更；酷狗在部分网络下报证书域名不匹配或握手 EOF，异常吞掉后回落下一个词源。
9. 没有本地播放功能；单账号；`session.dat` 绑定 Windows 用户，换用户或换机器需要重新扫码。
10. 没有致命异常落盘日志（`panic = "abort"`，运行日志只在内存）。
11. 管理员身份运行时 Windows 阻断资源管理器向程序拖放（UIPI），界面只能提示。
12. 玻璃「透出」只能看到被虚化的下一层，做不了真正的局部折射；系统材质的完整效果只在 Windows 11 上；亚克力在拖动 / 缩放窗口时比云母更耗电。
13. 仓库放在带低完整性标签的目录（如 Edge 的下载文件夹）时，直接运行的 exe 会把数据落在程序目录 `data\`，见 `docs/TAURI_REWRITE.md` 的「已知环境问题」。
14. 安装包与便携版没有代码签名（SmartScreen 可能提示未知发布者），没有自动更新。
15. 前端没有自动化测试。
