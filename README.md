[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
![.NET](https://img.shields.io/badge/.NET-8.0-512BD4?logo=dotnet)
![Windows](https://img.shields.io/badge/Platform-Windows-0078D4?logo=windows)
![Avalonia](https://img.shields.io/badge/Avalonia-11-8A2BE2)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri)
![Vue](https://img.shields.io/badge/Vue-3-42B883?logo=vue.js)
![GitHub release (latest by date)](https://img.shields.io/github/v/release/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer)
![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/main.yml)
![GitHub last commit](https://img.shields.io/github/last-commit/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer)
![GitHub repo size](https://img.shields.io/github/repo-size/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer)


# Lunatic Princess

一个面向 Windows 的网易云音乐桌面工具：扫码登录后长期保持登录态，读取账号歌单，扫描本地曲库，
自动对比歌单与本地文件差异，按选定音质下载缺失歌曲，解密本地 NCM 文件，并把标题、歌手、专辑、
封面与逐行/逐字歌词写入音乐标签。

> 仅用于个人备份自己有权访问的音乐。请遵守网易云音乐用户协议与当地版权法律，不要分发下载内容。

## 功能

- **歌单浏览**
- **本地曲库对比**
- **NCM 解密并补全标签**
- **歌词与标签匹配**


## 目录结构

```
LunaticPrincess.sln
src/
  LunaticPrincess.Core/        # 可复用的平台无关业务层
    Netease/                       # 登录、weapi 加密、歌单/歌曲/歌词 API
    Ncm/                           # NCM 解密
    Lyrics/                        # LRC / 增强 LRC / YRC 解析与序列化
    Tags/                          # 标签读写
    Library/                       # 本地扫描与匹配
    Download/                      # 下载、NCM 导入
    Sync/                          # 一键同步编排
    Storage/ Settings/ Models/     # 登录态、设置、领域模型
  LunaticPrincess.Server/      # ASP.NET Core sidecar：把 Core 暴露成本地 HTTP 接口（Tauri 版后端）
  LunaticPrincess.Desktop/     # Avalonia 界面（旧版，保留）
desktop/                       # Tauri 2 + Vue 3 + shadcn-vue 新界面
  src/                             # 页面与组件
  src-tauri/                       # Rust 壳：启动 sidecar、注入 API 基地址、建窗口
scripts/
  build-desktop.ps1            # 一键构建 Tauri 桌面端（sidecar + 前端 + Rust 壳）
tests/
  LunaticPrincess.Core.Tests/  # 单元测试（不依赖网络）
```

界面有两种形态，共用同一套业务层：旧的 Avalonia 版（`src/LunaticPrincess.Desktop`）与新的
Tauri 版（`desktop` + `src/LunaticPrincess.Server` sidecar），两者可并行开发。


## 构建与运行

前置条件：Windows 10/11、.NET 8 SDK。仓库内提供了本地 SDK 时可直接使用 `build.ps1`。

```powershell
# 还原 + 构建（Debug）
.\build.ps1

# 运行
.\src\LunaticPrincess.Desktop\bin\Debug\net8.0\LunaticPrincess.exe

# 单元测试
.\build.ps1 -Test

# 发布 Windows x64（默认轻量框架依赖包）
.\build.ps1 -Publish
# 输出：publish\win-x64\LunaticPrincess.exe
# 发布包：publish\LunaticPrincess-win-x64.zip

# 如需不依赖本机 .NET 的自包含版（体积更大）
.\build.ps1 -Publish -SelfContained
```

默认发布为轻量框架依赖模式，目标机器需要安装 **.NET 8 Desktop Runtime**（zip 约 11MB）；
自包含模式会把运行时一起打包（约 45MB，适合没有 .NET 的机器）。

如果使用系统安装的 .NET 8 SDK，也可以：

```powershell
dotnet restore LunaticPrincess.sln
dotnet build LunaticPrincess.sln -c Release
dotnet test tests/LunaticPrincess.Core.Tests/LunaticPrincess.Core.Tests.csproj
```

### Tauri 桌面端（新界面）

前置条件：Node.js 20+ 与 pnpm、Rust stable 工具链、WebView2 Runtime（Windows 10/11 一般已内置）、
.NET 8 SDK（用于发布 sidecar）。

```powershell
# sidecar（自适应单文件）→ Vue 前端 → Rust 壳，全部由脚本串起来
.\scripts\build-desktop.ps1

# 只重建 Rust 壳（前端已构建过时用）
.\scripts\build-desktop.ps1 -SkipInstall -SkipFrontend -SkipServer

# 生成 NSIS 安装包（需要 tauri-cli）
.\scripts\build-desktop.ps1 -Bundle
```

产物目录 `publish\desktop-release\`（Debug 为 `publish\desktop-debug\`）里需要**三个文件同放**：
`lunatic-princess.exe`（Rust 壳）、`lunatic-princess-server.exe`（.NET sidecar）、
`WebView2Loader.dll`（缺少它壳进程会以 0xC0000135 立即退出）。脚本会把三者一起复制过去；
把整个目录拷到任意位置即可运行，程序优先使用 exe 旁的便携 `data\`（登录态、设置、日志都在这里）。


## 已知限制

- 扫码登录 Cookie 不能通过刷新接口续期；若失效需要重新扫码。
- Tauri 版依赖 WebView2 Runtime；没有安装时窗口无法创建。
- Tauri 版每次启动使用系统分配的空闲端口与 .NET sidecar 通信，端口不固定，不要写死端口或做端口转发。

第三方组件与参考项目见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

