[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
![Rust](https://img.shields.io/badge/Rust-1.99-DEA584?logo=rust)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react)
![TypeScript](https://img.shields.io/badge/TypeScript-7-3178C6?logo=typescript)
![Windows](https://img.shields.io/badge/Platform-Windows_10%2F11-0078D4?logo=windows)
![GitHub release (latest by date)](https://img.shields.io/github/v/release/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer)
![CI](https://img.shields.io/github/actions/workflow/status/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/ci.yml?label=CI)
![GitHub last commit](https://img.shields.io/github/last-commit/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer)
![GitHub repo size](https://img.shields.io/github/repo-size/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer)


# Lunatic Princess

一个面向 Windows 的网易云音乐桌面工具：扫码登录后长期保持登录态，读取账号歌单，扫描本地曲库，
自动对比歌单与本地文件差异，按选定音质下载缺失歌曲，解密本地 NCM 文件，并把标题、歌手、专辑、
封面与逐行/逐字歌词写入音乐标签；还能用网易云、QQ 音乐、酷狗、LRCLIB 在线匹配歌词与标签。

2.0 起整个程序用 **Rust + Tauri 2 + React** 重写（界面为液态玻璃风格，深色 / 浅色 / 跟随系统）。
原来的 C# / Avalonia 版（v1.x）保留在 [`CSharp-GUI`](../../tree/CSharp-GUI) 分支，只作历史与维护用途。

> 仅用于个人备份自己有权访问的音乐。请遵守网易云音乐用户协议与当地版权法律，不要分发下载内容。
> 程序不绕过会员与版权限制：无权限的音质会降档或失败，试听片段一律判为失败。

## 功能

- **扫码登录**：网易云 App 扫码，登录态用 Windows DPAPI 加密保存，自动合并服务端下发的 Cookie 并定期续期。
- **歌单浏览**：账号全部歌单，大歌单分批补全，歌曲表格虚拟滚动（上千首也流畅）。
- **本地曲库对比**：扫描本地目录（MP3 / FLAC / M4A / OGG / OPUS / WAV / NCM），优先按网易云 ID 精确匹配，其次标题 / 歌手 / 时长评分，标出「已存在 / NCM 待解密 / 需下载」。
- **按音质下载与一键同步**：九档音质（标准到杜比全景声），服务端降档时提示实际音质；一键同步按本地状态跳过、解密 NCM 或下载。
- **NCM 解密**：流式解密 `.ncm`，回写内嵌元数据与封面，可选解密后自动在线补全标签、封面与歌词。
- **歌词与标签**：LRC / 增强 LRC / 网易云 YRC / QQ QRC / 酷狗 KRC 逐字歌词，双语歌词，封面与标签读写；歌词与标签编辑器、批量获取窗口，支持把文件或文件夹拖进窗口。
- **外观**：液态玻璃主题、窗口材质（亚克力 / 云母 / 无）、玻璃浓度、减少动效。

## 下载与安装

在 [Releases](../../releases) 页面下载（2.0.0 正式发布之前，Releases 里最新的仍是 C# 版 v1.x，想用新版请先自行构建，见下文）：

| 文件 | 说明 |
|---|---|
| `LunaticPrincess-<版本>-win-x64-setup.exe` | NSIS 安装包，安装到当前用户目录（不需要管理员权限），中文 / 英文安装界面 |
| `LunaticPrincess-<版本>-win-x64-portable.zip` | 便携版，解压后只有一个 `LunaticPrincess.exe`，不含任何用户数据 |
| `SHA256SUMS.txt` | 上面两个文件的 SHA-256 校验值 |

**WebView2 运行时**：界面由 Microsoft Edge WebView2 渲染。Windows 11 和绝大多数已更新的 Windows 10 已自带；
缺少时安装包会联网下载安装引导程序，便携版则需要你先手动安装
[WebView2 Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)。

**数据位置**：默认保存在 `%LOCALAPPDATA%\LunaticPrincess`（设置 `settings.json`、加密的登录态 `session.dat`、`webview` 缓存）。
想做成真正的便携版：在 `LunaticPrincess.exe` 旁边新建一个名为 `data` 的文件夹，程序检测到它就改用它保存全部数据。
也可以用环境变量 `LUNATIC_PRINCESS_DATA_DIR` 指定目录。`session.dat` 绑定当前 Windows 用户，换用户或换电脑需要重新扫码。

从 C# 版升级：`settings.json` 与 `session.dat` 格式互相兼容，数据目录相同时无需重新登录。

## 构建与运行

前置条件：Windows 10/11 x64、PowerShell 5.1+、[Rust](https://rustup.rs/) stable（MSVC 工具链，需要 Visual Studio 2022 生成工具的 C++ 组件）、
Node.js 与 [pnpm](https://pnpm.io/) 11、WebView2 运行时。

```powershell
# 安装前端依赖并构建发布版 exe：app\target\release\LunaticPrincess.exe
.\build.ps1

# 只跑检查与测试：cargo fmt --check、clippy -D warnings、cargo test、pnpm build
.\build.ps1 -Test

# 生成 NSIS 安装包 + 便携版 zip + 校验值，输出到 dist-release\
.\build.ps1 -Bundle
# 指定版本号（会临时写入各清单，构建完恢复）：v2.1.0、v1.3、v.1.0.0 都能识别
.\build.ps1 -Bundle -Version v2.1.0
```

开发时（在 `app\` 目录下）：

```powershell
pnpm install
pnpm tauri dev      # 带热更新的桌面窗口
pnpm dev            # 只启动前端，在普通浏览器里用假后端预览（?mock=... 见 AGENTS.md）
cargo test --workspace
```

首次打包会联网下载 NSIS 工具。仓库如果放在带低完整性标签的目录（例如 Edge 的默认下载文件夹），运行行为与测试
有些特殊，详见 [docs/TAURI_REWRITE.md](docs/TAURI_REWRITE.md) 末尾的说明；`build.ps1` 已经把临时目录指到工作区。

## 目录结构

```
app/
  Cargo.toml                 # Cargo 工作区：crates/lp-core + src-tauri
  crates/lp-core/            # 平台无关业务库（登录、歌单、歌词、标签、NCM、曲库、下载、同步、在线匹配），带单测
  src-tauri/                 # Tauri 外壳：命令、事件、窗口材质、打包配置、图标
  src/                       # React + TypeScript 前端（components / state / api / styles）
build.ps1                    # 构建 / 测试 / 打包入口
scripts/resolve-version.ps1  # tag -> SemVer（CI 与 build.ps1 共用）
docs/                        # 架构、实现状态、重写记录、界面设计参考
.github/workflows/           # ci.yml（检查与测试）、main.yml（tag 触发的打包与 Release）
```

## 已知限制

- 只面向 Windows 10/11 x64；没有本地播放功能，定位是「歌单同步 / 下载 / 解密 / 标签管理」。
- 在线匹配的词源是网易云（匿名 eapi 搜索，不需要登录）与 QQ 音乐，酷狗与 LRCLIB 作为歌词备用源；这些都是非公开接口，随时可能变更。
- `/weapi/login/token/refresh` 实测返回 `code=400`，续期是否真的延长了登录有效期还需要长期观察；登录态真正过期时需要重新扫码。
- 无损 / Hi-Res 以上音质受会员权益限制，无权限时服务端降档或失败，程序不绕过。
- 不支持 WMA 标签读写；管理员身份运行时 Windows 会阻断资源管理器向程序拖放文件（系统限制，程序只能提示）。
- 界面没有自动化测试，业务逻辑测试集中在 `lp-core`。

完整清单与已确认待修的问题见 [docs/IMPLEMENTATION_STATUS.md](docs/IMPLEMENTATION_STATUS.md) 和 [docs/TAURI_REWRITE.md](docs/TAURI_REWRITE.md)。
协作约定与交接说明见 [AGENTS.md](AGENTS.md)，第三方组件见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
