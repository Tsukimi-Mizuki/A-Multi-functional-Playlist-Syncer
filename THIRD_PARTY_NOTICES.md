# 第三方组件与参考项目

本项目为个人学习 / 备份工具，未包含任何网易云音乐官方素材或音频文件。本文件列出 2.0（Rust + Tauri + React）版直接使用的第三方组件与许可证；
旧的 C# / Avalonia 版（`CSharp-GUI` 分支）用到的 NuGet 组件见该分支上的同名文件。

## 运行时依赖：Rust crate（`app/Cargo.toml`）

版本取自 `app/Cargo.lock`，许可证取自各 crate 的元数据（`cargo metadata`）。

| crate | 版本 | 许可证 | 用途 |
|---|---|---|---|
| tauri | 2.12.5 | Apache-2.0 OR MIT | 桌面外壳：窗口、IPC、WebView2 集成、窗口材质（亚克力 / 云母） |
| tauri-plugin-dialog | 2.8.4 | Apache-2.0 OR MIT | 系统文件 / 文件夹选择对话框 |
| tauri-build（构建期） | 2.7.4 | Apache-2.0 OR MIT | 生成 Tauri 上下文与 Windows 资源 |
| tokio | 1.54.1 | MIT | 异步运行时、超时、定时器 |
| reqwest | 0.13.5 | MIT OR Apache-2.0 | HTTP（TLS 走 Windows Schannel，使用系统证书与代理） |
| serde / serde_json | 1.0.229 / 1.0.154 | MIT OR Apache-2.0 | 序列化与 JSON |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | 错误类型 |
| aes / cbc | 0.9.3 / 0.2.1 | MIT OR Apache-2.0 | weapi 的 AES-128-CBC、NCM 与 eapi 的 AES-128-ECB |
| md-5 | 0.10.6 | MIT OR Apache-2.0 | eapi（网易云搜索）请求摘要的 MD5 |
| num-bigint | 0.5.1 | MIT OR Apache-2.0 | weapi 的无填充 RSA 模幂 |
| base64 | 0.23.1 | MIT OR Apache-2.0 | Base64 编解码 |
| rand | 0.10.3 | MIT OR Apache-2.0 | 随机 secret |
| qrcode | 0.14.1 | MIT OR Apache-2.0 | 登录二维码（SVG） |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | QRC / KRC 的 zlib 解压（纯 Rust 后端 miniz_oxide） |
| regex | 1.13.1 | MIT OR Apache-2.0 | LRC / QRC / KRC 解析 |
| lofty | 0.25.5 | MIT OR Apache-2.0 | 音频标签、封面、歌词读写 |
| icu_normalizer / icu_properties | 2.4.0 | Unicode-3.0 | NFKC 归一化与 Unicode 字符类别（ICU4X，按 .NET 语义匹配歌曲） |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | DPAPI 登录态加密、已知文件夹、进程令牌查询 |

传递依赖在解析图里共约 315 个 crate，绝大多数是 MIT / Apache-2.0 / Unicode-3.0 / BSD / ISC / Zlib 等宽松许可证；
其中 `cssparser`、`cssparser-macros`、`selectors`、`dtoa-short`、`option-ext` 是 MPL-2.0（文件级 copyleft，本项目没有修改它们，源码在 crates.io 上）。
完整清单可用 `cargo metadata --format-version 1`（在 `app/` 下）自行生成。

## 运行时依赖：前端 npm 包（`app/package.json`）

| 包 | 版本 | 许可证 | 用途 |
|---|---|---|---|
| react / react-dom（及其依赖 scheduler） | 19.3.x | MIT | 界面 |
| @tauri-apps/api | 2.12.4 | Apache-2.0 OR MIT | 前端调用后端命令与事件 |
| @tauri-apps/plugin-dialog | 2.8.4 | MIT OR Apache-2.0 | 前端调用系统对话框 |

没有引入 UI 组件库；样式是手写 CSS。

## 构建期工具（不随程序分发）

| 工具 | 许可证 |
|---|---|
| @tauri-apps/cli 2.12.4 | Apache-2.0 OR MIT |
| TypeScript 7.0 | Apache-2.0 |
| Vite 8.3（及其内置的 Rolldown、PostCSS 等） | MIT；其依赖 lightningcss 为 MPL-2.0 |
| @vitejs/plugin-react 6.1、@types/react / react-dom / node | MIT |
| NSIS 3.13（由 Tauri 打包器下载，用于生成安装包） | zlib/libpng 许可证（NSIS 的 zlib 版许可证），安装器外壳代码会进入生成的 setup.exe |

## 运行环境

- **Microsoft Edge WebView2 Runtime**：界面的渲染引擎，由 Microsoft 提供，使用其自己的许可条款；本项目不随程序分发它，安装包在缺失时会下载微软的引导程序。

## 算法与协议参考

NCM 解密与网易云 / QQ / 酷狗接口协议来自社区公开研究，代码为本项目独立实现（Rust 版是 C# 版的移植，并与 C# 版逐字节交叉验证）：

- taurusxin/ncmdump — NCM 文件格式与解密流程参考
- chenmozhijin/LDDC — 歌词下载 / 逐字歌词与 QQ QRC 解密（3DES 变体）、酷狗 KRC 的产品形态与实现参考
- Binaryify/NeteaseCloudMusicApi 及其社区继任者（NeteaseCloudMusicApi Enhanced） — weapi / eapi 加密与接口端点语义参考；网易云搜索的 eapi 请求格式与海外 IP 下改走 eapi 的做法另参考了 lyrics-search / Lyrics-Helper
- MusicTag 桌面端 / 音樂標籤 — 在线匹配打分、多变体检索与客户端 `163 key` 注释的思路参考

## 界面设计参考

- **Salt UI** — Apache License 2.0，https://github.com/Moriafly/SaltUI
  - 本项目参考其 RoundedColumn 卡片、胶囊按钮、紧凑 Item 与色彩层级；
  - Salt UI 是 Compose Multiplatform 库，本项目没有引入它的运行时，也没有复制 Kotlin 源文件，CSS 样式为按该设计语言重新实现。
- **Salt Player / Moriafly 官网** — 界面与产品形态参考，https://moriafly.com/

## 视觉 / 窗口效果参考

液态玻璃效果由网页自身的半透明层叠、滤镜与 CSS 变量实现，窗口的亚克力 / 云母材质经 Tauri 的窗口效果接口（内部使用 Windows DWM / `window-vibrancy`）铺在网页下面。未引入额外的主题包或模糊库。

请遵守上述项目各自的许可证以及网易云音乐用户协议、当地版权法律。
