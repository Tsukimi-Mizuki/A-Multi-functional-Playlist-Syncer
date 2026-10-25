# 第三方组件与参考项目

本项目为个人学习/备份工具，未包含任何网易云音乐官方素材或音频文件。

## 运行时依赖（NuGet）

- **Avalonia** — MIT License，https://avaloniaui.net/
- **CommunityToolkit.Mvvm** — MIT License
- **QRCoder** — MIT License
- **TagLibSharp** — LGPL-2.1 / MIT（taglib-sharp 移植版），https://github.com/mono/taglib-sharp
- **System.Security.Cryptography.ProtectedData** — MIT License

## 算法与协议参考

NCM 解密与网易云接口协议来自社区公开研究，代码为本项目独立 C# 实现：

- taurusxin/ncmdump — NCM 文件格式与解密流程参考
- chenmozhijin/LDDC — 网易云音乐下载/NCM 处理产品形态参考
- Binaryify/NeteaseCloudMusicApi 及其社区继任者 — weapi 加密与接口端点语义参考
- 社区调研文档（.research/netease-integration-research.md，开发期参考，不参与发布）

## 界面设计参考

- **Salt UI** — Apache License 2.0，https://github.com/Moriafly/SaltUI
  - 本项目参考其 RoundedColumn 卡片、胶囊按钮、紧凑 Item 与色彩层级；
  - Salt UI 是 Compose Multiplatform 库，本项目未引入其运行时，也未复制 Kotlin 源文件；
    Avalonia 控件样式为按该设计语言重新实现。
- **Salt Player / Moriafly 官网** — 界面与产品形态参考，https://moriafly.com/

## 视觉/窗口效果参考

本项目未引入额外 UI 主题包，磨砂玻璃效果由 Avalonia 自带透明度能力与 Windows DWM 实现：

- Avalonia 官方文档 — Windows AcrylicBlur / TransparencyLevelHint
  （https://github.com/AvaloniaUI/avalonia-docs）

请遵守上述项目各自的许可证以及网易云音乐用户协议、当地版权法律。
