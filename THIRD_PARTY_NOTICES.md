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

请遵守上述项目各自的许可证以及网易云音乐用户协议、当地版权法律。
