[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
![.NET](https://img.shields.io/badge/.NET-8.0-512BD4?logo=dotnet)
![Windows](https://img.shields.io/badge/Platform-Windows-0078D4?logo=windows)
![Avalonia](https://img.shields.io/badge/Avalonia-11-8A2BE2)
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

- **歌单浏览**：扫码登录后读取账号全部歌单，侧栏可收起为封面栏，歌曲表显示时长、专辑与本地状态。
- **本地曲库对比**：扫描本地目录，按网易云 ID / 标题+歌手+时长匹配歌单曲目，标记“本地已存在 /
  NCM 待解密 / 需下载”。
- **九档音质**：标准 / 极高 (HQ) / 无损 (SQ) / 高解析度无损 (Hi-Res) / 高清臻音 (SpatialAudio) /
  沉浸环绕声 (Surround Audio) / 超清母带 (Master) / 臻音全景声 (Audio Vivid) / 杜比全景声 (Dolby Atmos)；
  账号没有对应特权时接口会自动降档，进度中提示实际拿到的音质。
- **NCM 解密并补全标签**：支持客户端下载的 .ncm 批量解密（MP3/FLAC、内嵌封面与元数据），
  可选解密后按设置优先级自动在线匹配标签、封面与双语歌词；导入窗口记忆上次的输入/输出目录。
- **歌词与标签智能匹配**：同时支持网易云与 QQ 音乐（含 QQ QRC 逐字歌词），多变体查询 + 可调匹配度
  阈值（默认 60%）减少错配；独立歌词/标签编辑器可联网搜索、预览封面与逐行/逐字双语歌词后写回；
  支持按目录批量补全标签、封面与歌词。


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
  LunaticPrincess.Desktop/     # Avalonia Windows 界面
tests/
  LunaticPrincess.Core.Tests/  # 单元测试（不依赖网络）
```


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


## 已知限制

- 扫码登录 Cookie 不能通过刷新接口续期；若失效需要重新扫码。

第三方组件与参考项目见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

