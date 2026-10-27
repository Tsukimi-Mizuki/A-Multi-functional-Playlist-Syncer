[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)





# Lunatic Princess

一个面向 Windows 的网易云音乐桌面工具：扫码登录后长期保持登录态，读取账号歌单，扫描本地曲库，
自动对比歌单与本地文件差异，按选定音质下载缺失歌曲，解密本地 NCM 文件，并把标题、歌手、专辑、
封面与逐行/逐字歌词写入音乐标签。

> 仅用于个人备份自己有权访问的音乐。请遵守网易云音乐用户协议与当地版权法律，不要分发下载内容。

## 功能

- **歌单浏览**
- **本地曲库对比**
- **NCM 解密并补全标签**



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

