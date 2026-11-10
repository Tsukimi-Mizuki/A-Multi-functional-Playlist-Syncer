# AGENTS.md — 协作约定与项目交接

> 本文件既是**长期协作约定**，也是**交接说明书**。接手本项目的人或 AI 助手请先通读，再按 §2 做一遍自检。
>
> 配套文档：`docs/ARCHITECTURE.md`（分层、命令与事件、关键流程、窗口效果）、`docs/IMPLEMENTATION_STATUS.md`（实现与验证记录、已知限制）、
> `docs/TAURI_REWRITE.md`（重写期间逐模块的实现记录、**「待修问题」**）、`README.md`（面向用户的说明）、`THIRD_PARTY_NOTICES.md`（第三方组件与许可证）。

## 1. 项目概况

| 项目 | 内容 |
|---|---|
| 名称 | **Lunatic Princess**（旧名 NeteaseMusicManager，已整体更名；设置 / 登录态目录会自动迁移） |
| 定位 | Windows 桌面端网易云音乐管理器：扫码登录 → 歌单浏览 → 本地曲库对比 → 按音质下载 → NCM 解密 → 歌词/标签/封面写入；另带在线歌词与标签匹配（网易云 / QQ 音乐 / 酷狗 / LRCLIB） |
| 平台 | Windows 10/11 x64（Tauri 2 桌面，WebView2 渲染）。其他平台不支持，也没有计划 |
| 技术栈 | Rust 1.99（edition 2024）、Tauri 2.12、React 19 + TypeScript 7 + Vite 8、pnpm 11；标签 lofty、HTTP reqwest（Schannel）、测试 `cargo test` |
| 许可证 | AGPL-3.0（见 `LICENSE`）。仅用于个人备份自己有权访问的音乐，不分发音频、不绕过版权与会员限制 |
| 仓库 | https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer |
| 当前状态 | **2.2.3（Rust + Tauri 重写版）**：重写模块 1–12 全部完成，`cargo test --workspace` 1116 个测试全过，fmt / clippy（`-D warnings`）/ `pnpm build` 通过；两个「待修问题」（登录续期、网易云搜索）都已修复。**已发布 tag `v2.0`（2.0.0）、`v2.1`（2.1.0：运行日志收起 / 展开动画、歌词预览跟随「歌词来源」）、`v2.1.1`（2.1.1：启动时检查 GitHub 最新正式版并弹窗提醒）、`v2.1.2`（2.1.2：背景雾度、精简设置说明）、`v2.1.3`（2.1.3：「下载缺失」「一键同步」二次确认，缺失清单可取消勾选）、`v2.2`（2.2.0：液态玻璃主题重做——内置背景 WebGL 真折射、对话框折射下层界面、高级设置与即时预览）、`v2.2.1`（2.2.1：拖入多首或文件夹时交给批量获取）、`v2.2.2`（2.2.2：封面更清楚——网易云最长边 4000、QQ 1200，自动补全的标签与封面网易云优先）与 `v2.2.3`（2.2.3：批量修改标签——右边歌曲列表、左边选中歌曲的标签，可多选批量改；编辑器显示封面参数；新增作词家）** |
| 分支 | `main` = Tauri 重写版（旧 `main` 的快进延续，历史连续）；`CSharp-GUI` = 旧的 C# / Avalonia 版（v1.x，含 v1.3），只作历史与维护用途，**不要往里合并新功能**；`GUI-Tauri`、`tauri-experiment` 等重写期间的分支已并入 `main` 或删除 |
| 历史 tag | `v0.0.1`、`v0.0.1-manual`、`v.1.0.0`（带前导点）、`v1.1`、`v1.2`、`v1.3` 都是 C# 版；Tauri 版从 `v2.0`（版本号 2.0.0）起 |

## 2. 接手后先做的自检（约 20 分钟）

1. **环境**：Windows 10/11 x64 + PowerShell 5.1 以上；Rust stable（≥ 1.99，msvc 工具链，带 rustfmt / clippy）；Visual Studio 2022 生成工具（C++ 桌面开发）；Node.js（开发用 26）与 pnpm 11；WebView2 运行时（Win11 自带）。
2. **跑基线**：`.\build.ps1 -Test` → fmt、clippy、`cargo test --workspace`（**1116 个测试全部通过**：lp-core 库内 1042 + 集成 36 + 外壳 38）、`pnpm build` 全过（首次含依赖下载与编译，约 5–10 分钟）。
3. **出包并试用**：`.\build.ps1` 得到 `app\target\release\LunaticPrincess.exe`；`.\build.ps1 -Bundle` 在 `dist-release\` 得到安装包与便携版 zip。用**自己的**账号扫码登录，确认歌单加载、本地对比、下载、NCM 导入四个链路可用（这四条的「真实账号」验证历来要靠用户真机，见 `docs/TAURI_REWRITE.md` 各模块的「验证」）。
4. **读文档**：`docs/ARCHITECTURE.md` 的「关键流程」「外观与窗口效果」，`docs/IMPLEMENTATION_STATUS.md` 的「已知限制」，`docs/TAURI_REWRITE.md` 的 **「待修问题」**（两个问题已确认并修复，记录与修复说明都在那里，别重复踩）。
5. **确认干净**：`git status` 应无改动；`dist-release/`、`.tmp/`、`app/target/`、`app/node_modules/`、`publish/`、`.dotnet/` 等都已 gitignore，不要在仓库里留临时脚本、探针或登录态。
6. **如果仓库放在 Edge 的下载文件夹里**（或任何带低完整性标签的目录）：程序与测试以低完整性运行，不能写 `%LOCALAPPDATA%` / `%TEMP%`。`build.ps1` 已把 `TMP` / `TEMP` 指到工作区 `.tmp`；直接运行 exe 时数据会落到程序目录 `data\`。细节见 `docs/TAURI_REWRITE.md` 末尾的「已知环境问题」。最省事的办法是把仓库搬出该文件夹：用不复制安全信息的方式复制（例如 `robocopy /E /COPY:DAT`，跳过 `app\target`、`app\node_modules` 后在新位置重新 `pnpm install` / 构建），新文件会继承新目录的权限、不再带低完整性标签（本机 2026-11-04 已这样搬到 `D:\claude\workspace`）。

## 3. 技术栈与版本（改依赖前先看这里）

| 项 | 版本 / 说明 |
|---|---|
| Rust | stable，开发用 1.99.1；`edition = "2024"`；需要 rustfmt、clippy |
| Tauri | crate `tauri` 2.12.5、`tauri-build` 2.7、`tauri-plugin-dialog` 2.8.4；npm `@tauri-apps/cli` / `@tauri-apps/api` 2.12.4、`@tauri-apps/plugin-dialog` 2.8.4 |
| 前端 | React 19.3、TypeScript 7.0（严格模式）、Vite 8.3、`@vitejs/plugin-react` 6.1；**不引入 UI 组件库**，样式是手写 CSS（`app/src/styles/`） |
| 包管理 | pnpm 11，锁文件 `app/pnpm-lock.yaml`（CI 与 `build.ps1` 用 `--frozen-lockfile`） |
| 异步 / HTTP | tokio 1（`rt` `sync` `time`）、reqwest 0.13（`native-tls` 即 Windows Schannel，系统证书与系统代理；不启用 cookie 容器） |
| 加密 / 编码 | `aes` 0.9 + `cbc` 0.2（weapi；aes 的 ECB 用于 eapi）、`md-5` 0.10（eapi 摘要）、`num-bigint` 0.5（无填充 RSA）、`base64` 0.23、`rand` 0.10、`flate2` 1.1（zlib）、`regex` 1.13 |
| 标签读写 | lofty 0.25.5 |
| 二维码 | `qrcode` 0.14（只开 `svg`），前端用 `data:image/svg+xml` 显示 |
| Unicode | `icu_normalizer` / `icu_properties` 2.4（NFKC 与字符类别，用来复现 .NET 的匹配语义） |
| Windows API | `windows-sys` 0.61（DPAPI、已知文件夹、进程令牌提权检测、`ShellExecuteW` 用默认浏览器打开发布页） |
| 序列化 / 错误 | serde 1、serde_json 1、thiserror 2 |
| 发布 profile | `lto = true`、`codegen-units = 1`、`opt-level = "s"`、`panic = "abort"`、`strip = true` |
| 打包 | Tauri bundler 的 NSIS（`bundle.targets = ["nsis"]`，当前用户安装、可选安装目录，简体中文 / 英文，安装钩子 `src-tauri/windows/installer-hooks.nsh`）；便携版 zip 由 `build.ps1` 生成 |

## 4. 目录结构与职责

```
build.ps1                          # 唯一的构建 / 测试 / 打包入口
scripts/resolve-version.ps1        # tag -> SemVer（CI 与 build.ps1 -Version 共用）
.github/workflows/ci.yml           # main 的 push / PR：build.ps1 -Test
.github/workflows/main.yml         # v* tag 与手动运行：检查 + 打包 (+ Release)
app/
  Cargo.toml  Cargo.lock           # 工作区（members = src-tauri, crates/lp-core），共享版本号 / 许可证 / profile
  package.json  pnpm-lock.yaml  vite.config.ts  tsconfig*.json  index.html
  crates/lp-core/                  # 与界面无关的业务库，必须可单测（见下表）
    src/                           # 业务代码；测试在同目录的 tests.rs / <模块>/tests.rs 里
    tests/                         # 集成测试：csharp_*.rs 对照 C# 版生成的「黄金」夹具
    tests/fixtures/                # 夹具（合成数据，没有任何真实歌曲 / 歌词），每个子目录有 README
  src-tauri/                       # Tauri 外壳：命令、事件、窗口、打包配置
    src/lib.rs                     # 全部命令 + AppState 组合根 + 窗口材质 + 外壳单测
    tauri.conf.json  capabilities/ icons/
  src/                             # React 前端
    api/*.ts                       # 与后端命令一一对应的类型化 invoke 封装 + DTO 类型
    state/*.ts                     # 业务 hook：登录、歌单、本地对比、下载同步、选择、日志、拖放、编辑器、批量修改标签、外观
    components/*.tsx               # 标题栏、侧栏、歌曲表格、各对话框（登录 / NCM / 标签 / 歌词 / 编辑器 / 批量修改标签 / 设置）
    styles/*.css                   # theme.css（主题 token 与动效）、glass.css（液态玻璃）及各区域样式
    glass/*.ts                     # 液态玻璃：WebGL2 渲染器与着色器、参数表与 CSS 近似换算、背景解析、对话框折射
    dev/mockBackend.ts             # 普通浏览器里的假后端（仅开发，生产构建会被摘除）
    lib/                           # 格式化、虚拟滚动
docs/                              # ARCHITECTURE / IMPLEMENTATION_STATUS / TAURI_REWRITE / design
dist-release/                      # build.ps1 -Bundle 的输出（gitignore）
```

`lp-core` 的模块（每个模块的细节、与 C# 版的差异见 `docs/TAURI_REWRITE.md` 对应章节）：

| 模块 | 职责 |
|---|---|
| `paths` / `settings` / `models` | 数据目录解析、`settings.json`（`AppSettings` + `JsonSettingsStore` + 严格补丁 `apply_patch`；`settings::liquid_glass` 是液态玻璃参数的范围表）、领域枚举（`AudioQuality`、`ThemeMode`、`WindowMaterial`、`BackgroundSource`、`GradientPreset`、`LyricsSourceKind` …） |
| `session` / `auth` | DPAPI `session.dat`、`SessionAutoSaver`；登录编排 `LoginManager`（启动校验、续期、扫码、退出、运行期复核） |
| `netease` | 非公开接口：`crypto`（weapi + eapi）、`http`（传输 trait + reqwest 实现）、`client`（会话 / Cookie 合并 / 超时）、`login` `playlists` `songs` `player`（取链）`lyrics` `search`，以及 `mappers` / `models` |
| `playlist` | 歌单加载编排（取列表 / 曲目、写日志、301 走登录复核） |
| `lyrics` | `parser`（LRC / 增强 LRC / YRC / QRC / 译文）、`qrc`（QQ 3DES 变体）、`krc`（酷狗）、`zlib`、`html`、`service`（歌词预览，按设置里的「歌词来源」换源）、`scorer`（匹配打分）、`batch`（批量服务 + 任务编排 + 在线补全挂钩） |
| `tags` | `LoftyTagService`：MP3 / FLAC / M4A / OGG / OPUS / WAV 标签、封面、歌词、网易云 ID（原子写入）；`text` 辅助、`view` DTO、`table`（批量修改标签：列表行、只写提到的字段的 `TagPatch`、并行读 / 写） |
| `ncm` | `decoder`（流式解密）、`client_comment`（客户端文件注释里的 `163 key`）、`synthetic`（测试用合成 NCM） |
| `library` | `scanner`（并行读标签）、`matcher`（一对一匹配）、`text`（.NET 语义的文本归一化）、`manager`（扫描缓存与对比）、`view` |
| `download` | `service`（单曲下载）、`fetch`（流式取文件）、`media`（文件名 / 原子落盘 / 标签数据）、`pool`（并发任务池）、`task`（下载 / 同步任务编排）、`ncm_import`（NCM 导入） |
| `sync` | `SyncService`：本地命中 / NCM 解密 / 下载三分支 |
| `online` | `service`（网易云 + QQ 搜索与详情）、`qq`、`kugou`、`lrclib`、`net`（超时与取消）、`manager`（编辑器 / 批量用的编排）、`view` |
| `editor` | 编辑器记忆、拖入解析、启动参数、封面文件读取、提权检测 |
| `update` | 检查更新：读 GitHub `releases/latest`、tag 归一与 SemVer 比较（`Version`，规则同 `scripts/resolve-version.ps1`）、已忽略版本、写运行日志；`open_release_page` 只打开本项目发布页 |
| `logs` / `qr` / `error` / `utilities` | 运行日志（内存 400 条）、二维码、错误类型、`error_text`（中文错误文案）/ `file_name_sanitizer` / `audio_file_types` / `cancel` |

## 5. 构建 / 测试 / 发布

### build.ps1（推荐入口，PowerShell 5.1 与 7 都能跑）

| 命令 | 作用 |
|---|---|
| `.\build.ps1` | `pnpm install --frozen-lockfile` + `pnpm tauri build --no-bundle`（前端构建 + `cargo build --release`）→ `app\target\release\LunaticPrincess.exe` |
| `.\build.ps1 -Test` | 只检查不打包：`cargo fmt --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`pnpm build`（tsc + vite） |
| `.\build.ps1 -Bundle` | 先 `pnpm tauri build --no-bundle` 编译，从**未被打包器改写**的 exe 做便携版 zip，再 `pnpm tauri bundle --bundles nsis`，在 `dist-release\` 得到 `LunaticPrincess-<版本>-win-x64-portable.zip`（**exe + 一个空的 `data/` 目录条目**，直接写进 zip，从不拷贝真实数据目录）、`LunaticPrincess-<版本>-win-x64-setup.exe`、`SHA256SUMS.txt`。首次需要联网下载 NSIS 工具与 WebView2 引导程序：`bundle.useLocalToolsDir = true`，缓存在 `app\target\.tauri\`（`cargo clean` 后重新下载；偶发 `unexpected end of file` 是网络中断，重跑即可）。不用 Tauri 默认的 `%LOCALAPPDATA%\tauri`，因为在 MSIX 打包应用（例如 Claude 桌面版）的子进程里 `%LOCALAPPDATA%` 被虚拟化，Tauri 改名 NSIS 目录时会报 `os error 17` |
| `.\build.ps1 -Test -Bundle` | 先检查测试，再打包 |
| `-Version <tag 或版本>` | 与 `-Bundle` 合用。经 `scripts/resolve-version.ps1` 归一成 SemVer（`v1.3` → `1.3.0`，`v.1.0.0` → `1.0.0`，`V2.0.0-rc.1` → `2.0.0-rc.1`），**临时**写进 `app/Cargo.toml`、`app/package.json`、`app/src-tauri/tauri.conf.json`（`Cargo.lock` 也会被 cargo 更新），构建结束后原样恢复，工作区不会变脏 |
| `-Tauri` | 旧开关，保留但已无作用（Tauri 构建现在就是默认行为），会给出警告，计划以后删除 |

脚本还做了：刷新 PATH（只追加，不覆盖 CI 里 `GITHUB_PATH` 加的项）、`TMP`/`TEMP` 指向工作区 `.tmp`、在 GitHub Actions 里用 `::group::` 折叠每个步骤的日志。
脚本只服务 Tauri 版，**不会碰** `publish\`、`.dotnet\` 等 C# 版遗留的本地目录。

### 手动命令（在 `app\` 目录下）

```powershell
$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')   # 新 shell 找不到 cargo/pnpm 时
$env:TMP = (Resolve-Path ..\.tmp).Path; $env:TEMP = $env:TMP      # 仓库在低完整性目录时必须
pnpm install
cargo test -p lp-core --lib download::service                      # 跑一部分测试
cargo test -p lp-core --test csharp_lyrics                         # 跑某个集成测试
pnpm tauri dev                                                     # 桌面开发窗口（Vite 固定端口 1420）
pnpm dev                                                           # 只开前端，浏览器里用假后端预览
```

### 运行与调试

- 发布版 exe：`app\target\release\LunaticPrincess.exe`；开发版：`app\target\debug\LunaticPrincess.exe`（需要前端已构建或 dev server 在跑）。
- 命令行参数：`--edit <文件>` / `--lyrics <文件>` 启动后直接打开歌词与标签编辑器（文件必须存在，不存在只在运行日志里警告）。
- 实机测试请把 exe **复制到别处**（例如 `D:\claude\lp-run\`）并设 `LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，不要动真实登录态和音乐目录；测完清空这些测试目录。本机的仓库自 2026-11-04 起就在 `D:\claude\workspace`，清理时只删自己建的测试子目录，**绝不要清空整个 `D:\claude`**。
- 驱动真实界面：给被测进程设 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<端口>`，再用 CDP 调命令 / 读 DOM / 截图（历史验证都这么做，探针脚本验证完即删）。
- 运行日志只在内存（最多 400 条，不落盘）；`panic = "abort"`，目前**没有**致命异常落盘日志（`<数据目录>\logs` 路径已预留但没人写入）。

## 6. 运行时数据、路径与隐私

数据目录解析顺序（`lp-core/src/paths.rs`，每个候选都会做真实写入探测，不可写就降级）：

1. 环境变量 `LUNATIC_PRINCESS_DATA_DIR`（旧名 `NETEASE_MUSIC_MANAGER_DATA_DIR` 仍兼容，新变量为空时才看旧变量）；
2. 程序目录下已存在 `data\` → 便携模式，固定使用它（保证换启动方式后登录态不变）。**安装版与便携版都走这一条**：安装钩子（`app/src-tauri/windows/installer-hooks.nsh`）在用户选的安装目录里建 `data\`，便携版 zip 自带空的 `data\`，所以设置、登录态（Cookie）、WebView2 缓存与默认下载目录 `data\Music` 都在程序目录下。安装钩子只在卸载页勾选「删除应用程序数据」（且不是升级）时删 `settings.json` / `session.dat`（含 `.tmp`）/ `webview\` / `logs\`，`data\Music` 与 `data\` 只在为空时删，绝不删下载的音乐；改了数据目录里的文件名要同步改钩子；
3. `%LOCALAPPDATA%\LunaticPrincess`；若只有旧目录 `%LOCALAPPDATA%\NeteaseMusicManager` 会先整体迁移；
4. 程序目录 `data\`；
5. `%TEMP%\LunaticPrincess`。

数据目录里的文件：

| 文件 | 说明 |
|---|---|
| `settings.json` | 明文 JSON（camelCase，枚举按名字序列化，读取时键名不区分大小写）。与 C# 版互通：C# 读取时忽略 Tauri 版新增的 `windowMaterial` / `glassOpacity` / `backdropHaze` / `reduceMotion` / `autoCheckUpdates` / `ignoredUpdateVersion` / `background` / `gradientPreset` / `backgroundImage` / `liquidGlass` |
| `session.dat` | 登录 Cookie，DPAPI（CurrentUser，entropy `LunaticPrincess.Session.v1`）加密，文件头 `NMMSES01`；换 Windows 账号或换机器**无法解密**，需重新扫码。C# 版写的文件 Tauri 版能读，反之亦然 |
| `webview\` | WebView2 用户数据目录（缓存、localStorage）。显式放在数据目录下，见 `docs/TAURI_REWRITE.md` 的低完整性说明 |
| `logs\` | 预留，目前没有代码写入 |

默认下载 / 曲库目录：便携模式为 `data\Music`；否则首选 `%MyMusic%\LunaticPrincess`（旧目录 `%MyMusic%\NeteaseMusicManager` 已存在则沿用，避免文件分裂）；再退到 `data\Music`；最后 `%TEMP%\LunaticPrincess\Music`。

`settings.json` 键（全部有默认值，缺字段就取默认；某字段类型错误只让该字段回落默认，其余保持；`normalize()` 校验并夹取）：

| 键 | 含义 | 默认 |
|---|---|---|
| downloadDirectory / libraryDirectory | 下载 / 曲库目录 | 见上 |
| quality | 音质档（`Standard/ExHigh/Lossless/HiRes/SpatialAudio/SurroundAudio/Master/AudioVivid/DolbyAtmos`；旧值 `Higher` 迁移为 `ExHigh`） | ExHigh |
| lyricsMode | `None/Line/Word` | Line |
| writeCover / overwriteExisting / skipExisting | 封面与覆盖策略 | true / false / true |
| fileNameTemplate | 文件名模板（占位符 title/artist/album/track/year/id） | `{artist} - {title}` |
| maxConcurrency | 下载并发（1–8） | 3 |
| autoMatchLocal | 加载歌单后自动对比本地曲库 | true |
| tagSourcePriority / lyricsSourcePriority | `NeteaseFirst/QQMusicFirst/NeteaseOnly/QQMusicOnly` | NeteaseFirst |
| ncmImportFolders / ncmImportOutputDirectory | NCM 导入窗口记忆 | [] / 空（回退下载目录） |
| lyricsEditorSearchSource / lyricsEditorPreferWordLyrics | 编辑器记忆的搜索来源与歌词格式 | Netease / false |
| fallbackKugouLyrics / fallbackLrclibLyrics | 备用词源开关 | true / true |
| matchThreshold | 在线匹配度阈值（0.30–1.00，银行家舍入到两位） | 0.60 |
| theme | `Dark/Light/System` | Dark |
| renderFrameRate | 沿用的旧键，Tauri 版界面不使用 | VSync |
| windowMaterial | `Acrylic/Mica/None`（Tauri 版新增） | Acrylic |
| glassOpacity | 玻璃浓度 0.10–0.95（Tauri 版新增） | 0.45 |
| backdropHaze | 背景雾度 0.00–1.00：对话框下面那层界面的虚化与遮暗程度，0 完全清晰、1 完全看不清（四舍五入到两位；Tauri 版新增） | 0.30 |
| reduceMotion | 减少动效（Tauri 版新增） | false |
| autoCheckUpdates | 启动时自动检查 GitHub 上的新版本（Tauri 版新增） | true |
| ignoredUpdateVersion | 「忽略本次更新」记住的版本（归一后的 SemVer，无法识别的清空；由更新弹窗写入，设置窗口不改它；Tauri 版新增） | 空 |
| background | 主界面背景 `System/Gradient/Image/PlaylistCover`：后三种由界面用 WebGL 画、玻璃真正折射，这时不铺系统材质（`effective_window_material`）；`System` 透出桌面、玻璃为 CSS 近似（Tauri 版新增） | Gradient |
| gradientPreset | 渐变光斑配色 `Aurora/Ocean/Sunset/Forest`（Tauri 版新增） | Aurora |
| backgroundImage | 自选背景图片的路径（只记路径，不复制图片；Tauri 版新增） | 空 |
| liquidGlass | 液态玻璃高级参数（对象）：折射 / 色散 / 菲涅尔 / 高光 / 模糊 / 色调 / 阴影 / 圆角与超椭圆，20 个数值 + `blurEdge` + `tintColor`；范围与默认见 `settings/liquid_glass.rs` 与 `docs/design/README.md`。读取宽容（逐项回落默认、夹取），补丁可以只含改动的参数（Tauri 版新增） | 见设计文档 |

隐私与交接注意：`session.dat` 与个人路径属于用户数据，**不要提交、不要放进发布包**（便携版 zip 里的 `data\` 只是一个空目录条目，`build.ps1` 从不拷贝任何真实数据目录）；下载目录里可能有受版权保护的文件，不要随项目交付；
本地的 `publish\win-x64\data`（C# 版遗留）里是用户真实的登录态与设置，**不要删除或修改**。

## 7. 架构速查（详细见 docs/ARCHITECTURE.md）

### 分层与组合根

`React 前端 → Tauri 命令 / 事件（src-tauri） → lp-core`。`src-tauri/src/lib.rs` 里的 `AppState` 是**组合根**：启动时解析路径、加载设置与登录态，构造 `NeteaseClient`、各服务和管理器并交给 Tauri 托管；命令只做参数转换、调用 lp-core、把进度变成事件。
`lp-core` 不引用 Tauri（Windows 相关的只有 DPAPI、已知文件夹、提权检测，都用 `windows-sys`），业务逻辑放这里并带单测；新业务逻辑写在命令函数或前端 hook 里通常说明位置放错了。

### 关键流程（改代码前先确认这些不变量）

- **扫码登录**：`unikey(type=3)` → 二维码（失败降级 web `type=1`）→ 每约 2 秒轮询 `801/802/803/800` → 803 时合并 Set-Cookie 与响应体 cookie → `NeteaseSession` → 先落盘再让登录态生效；启动时用 `/weapi/nuser/account/get` 校验。**登录续期与 Cookie 合并**：`NeteaseClient` 对每个带会话的响应调用 `CookieMerger`（按名覆盖、Max-Age 优先于 Expires、空值不覆盖、合并后会丢 `MUSIC_U` 则忽略；同一响应里同名的 `Path=/` 删除与设值并存时删除忽略、带非根 `Path` 的删除永远忽略，见 `netease::cookie_merger` 文档），变化时通知 `SessionAutoSaver` 串行落盘；`refresh_login` 走 **eapi** `POST interface.music.163.com/eapi/login/token/refresh`（签名路径 `/api/login/token/refresh`，`header` 里带 `os=pc` 桌面标识与会话的 `__csrf` / `MUSIC_U` / `NMTID`，桌面 UA、无 Referer；weapi 同名端点只会 `code=400` 或 `bizCode=201` 空操作，不换发令牌），成功时服务端换发新的 `MUSIC_U`（约 180 天）/ `__csrf` / `MUSIC_R_U` 并经 `CookieMerger` 落盘；启动校验成功后调用一次，运行期每约 12 小时一次（间隔可注入，单测用暂停时钟）。**「登录失效」只认接口业务 `code == 301`，或 `account/get` 返回 200 但 account/profile 为空**；HTTP 401/302/5xx、406、-460、超时、网络错误一律只记「暂时无法验证登录状态，已保留登录」；清除前先续期并复核一次；清除顺序是 `set_session(None)` → `flush` → 删 `session.dat`。
- **歌单**：`/weapi/user/playlist` 翻页（最多 10 页）；大歌单先取 `trackIds`，再按 500 首 / 批调用 `/weapi/v3/song/detail` 补齐并保持原顺序，某批失败重试一次，仍失败置 `is_partial`（界面给横幅与「重新加载」）。
- **本地对比**：扫描（并行读标签，结果按路径排序）→ 匹配。优先用网易云 ID（本程序写入的 `LunaticPrincess:<id>` 注释，或客户端文件里的 `163 key(Don't modify)` 解出的 `musicId`）精确匹配；否则标题 0.55 / 歌手 0.30 / 时长 0.15 综合评分，**阈值固定 0.72**，每首本地文件最多匹配一首歌单曲。没扫描过显示「—」而不是「需下载」。
- **下载**：九档音质 → `/weapi/song/enhance/player/url/v1`（取链请求带桌面客户端 Cookie，否则 Hi-Res 会被静默降级）→ 逐级降档 → 服务端 `level` 与请求不同时提示「实际音质」→ 试听片段判失败 → 流式写 `.part` 临时文件 → 文件头是 NCM 就解密 → 歌词 / 翻译 / 封面 → lofty 写标签 → 原子落盘 → 在线补全（标签与封面网易云优先、QQ 兜底，歌词 QQ 逐字优先；文件里已有封面时只换成像素更多的，关掉「写入封面」时不写封面）。补全失败只作提示，不影响下载成功。取消时保留已完成结果并清理临时文件。
- **NCM 解密**：输出格式看首块（ID3 / 帧同步字 → mp3，`fLaC` → flac）；音频不整文件读入内存，32 KB 分块边读边解边写临时文件，成功后原子改名；先写回内嵌元数据 / 封面 / 网易云 ID；导入窗口默认勾选「解密后自动获取在线标签、封面与歌词」。
- **在线匹配**：主源 `OnlineMusicSearchService`（网易云匿名 eapi `/eapi/cloudsearch/pc`，失败再回退网页接口 `/api/search/get/web`；QQ `/soso/fcgi-bin/search_cp`）；都拿不到歌词时按 `LyricsSource` 顺序回落酷狗 KRC、LRCLIB。打分与候选筛选集中在 `lyrics::scorer`，阈值来自设置（运行期读共享设置对象，保存后下一首歌就生效）。**任何搜索都不自动应用结果**，编辑器里由用户点选后再载入。
- **同步**：`SyncService` 按本地状态分三支（已存在且无覆盖 → 跳过；NCM → 解密到临时子目录 + 写标签 + 在线补全；缺失 → 下载），与下载共用并发任务池，结果顺序与输入一致。
- **二次确认**：「下载缺失」「一键同步」先弹确认框（`useTransferConfirm`），确认后才开始任务；还没对比本地曲库就先对比（必要时扫描，扫描被取消 / 失败就不弹窗）。「下载缺失」列出缺失歌曲（默认全选，可取消勾选、Shift 连选），确认后把仍勾选的 ID 传给 `download_missing(trackIds)`，后端只下载其中**仍然**缺失的；「一键同步」显示跳过 / NCM 解密 / 需下载各多少首、音质与下载目录（开了「覆盖已存在文件」时写明会重新下载）。计数按歌曲 ID 去重，与后端一致；曲库目录不存在时与后端一样全部算缺失并在框里提示；没有缺失时只在工具条提示、不弹窗。「下载选中」不需要确认。
- **液态玻璃**：内置背景时 `GlassBackdrop` 用 WebGL2 画背景，并把带 `data-glass` 的标题栏、侧栏、歌曲区、日志栏画成折射的玻璃（斯涅尔折射、色散、菲涅尔、按角度的高光，算法改编自 liquid-glass-studio，MIT）；系统材质下这些面板，以及两种背景下的按钮、对话框、卡片，都由 `glassCssVars` 把同一组参数换算成 CSS 近似。对话框打开时 `.app-shell` 按背景雾度虚化，`#root` 再套 SVG 位移滤镜让对话框边缘折射下层界面（`glass/underGlass.ts`，只在单个对话框时）。改参数范围要同时改 Rust 的表（`settings/liquid_glass.rs`）与前端的 `glass/params.ts`（`PARAM_GROUPS`、`DEFAULT_LIQUID_GLASS`），后端会严格校验。
- **拖放**：拖入一首（或只含一首的文件夹）用歌词与标签编辑器打开；拖入多首、或含多首的文件夹（含子文件夹，规则同批量修改标签扫描目录）交给「批量修改标签」，清单就是拖入的文件（`resolve_drop` 返回 `batch` + `files`，按拖入顺序、去重，无法处理的目标跳过并写日志）；编辑器或批量窗口开着时交给最上面的窗口（批量窗口开着时新拖入的一批替换清单，正在处理时不换）；只有 `.ncm` 时提示改用「导入 NCM」。
- **批量修改标签**（`BatchTagsDialog` + `useBatchTags`，布局参考见 `docs/design/README.md`）：右边是歌曲列表（选目录或拖入，标签由 `read_tag_rows` 分批读出，读不出来的文件也占一行），选中歌曲（单击 / Ctrl / Shift / Ctrl+A / 方向键）后左边显示它们的标签，值不一样的字段显示「〈保持〉」。保存时 `patch_tags` **只写动过的字段**（`TagPatch`：没出现 = 不动，`null` / 空白 = 清除，落到 `AudioTagData::clear`），逐个文件返回结果，一个失败不影响其余；改备注时保留网易云 ID 标记；歌词不在面板里改，「编辑…」用歌词与标签编辑器打开那一首。「在线获取」（原来的批量获取标签、封面与歌词，`batch_run`）只处理选中的歌，没选就是全部；面板里有没保存的改动时不能开始。换选中的歌、换清单、关窗口之前先确认没保存的改动。新增标签字段时要同时改 `AudioTagData` / `AudioTagInfo` / `TagField`、`TagView` / `WriteTagsRequest`、`TagRowView` / `TagPatch`、前端 `api/tags.ts`、编辑器（`useLyricsEditor` 用 `replaceAll` 保存，漏了字段会被清掉）与批量面板（`useBatchTags` 的字段表）。
- **标签写入**：复制成同目录临时副本 → 写副本 → 重新解析确认类型没变 → 原子替换；任何一步失败都保留原文件。`replace_all` 保存时保留网易云 ID 标记与封面（没提供封面就不动封面）。
- **检查更新**：启动约 3 秒后，`autoCheckUpdates` 开着就匿名请求 GitHub `releases/latest`（只有正式版），tag 归一后按 SemVer 与当前版本比较：新版本且不是 `ignoredUpdateVersion` → 弹窗（跳转至下载页面 / 忽略本次更新 / 稍后提醒）；忽略过的版本、已是最新 → 只写日志。「稍后提醒」只关窗，下次启动再提醒；「忽略」只跳过这一个版本。设置「关于」的「立即检查」不受开关影响，忽略过的版本也照样弹窗。程序**不下载、不安装**更新，浏览器只会打开本项目的发布页。自动检查失败记普通日志，手动检查失败记警告。
- **错误文案**：`utilities::error_text` 识别权限不足 / 空间不足 / 文件被占用 / 路径过长并给出「是什么 + 怎么办」；命令的 `Err` 统一是 `{ kind, message }`（message 为中文），**不把英文异常抛给用户**。
- **取消**：长任务（扫描、下载、同步、NCM 导入、批量）各自一个任务槽，同时只有一个（`busy`）；取消用 `CancelToken`，在每个网络请求处立刻生效，已完成的结果保留。

### 关键常量

| 项 | 值 | 位置 |
|---|---|---|
| 匹配度阈值 | 默认 0.60，范围 0.30–1.00 | `lyrics::scorer` |
| 在线打分权重 | 有歌手：标题 0.55 / 歌手 0.25 / 时长 0.20；无歌手：标题 0.72 / 时长 0.20 / 专辑 0.08；歌手相似度 < 0.30 整体 ×0.75；时长容差 ±8 s | `lyrics::scorer` |
| 本地对比阈值 | 0.72（与设置无关），时长容差 12 s | `library::matcher` |
| 单源候选数 | 每来源前 10 条、最多 3 组查询变体（`build_queries` 至多 6 组） | `lyrics::scorer` / `lyrics::batch` |
| 批量匹配并发 | 3 | `lyrics::batch::task` |
| 下载并发 | 默认 3，clamp 1–8 | `AppSettings::normalize` |
| 运行日志条数 | 400（只在内存） | `logs` |
| NCM 分块 | 32 KB | `ncm::decoder` |
| 歌单详情批量 | 500 首 / 批 | `netease::playlists` |
| 登录续期间隔 | 12 小时 | `auth` |
| 二维码轮询 | 约 2 秒 | 前端 `useAuth` |
| 检查更新 | 启动后约 3 s 一次；超时 15 s、不重试；更新说明最多 4000 字 | `update` / 前端 `useUpdates` |
| 封面尺寸 | 网易云最长边上限 4000 px、输出 JPEG；QQ 依次请求 1200 / 800 / 500 / 300；单张下载上限 10 MB | `netease::cover` / `online::qq` / `download::fetch` |
| 接口超时 | weapi 20 s；QQ / 封面 20 s；酷狗 / LRCLIB 10 s；音频下载连接 15 s、读取间隔 60 s、总 30 分钟 | `netease::client` / `online::net` / `netease::http` |

## 8. 代码约定

- **能力先落 `lp-core`，并带单测**；`src-tauri` 只做命令转发与事件，前端只做编排与展示。
- **Rust**：`cargo fmt` 默认风格；`clippy --all-targets -D warnings` 必须干净；错误用 `thiserror`，面向用户的文本一律中文；不写 `unwrap()` / `expect()` 于生产路径（测试里可以）；I/O 密集的同步代码（标签、解密、扫描）在调用方用 `spawn_blocking`；网络与长任务 `async` + `CancelToken`，进度走回调 / 事件；依赖用 trait 注入（传输层 `HttpTransport`、`TagService`、`NcmDecoder`、`OnlineMusicSearch`、`Downloader` …），测试写手写 Fake，不引入 mock 框架。
- **新增命令**：在 `lp-core` 里实现 → `src-tauri/src/lib.rs` 写 `#[tauri::command]` 并加进 `generate_handler!` → `app/src/api/*.ts` 加类型化封装与 DTO → `dev/mockBackend.ts` 加假实现 → 外壳单测（请求结构 / 设置写入范围）。DTO 一律 camelCase（`#[serde(rename_all = "camelCase")]`）。
- **TypeScript / React**：严格模式，函数组件 + hook；业务状态放 `state/` 的 hook，组件只渲染；后端数据结构只在 `api/` 里定义；高频事件（下载进度）先写进 ref，每帧最多刷新一次；长列表用 `lib/virtual.ts` 的虚拟滚动。
- **中文文案**：所有用户可见文字用中文；注释用中文摘要，解释「为什么」而不是复述代码；不提交注释掉的旧代码。构建脚本（`build.ps1`、`scripts/`）保持 **ASCII**，因为 Windows PowerShell 5.1 会把无 BOM 的脚本按 ANSI 读。
- **依赖**：不轻易加包（Rust 与 npm 都一样），加了要写进 `docs/TAURI_REWRITE.md` 对应模块的「新增依赖」和 `THIRD_PARTY_NOTICES.md`。
- **可访问性**：新增的可交互控件补中文 `aria-label`：既是无障碍要求，也是本项目历史验证（读控件名 / 文案）依赖的锚点。
- **提交前**：跑 `.\build.ps1 -Test`；工作区不留临时脚本、探针、日志。

## 9. 测试约定

- 位置：`app/crates/lp-core`（库内 `#[cfg(test)]` 模块与集成测试）和 `app/src-tauri`（外壳的请求结构 / 设置写入范围测试）。**不依赖网络、不依赖真实音频文件**（`cargo test` 全程不联网）。
- 现状（2026-11）：**1116 个**——`lp-core` 库内 1042、集成测试 36（`csharp_compat` 5、`csharp_download` 2、`csharp_library` 10、`csharp_lyrics` 4、`csharp_ncm` 4、`csharp_online` 7、`csharp_tags` 4）、外壳 38。前端没有自动化测试（见 §14）。
- **`csharp_*.rs` 是什么**：重写期间用 C# 版（`CSharp-GUI` 分支）在同样的输入上跑出的「黄金」输出，提交在 `tests/fixtures/`；Rust 实现必须与之逐字节 / 逐字段一致（浮点误差 1e-9）。夹具全是合成数据，说明在各子目录 `README.md`。C# 源码已不在 `main` 上，**这些夹具不能再生成，只能读**；确需新增黄金数据，到 `CSharp-GUI` 分支写临时探针（用完即删）。
- 测试夹具与替身（都在 `lp-core` 里）：`test_util::FakeTransport`（按序返回响应、记录请求、可挂起制造「在途」窗口）、`tags/fixtures.rs`（按规范生成最小合法 MP3 / WAV / M4A；FLAC / Ogg / Opus 是 `tests/fixtures/audio/` 里的几百字节静音文件）、`ncm/synthetic.rs`（按格式规范做**逆运算**造出真实可解的 `.ncm`，与生产实现互相独立）、`download/test_support.rs`、`online/test_support.rs`、`TempDir` 帮手。
- 加测试时同步更新 `docs/IMPLEMENTATION_STATUS.md` 的总数与分项，以及本文件 §1 / §2 / §9 的数字。
- 真实网络验证：可以临时写探针跑一次（本项目历次都是这么验证的），**验证完立刻删除**，不要把依赖网络的测试提交进仓库，也不要放进会被 `cargo test` 全量执行的路径；歌词正文有版权，不打印不入库。
- 跑单个测试：`cargo test -p lp-core --lib <路径片段>`、`--test csharp_online`（环境变量见 §5）。

## 10. 外部接口与实网经验（最容易失效的部分）

这些接口都不是公开 API，随时可能变；出问题优先怀疑接口而不是本地逻辑。

- **网易云**：`weapi` 需要 AES-CBC 双重加密 + 无填充 RSA 的 `encSecKey`；取链 `/weapi/song/enhance/player/url/v1` 需要桌面客户端 Cookie 才能拿到 Hi-Res 及以上，无会员特权时服务端**静默降档**（实测匿名请求 Hi-Res 得到 `exhigh`），所以要读响应里的 `level` 并提示实际音质。**搜索走匿名 eapi `POST /eapi/cloudsearch/pc`**（`netease::crypto::eapi_encrypt`：`params = HEX(AES-128-ECB(key e82ckenh8dichen8, "</api/ 路径>-36cd479b6b5-<JSON>-36cd479b6b5-<md5>"))`，请求体里带桌面端 `header` 对象，不带登录态；实测匿名可用、海外 IP 也是明文）。**网页接口 `/api/search/get/web` 在海外 IP 下返回十六进制密文**（`{"result":"<hex>","abroad":true}`，eapi / weapi / linuxapi 的固定 key 都解不开），所以它只作回退；`/weapi/cloudsearch/get/web` 匿名恒返回 `code=50000005`，别再试。搜索接口连续多变体请求仍可能 `code=406` 限流（限流 / 风控不回退）；eapi 偶发 TLS `unexpected EOF during handshake` 时会换 `music.163.com` 主机再试一次。
- **QQ 音乐**：搜索旧接口 `client_search_cp` 已下线，现用 `/soso/fcgi-bin/search_cp` + `new_json=1`；`c.y.qq.com` 连接失败 / 非 2xx 自动切 `c6.y.qq.com`。歌词用 `music.musichallSong.PlayLyricInfo`（`qrc=1&crypt=1&roma=1`），QRC 是 3DES 变体：字节编号 / 位序与标准 DES 不同（已知向量：key `133457799BBCDFF1`、plain `0123456789ABCDEF` → `F63A972CE52AA327`，而非教科书的 `85E813540F0AB405`），解出后 zlib 解压再按 QRC 行 / 词时间戳解析。保留的怪癖：PlayLyricInfo 没有译文时双语版本等同原文，所以 QQ 的 `has_translation` 恒为真。
- **酷狗**：三段链路 `mobilecdn.kugou.com/api/v3/search/song` → `krcs.kugou.com/search` → `lyrics.kugou.com/download?fmt=krc`；KRC 前 4 字节明文 `krc1`，之后 16 字节固定密钥 XOR 再 zlib；**候选的 `duration` 是毫秒**（C# 版当秒，导致酷狗在实网永远没结果，Tauri 版已修）；本机偶发证书域名不匹配（`SEC_E_WRONG_PRINCIPAL`）与 `unexpected EOF during handshake`，异常吞掉后回落下一个词源，不加重试。
- **LRCLIB**：只有逐行 `syncedLyrics`；先用 `?q=` 多变体检索，落空再用 `track_name/artist_name` 兜底；429/503 属瞬时失败，退避后重试一次。
- **登录**：803 确认段必须真人手机扫码，自动化只能验证到 801；登录续期走 eapi `/eapi/login/token/refresh`（2026-11-04 联网实测：换发新 `MUSIC_U` / `__csrf` / `MUSIC_R_U`，旧令牌换发后仍有效，可连续续期）；weapi 的 `/weapi/login/token/refresh` 对桌面端会话返回 `code=400`（C# 版同样，原「待修问题 ①」，已修复），带桌面 Cookie 时返回 `code=200 bizCode=201` 却不换发令牌；`MUSIC_U` 真正过期或被服务端作废时仍需重新扫码。服务端在同一响应里会对同名 Cookie 下发多条不同 `Path` 的 `Set-Cookie`（含 `Path=/` 的删除指令，顺序随机），`CookieMerger` 已按 `Path` 处理。
- **GitHub（检查更新）**：匿名 `GET api.github.com/repos/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases/latest`（`Accept: application/vnd.github+json`、`X-GitHub-Api-Version: 2022-11-28`，必须带 User-Agent，不带任何 Cookie）。未登录每个 IP 每小时 60 次，超了返回 403 / 429（界面提示稍后再试）；`releases/latest` 不含预发布与草稿，所以预发布 tag 不会触发提醒。仓库改名或换地址时要同步改 `lp-core/src/update.rs` 的 `LATEST_RELEASE_API` 与 `RELEASES_PAGE`。
- **封面尺寸（2026-11-09 实测）**：网易云 `picUrl` 是上传的原图（约 600 张榜单封面里 3000×3000 最常见，4000×4000 有 29 张，最大 12081×12081；PNG 原图能到 19 MB，超过 10 MB 的会被整张丢弃）。图床参数 `?imageView&thumbnail=4000x4000&type=jpg` 在服务端处理：超过 4000 的等比缩小、PNG 转 JPEG（19 MB → 1.1 MB），4000 以内的 JPEG 原样返回；`?param=宽y高` 只缩放不转格式，不会放大。QQ 封面 `y.gtimg.cn/music/photo_new/T002R<尺寸>x<尺寸>M000<albummid>.jpg` 只有 150 / 300 / 500 / 800 / 1200 这几档（1000、2000 回 404），原图比请求的小时直接回原图。C# 版固定取 QQ 的 300×300，自动补全又是 QQ 优先、覆盖已有，所以下载时写入的网易云原图会被换成 300×300，这是 2026-11-09 修掉的问题。
- 网络偶发 `tls handshake eof`：重试即可，不是代码问题。
- **版权**：不绕过会员 / 版权限制；试听片段直接判失败。
- 所有在线请求都有独立超时与可读的中文错误；封面下载限制 10 MB；解析失败保留本地已有内容。

## 11. 界面设计与视觉基线

- **界面设计一律以用户提供的设计参考为准**：对话中给出的设计稿 / 截图就是视觉基准，改配色、圆角、间距、控件形态、排版前先对照，不要自行更换视觉语言。设计稿存 `docs/design/`（命名 `ui-<窗口或场景>.png`），并在 `docs/design/README.md` 登记文件名与文字要点——无法看图的人（包括 AI）靠这些文字实现。目前该目录还没有设计稿，视觉基线以下面这一节为准。
- **液态玻璃**主题（深色 / 浅色 / 跟随系统）：效果参考是用户指定的 liquid-glass-studio（参数含义、范围、默认值见 `docs/design/README.md`）。背景默认是内置的渐变光斑（WebGL 画、面板真正折射，也可选自选图片 / 歌单封面），选「系统材质」时主窗口透明无边框，Rust 侧给窗口铺系统材质（亚克力 / 云母 / 无，`windowMaterial`），失败自动退到另一种；`theme.css` 只定义每套主题的底色三元组与高光，各层不透明度由 `--glass-strength`（`glassOpacity`）推导；`glass.css` 统一给标题栏、侧栏、歌曲区、对话框、设置卡片加顶部柔光、内侧高光、投影与 1px 边缘折光。颜色一律用 CSS 变量，不写死色值。
- **景深**：给「下一层」加 `filter`：有对话框时整个主界面 `.app-shell` 平滑虚化（`body:has(> .modal-backdrop:not(.modal-leaving)) .app-shell`），叠放对话框时被盖住的也虚化；单个对话框时 `#root` 上再套 SVG 位移滤镜，让对话框边缘折射下层界面（先模糊再折射，两个滤镜分在两个元素上，雾度的过渡才不被 url() 滤镜打断）。虚化半径与对话框后的遮罩 `--scrim` 都由 `--backdrop-haze`（设置「背景雾度」`backdropHaze`）推导：主界面 `h × 30 px`、被叠住的对话框 `h × 20 px`，遮罩不透明度 `h·(0.4+s)·(1−h) + 0.92·h²`（s 为玻璃浓度），0 时完全清晰、1 时几乎被盖满，默认 0.30 即原来的固定效果。对话框用 `createPortal` 挂到 `body`，避免被上一个对话框的 filter 截成局部定位容器。早期 WebView2 的透明窗口里 `backdrop-filter` 不生效；2026-11 实测 WebView2 156 对页面内容已经生效，但它会在折射之后再模糊一遍、把折射抹掉，所以对话框**不用** `backdrop-filter`。**不要为了「更像玻璃」把对话框底色改回高不透明度，也不要依赖 `backdrop-filter` 做模糊。**
- **动效**：时长（120 / 200 / 320 / 460 ms）与缓动统一在 `theme.css`；对话框进场缩放 + 淡入，退场靠卸载时留下的静态「残影」播放淡出；主题切换用 View Transition 整窗交叉淡化；「减少动效」（`reduceMotion` 或系统开启「减少动画」）把所有过渡缩到 1 ms。
- 窗口：无边框自绘标题栏（`TitleBar`，68 px 顶栏即标题栏，自绘最小化 / 最大化 / 关闭，可拖动，双击最大化），窗口最小 960×600；弹出层用 `Modal` 组件（模态栈：Esc 只关最上面一个）。
- 受限环境（远程桌面 / 关闭透明效果）下系统材质不可用，窗口退化为纯透明 + 界面自身的玻璃底色。
- **改 UI 必须实机验证**：①普通浏览器 `pnpm dev` + 假后端（`mockBackend.ts`，`?mock=loggedout|empty|listerror|slow|libmanual|libmissing|libbig|dlslow|dlnote|ncmempty|ncmbig|neteasefail|startup|kugounone|batchbig|elevated|systembg` 等场景）检查渲染与状态，深色 / 浅色都看；②启动发布版 exe（复制到独立目录 + 独立数据目录），用 WebView2 远程调试端口读 DOM / 调命令 / 截图，至少确认窗口能开、关键文案正确、设置能保存、深色 / 浅色都不崩。
- 管理员身份运行时 Windows 用 UIPI 阻断「资源管理器 → 应用」的拖放，这是系统限制，界面只能提示（`editor::is_elevated`），不要试图绕过。

## 12. Git、版本与 CI 发布

- 提交信息：**英文祈使句、一次提交一件事**（例：`Prefer QQ word lyrics for automatic online matching`、`Rewrite build.ps1 for the Tauri app`）；**不加** `Co-Authored-By` 之类的署名行。
- 提交身份：`Tsukimi-Mizuki <Tsukimi_Mizuki@outlook.com>`（仓库级 `git config`）。**交接后由新维护者改成自己的身份**，并考虑是否同步改 `LICENSE` / README 与仓库地址。
- 提交前：`.\build.ps1 -Test` 通过；`git status` 干净；不提交 `dist-release/`、`.tmp/`、`app/target/`、`app/node_modules/`、缓存与临时脚本（`.gitignore` 已覆盖；`bin/`、`obj/`、`.dotnet/`、`publish/` 等 C# 版遗留目录也留在忽略表里，切回 `CSharp-GUI` 分支时它们还要用）。
- 不要 force push `main`；不要往 `CSharp-GUI` 加新功能。
- **CI（`.github/workflows/`）**：
  - `ci.yml`：`main` 的 push 与 pull request → windows-latest，pnpm / Node / Rust stable（带 rustfmt、clippy，`Swatinem/rust-cache` 缓存）→ `./build.ps1 -Test`。
  - `main.yml`（名称 Release）：推送 `v*` tag 或手动运行 → 同样的环境 → 解析版本号 → `./build.ps1 -Test` → `./build.ps1 -Bundle -Version <版本>` → `actions/upload-artifact`（总是）→ 有 tag 时 `softprops/action-gh-release@v2` 建 Release（正文是 annotated tag 的说明正文，后面附自动生成的变更链接；所以**打 tag 一定用 `git tag -a` 写改动列表**，版本带 `-` 的标为预发布）。**手动运行（分支上）不建 Release**，版本是 `tauri.conf.json` 的版本加 `-manual.<运行序号>`，只影响产物命名。
  - 版本解析在 `scripts/resolve-version.ps1`：兼容 `v1.3`（补成 `1.3.0`）、`V1.0.0`、旧的 `v.1.0.0`（去前导点）、`v2.0.0-rc.1`；不是 SemVer 的 tag 直接报错。外部输入（tag 名）一律通过 `env:` 传入脚本，不直接拼进命令。
  - Release 资产（名字不含空格）：`LunaticPrincess-<版本>-win-x64-setup.exe`、`LunaticPrincess-<版本>-win-x64-portable.zip`、`SHA256SUMS.txt`。
- **发版流程**：先确认没有新的「待修问题」要在此版本处理 → 把 `app/Cargo.toml`、`app/package.json`、`app/src-tauri/tauri.conf.json` 的版本改成同一个数字并提交（CI 以 tag 为准、会临时覆盖，但仓库里的数字应与发布一致，`Cargo.lock` 里成员版本随 cargo 构建更新，一并提交）→ 本地 `.\build.ps1 -Test -Bundle` → `git push origin main` → `git tag -a v<版本> -F <说明文件>`（第一行 `Lunatic Princess v<版本>`，空一行后写改动列表，会成为 Release 正文）→ `git push origin v<版本>` → 在 Actions 里确认通过、到 Releases 核对资产并试装一次。2.0.0 用的 tag 是 `v2.0`（与 `v1.3` 一样两段式，解析成 `2.0.0`）。
- 安装包说明：NSIS、当前用户安装（不要管理员权限）、安装时可选目录（默认 `%LOCALAPPDATA%\Lunatic Princess`，Tauri 用 `productName` 命名）、简体中文 / 英文、WebView2 缺失时联网下载引导程序（Tauri 默认 `downloadBootstrapper`）；未做代码签名，SmartScreen 可能提示未知发布者。

## 13. 常见修改配方

1. **新增 / 调整音质档**：改 `lp-core/src/models/audio_quality.rs`（枚举 + `api_level()/api_levels()/display_name()/bitrate()/from_api_level()/fallback_chain()`）→ 设置下拉与工具栏下拉由 `qualityOptions` 自动带出 → 补该文件的测试 → 更新 `docs/ARCHITECTURE.md` 的音质映射表。
2. **新增设置项**：`AppSettings` 加字段 + `new()` 默认值 + `normalize()` 校验（非法值归默认）→ `apply_patch` 里加这个键的严格校验（类型 / 枚举 / 范围）→ 若是枚举改名，在设置读取的迁移处理旧值 → `src-tauri` 的 `SettingsDto` 如需额外选项一并带出 → 前端 `api/settings.ts` 类型、`SettingsDialog.tsx` 加控件（补 `aria-label`）、`mockBackend.ts` → 补默认值 / 夹取 / 补丁 / C# 兼容读取的测试（`csharp_compat` 只比较 C# 认识的键）。
3. **新增在线词源**：在 `lp-core/src/online/` 实现 `LyricsSource`（`label()` + `fetch(&LyricsQuery, &CancelToken)`；任何网络 / 证书 / JSON 错误都回落成「没有结果」，只有取消才是 `Err`）→ 加进 `AppState` 装配的备用词源列表 → 若要可开关，加 `AppSettings` 布尔项并接进 `online::fallback_source_enabled` → 若要出现在编辑器下拉，扩展 `LyricsSourceKind`、`online_search` / `lyrics_source_fetch` 与 `LyricsEditorDialog` → 测试用 `FakeTransport` 脚本化响应。
4. **新增对话框**：`components/XxxDialog.tsx`（用 `Modal`，别自己写遮罩）+ `state/useXxx.ts` + `api/xxx.ts` + 后端命令（配方见 §8「新增命令」）→ 可交互控件补 `aria-label` → 假后端加场景 → 深色 / 浅色实机检查，叠放时 Esc 只关最上面一个。
5. **改错误提示**：优先扩 `utilities/error_text.rs`（把错误映射成「是什么 + 怎么办」的中文），补对应测试；命令返回 `{ kind, message }`。
6. **动 NCM / 歌词 / 标签写入**：先读 `docs/TAURI_REWRITE.md` 对应模块章节与现有测试；`ncm/synthetic.rs` + `tags/fixtures.rs` 可以造出真实可用的样本做端到端验证；改了行为要重跑 `csharp_*` 集成测试，它们是与 C# 版逐字节对照的护栏。
7. **改打包 / 版本**：安装包选项在 `app/src-tauri/tauri.conf.json` 的 `bundle`；产物命名与便携版组装在 `build.ps1` 的 `-Bundle` 段；版本解析在 `scripts/resolve-version.ps1`；改完用 `.\build.ps1 -Bundle -Version v9.9.9-test` 本地演练（构建完会自动恢复清单）。

## 14. 已知限制与技术债

- **待修问题（已全部修复，记录保留在 `docs/TAURI_REWRITE.md`「待修问题」）**：①登录续期 `/weapi/login/token/refresh` 返回 `code=400` 已于 2026-11-04 修复（改走 eapi `/eapi/login/token/refresh`，见「登录续期修复」；仍待长期观察 180 天后的实际掉线情况，以及 `MUSIC_R_U` 路径限定令牌的用途）；②「网易云搜索匿名不可用」已于同日修复（改走匿名 eapi，见「网易云搜索修复」），编辑器默认搜索来源已按用户决定于 2026-11-09 改回网易云（封面是原图，比 QQ 清楚；已有设置文件里写着的来源原样保留）。
- 没有本地播放功能：定位是「歌单同步 / 下载 / 解密 / 标签管理」。
- 前端**没有自动化测试**（逻辑集中在 `lp-core` 单测；界面靠假后端 + 实机验证）；纯逻辑（Shift 连选、进度并入）已放在可导出的函数里，需要时可补前端测试框架。
- 真实账号下的下载、同步、状态列、歌词预览、歌曲行「编」按钮、资源管理器真实拖拽，历来无法在无人值守环境验证，**需要用户真机确认**（清单在 `docs/TAURI_REWRITE.md` 各模块「验证」）。
- 外部接口随时可能变（见 §10）；匹配成功率依赖第三方可用性。
- 单账号：没有账号切换；`session.dat` 绑定 Windows 用户，换用户或换机器必须重新扫码。
- 没有致命异常落盘日志；运行日志只在内存。
- 无损 / Hi-Res 以上档位受会员权益限制：无权限时降档或失败，程序不绕过。
- 不支持 WMA 标签读写（lofty 不支持 ASF）；FLAC 封面是原生 PICTURE 块；Windows 资源管理器只认 RIFF INFO 的 WAV 标签。
- 真正的折射只发生在界面自己画的内容上：内置背景下的主面板（WebGL）与对话框边缘（SVG 位移滤镜）；「系统材质」透出的桌面网页读不到，那时面板、按钮只是 CSS 近似。内置背景需要 WebGL2，不可用时自动退回系统材质的样式。亚克力在拖动 / 缩放时比云母更耗电（设置里有说明）。浏览器预览窗格被隐藏时 `requestAnimationFrame` 不触发，WebGL 画布不会重画（验证时要让窗格或 exe 窗口可见）。
- 安装包与便携版未签名；更新只做到「提醒 + 打开发布页」，不会自动下载安装。
- `lp-core` 的 `csharp_*.rs` 与夹具名里的 `csharp` 是历史命名，指的是「C# 版生成的黄金数据」。
- Android 等平台没有实现：`lp-core` 与 Tauri 外壳分离，理论上可复用，但 DPAPI、已知文件夹、提权检测需要替换。

## 15. 交接清单（转交前逐项确认）

- [ ] 仓库所有权 / 协作者权限、GitHub Actions 与 Releases 权限已移交（CI 只用默认 `GITHUB_TOKEN`，无额外 secret）。
- [ ] 新维护者的 `git config user.name/user.email` 已设置；`LICENSE`、README 徽章与仓库链接是否要改（仓库地址变了还要改检查更新用的两个常量，见 §10「GitHub」）。
- [ ] 删除个人数据：`%LOCALAPPDATA%\LunaticPrincess`、`%MyMusic%\LunaticPrincess` 里的下载内容、本地遗留的 `publish\win-x64\data`（含 `session.dat`）；检查 `settings.json` 里有没有个人路径。
- [ ] `git status` 干净，仓库里没有临时脚本 / 日志 / 发布产物；`git log` 与远端一致；`main` 与 `CSharp-GUI` 两个分支都已推送。
- [ ] 在干净环境重新跑 `.\build.ps1 -Test`（1116 全过）与 `.\build.ps1 -Bundle`，并在一台没装开发环境的机器上试装安装包、解压便携版确认能启动。
- [ ] 与接手人当面过一遍 §2 自检、§10 外部接口风险、§12 发版流程，并确认谁负责「待修问题」、后续 tag / Release，以及 2.0.0 的发版时机。
- [ ] 约定后续节奏：分支策略（是否引入 develop / PR 流程）、发版频率、是否给安装包做代码签名、是否继续推进本地播放。
