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
| `paths` | `Core/Storage/AppPaths.cs` |
| `settings` | `Core/Settings/` |
| `session` | `Core/Storage/`（`ProtectedSessionStore`、`SessionAutoSaver`） |
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
- **不要把可执行文件放在含非 ASCII 字符的路径下运行**：在系统区域为繁体中文（代码页 950）的机器上，当前仓库路径里的简体字（`edge浏览器下载`）会让 WebView2 创建失败（`os error 5` / `0x800700AA`）。把 exe 复制到纯 ASCII 路径（如 `C:\lp-test\`）即可正常运行，这是本机环境问题，与应用代码无关。
- 窗口背景：Tauri 窗口为透明无边框，Rust 侧在 `setup` 中给窗口铺 Mica，失败回退 Acrylic；前端 `body` 保持透明，由半透明 token 叠加。

## 模块清单与进度

| # | 模块 | 内容 | 状态 |
|---|---|---|---|
| 1 | 项目骨架 | Cargo workspace、`lp-core` 空骨架、Tauri 外壳、自绘标题栏、主题 token、`build.ps1 -Tauri` | 完成 |
| 2 | 基础层 | 数据目录解析、`settings.json`、DPAPI `session.dat`（与 C# 版兼容） | 待做 |
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
