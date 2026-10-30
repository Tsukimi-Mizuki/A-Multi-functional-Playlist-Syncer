# AGENTS.md

本文件记录人机协作的长期约定（构建方式、代码风格、界面设计基准）。后续会话或协作者动手前先读这里。

## 项目概览

- **Lunatic Princess**：Windows 端网易云音乐管理器（歌单浏览、本地曲库对比、按音质下载、NCM 解密、歌词/标签/封面写入）。
- `.NET 8` + `C# 12` + `Avalonia 11.3` + `CommunityToolkit.Mvvm` + `TagLibSharp`；测试用 `xUnit`。
- 目录：
  - `src/LunaticPrincess.Core`：与 UI 无关的核心逻辑（网易云接口、NCM、下载、歌词、标签、扫描、设置），必须可单测。
  - `src/LunaticPrincess.Desktop`：Avalonia GUI（View + ViewModel + Services）。
  - `tests/LunaticPrincess.Core.Tests`：Core 的单元测试。

## 构建 / 测试 / 发布

- `.\build.ps1 -Test`：构建 + 全量单元测试（当前 331/331 通过）。
- `.\build.ps1 -Publish`：发布到 `publish\win-x64\LunaticPrincess.exe` 并打包 `publish\LunaticPrincess-win-x64.zip`。
  - 发布前先 `Get-Process LunaticPrincess | Stop-Process -Force`。
  - `publish\win-x64\data`（登录态 `session.dat`、`settings.json`）**不要删除**。
- 手动执行 `dotnet` 命令时，把 `DOTNET_CLI_HOME` / `TMP` / `TEMP` / `NUGET_*` 指到工作区目录，并使用仓库内的 `.\.dotnet\dotnet.exe`。

## 代码约定

- 能力先落在 Core（带单元测试），Desktop 只做编排与展示。
- 用户可见文案一律中文；异常统一经 `ErrorFormatter` 转成中文说明与处理建议，不把英文异常直接抛给用户。
- 提交信息用英文祈使句、一次提交一件事；提交前跑 `.\build.ps1 -Test`，工作区不要留下临时脚本。

## 界面设计

- **界面设计一律以用户提供的设计参考为准**：用户在对话中给出的设计稿/截图就是视觉基准，改窗口的配色、圆角、间距、控件形态、排版前先对照它，不要自行发明或更换视觉语言。
- 设计稿统一存放在 `docs/design/`（命名如 `ui-main.png`），便于后续会话按文件名引用；新设计要求也追加到这里。
- 参考尚未覆盖到的细节，沿用现有基线：Salt UI 风格卡片 + 胶囊控件、紧凑列表行；液态玻璃主题（深色 / 浅色 / 跟随系统）；无边框自绘标题栏；中文文案与友好错误提示。
