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
| `session` | `Core/Models/NeteaseSession.cs`、`Core/Storage/`（`ProtectedSessionStore`；`SessionAutoSaver` 留到模块 3） |
| `netease` | `Core/Netease/` |
| `ncm` | `Core/Ncm/` |
| `lyrics` | `Core/Lyrics/` |
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
| 3 | 登录 | weapi 加密、HTTP、Cookie 合并与续期、扫码登录 UI | 待做 |
| 4 | 歌单 | 歌单列表与详情（trackIds + 分批 song/detail） | 待做 |
| 5 | 歌词解析 | LRC / 增强 LRC / YRC / QRC（3DES 变体）/ KRC | 待做 |
| 6 | 标签读写 | MP3 / FLAC / M4A / OGG 标签、封面、歌词写入 | 待做 |
| 7 | NCM 解密与导入 | `.ncm` 解密、元数据与封面回写、导入窗口 | 待做 |
| 8 | 本地曲库与对比 | 曲库扫描、与歌单一对一匹配 | 待做 |
| 9 | 下载与同步 | 九档音质取链下载、一键同步编排 | 待做 |
| 10 | 在线匹配 | 网易云 / QQ / 酷狗 / LRCLIB、歌词与标签编辑器、批量匹配 | 待做 |
| 11 | 设置与外观 | 设置窗口、主题切换、帧率等 | 待做 |
| 12 | CI 打包、文档、删除 C# | 安装包 / zip、GitHub Actions、更新 AGENTS.md 与 README、删除 C# 代码 | 待做 |

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
