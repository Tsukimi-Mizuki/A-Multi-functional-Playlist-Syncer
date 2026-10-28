# 实现状态

> 最后更新：2026-10-28。当前状态：**Avalonia 版功能完成；Tauri 版（Rust 壳 + Vue 前端 + .NET sidecar）已真机跑通全部核心流程**。

## 模块

| 模块 | 状态 | 说明 |
|---|---|---|
| 解决方案 / 契约 / 构建 | ✅ 完成 | .NET 8，Core/Desktop/Tests 三项目，Debug/Release 0 warning / 0 error |
| 扫码登录与 Cookie 持久化 | ✅ 完成 | type=3 主路 + type=1 降级；803 Cookie 合并；DPAPI 加密落盘；启动恢复登录态 |
| 歌单读取与曲目补齐 | ✅ 完成 | 我的歌单分页、大歌单 trackIds 按 500/批补齐、保持顺序 |
| 本地扫描与歌单差异匹配 | ✅ 完成 | 标签/文件名/NCM 元数据、网易云 ID 精确匹配、模糊匹配、一对一 |
| 音质选择下载 | ✅ 完成 | 九档：标准 / 极高 (HQ) / 无损 (SQ) / 高解析度无损 (Hi-Res) / 高清臻音 (SpatialAudio) / 沉浸环绕声 (Surround Audio) / 超清母带 (Master) / 臻音全景声 (Audio Vivid) / 杜比全景声 (Dolby Atmos)；映射 level 见 ARCHITECTURE.md，不可用时自动降档并提示实际音质；取链请求带桌面客户端 Cookie，Hi-Res 实际返回 24bit/48–96kHz FLAC；旧设置 "Higher" 迁移为 ExHigh；试听片段拒收 |
| NCM 解密 | ✅ 完成 | AES/RC4 变体流式解密，MP3/FLAC 识别，封面/元数据提取，原子输出；导入窗口可选解锁后自动在线匹配标签/封面/歌词 |
| 逐行/逐字歌词解析与写入 | ✅ 完成 | LRC/增强 LRC/YRC v0+v1、QQ QRC 逐字（3DES 变体解密）、翻译全局贪心对齐、时间戳精确到毫秒（译文与原文同戳）；多变体检索 + 可调匹配度阈值（0.30–1.00，默认 0.60）作用于编辑器/批量/NCM 自动匹配；普通/逐字写入标签 |
| 标签/封面自动写入 | ✅ 完成 | MP3 ID3v2、FLAC VorbisComment/METADATA_BLOCK_PICTURE、M4A 等容器；中文标签往返 |
| Avalonia GUI | ✅ 完成 | Salt UI 风格卡片/胶囊控件/紧凑 Item；液态玻璃主题（深色 / 浅色 / 跟随系统）；登录/歌单/歌曲表/本地状态/设置/NCM 导入/进度日志/独立歌词标签编辑器；无边框自绘标题栏（自绘窗口按钮、可拖动、双击最大化）。旧版界面保留，与新版并行开发 |
| Sidecar HTTP API | ✅ 完成 | `src/LunaticPrincess.Server`：26 条路由覆盖健康检查/会话/设置/歌单/曲库/下载/同步/NCM/标签/歌词/批量/文件/日志；长任务返回 202 + `jobId`，进度走 SSE；契约见 [TAURI_API.md](TAURI_API.md) |
| Tauri 桌面端（新） | 🚧 进行中 | Rust 壳在 `setup` 里手动建窗（`WebviewWindowBuilder` + 可覆盖的 WebView2 数据目录），随机端口拉起 sidecar，读 `LPP_SERVER_READY` 后注入 `window.__LPP_API_BASE__`；前端为 Vue 3 + shadcn-vue + Motion for Vue；已真机验证登录态/歌单/设置/NCM；构建入口 `scripts/build-desktop.ps1` |
| Windows 发布 | ✅ 完成 | 轻量框架依赖 win-x64 发布目录 + zip（可用 -SelfContained 生成独立包） |
| Android 复用 | 🧩 预留 | Core 与 UI/存储通过接口隔离，见 ARCHITECTURE.md |

## 验证记录

### 自动化测试
- `dotnet test`：**273 / 273 通过，0 skipped**。
  - Netease API：63
  - NCM / Lyrics / Tags：93
  - Library / Download / Sync：66
  - Storage / Settings / Filename：18
  - 音质映射（Models）：30
  - Online 搜索：3
- Debug 与 Release 全解决方案构建均 0 warning / 0 error。

### 真实网络冒烟（匿名，官方域名）
- 扫码出码 + 轮询 801：通过（确认段需本人手机扫码，自动化环境无法完成）。
- 用户歌单 uid=1（848 个，分页逻辑生效）。
- 热歌榜 playlist 3778678（200 首，trackIds 与 songs 一致）。
- 歌曲详情 186016（晴天/周杰伦/叶惠美/999000 maxbr）。
- 取链 33894312 ExHigh -> 320k mp3。
- 歌词 33894312：LRC 1032 字符 + YRC 6473 字符。
- 专辑 18875（跨时代/周杰伦）。
- **完整链路**：下载 33894312（约 15.8MB 320k mp3）→ 写中文标签/封面/逐字歌词 →
  TagLib 读回 title/artist/album/cover/lyrics/NeteaseId 全部正确。
- **真实歌曲歌词时间戳**：对 4 个真实文件（含 FLAC/MP3、日语/韩语/纯音乐）实网匹配并写入，
  输出全部为 3 位毫秒时间戳（`[mm:ss.fff]`，2 位为 0 条），双语行与原文行时间戳完全一致、
  无 10ms 偏移；逐字歌曲（网易云 YRC）写出 884 个 `<mm:ss.fff>` 词标签、同样无 2 位残留。

### Tauri 桌面端（真机，2026-10-28）
- 进程模型：`lunatic-princess.exe`（Rust/Tauri 2 + WebView2）随机端口拉起 `lunatic-princess-server.exe`（.NET 8 自包含），
  日志出现 `[lpp] sidecar ready at http://127.0.0.1:<port>`；关窗时 sidecar 一并退出。
- 真实账号（黑胶 VIP）验证：登录态注入成功，侧栏 16 个真实歌单，点开后 1233 首曲目渲染正常；
  设置对话框显示真实值（高解析度无损 (Hi-Res)、8 并发、跳过已存在）。
- NCM 页：自动扫描到 `D:\音乐\中转` 的 1 个真实 .ncm（ハレの日に - Harenohini，4:37），
  输入/输出目录从设置记忆恢复，来源优先级文案与实际设置一致。
- 界面瑕疵修复：WebView2 数据目录跨启动复用旧磁盘缓存时会出现演示数据（假歌单/普通用户），
  清缓存即正确；发布包不携带该目录。
- **便携目录（exe + sidecar + `WebView2Loader.dll`）拷到工作区外的普通目录后实机复测**：主界面 16 个歌单、
  设置与 NCM 页均正常；经 `POST /api/ncm/import` 把 `D:\音乐\中转` 里 53.5 MB 的真实 .ncm 解密到同一盘的
  新目录，返回 `成功 1 / 失败 0`，enrichmentMessage=`匹配「ハレの日に - Harenohini - 汐れいら」：已写入标签、封面、双语逐字歌词（QQ 音乐）`；
  解密出的 FLAC 56,120,666 B，读回标签 `title/artist/album/albumArtist/year=2025/track=1` 与封面全部正确，
  说明下载、解密、写标签在工作区外的真实路径上均无权限问题。
- 踩坑记录：便携目录最初只复制了两个 exe，缺 `WebView2Loader.dll` 时壳进程直接以 `0xC0000135`
  （STATUS_DLL_NOT_FOUND）退出且没有任何日志；`scripts/build-desktop.ps1` 现已把三个文件一起复制。
- 冷启动修复（同日复测通过）：全新便携目录首次启动时，前端会在 sidecar 地址注入前就查询会话，
  导致首屏停在登录页（`#/login`），手动刷新后即恢复。现 `desktop/src/lib/api.ts` 提供
  `whenApiBaseReady()`（等 `lpp-api-ready` 事件或 `initApiBase()` 完成，10 s 超时），
  `desktop/src/App.vue` 在首查前等待该地址、连接类错误按 300 ms×(n+1) 退避重试至 5 次，
  并在 `lpp-api-ready` 晚到时再补查一次会话。实测：带登录态的全新目录两次冷启动都直接进入主界面
  （16 个歌单、`Tsukimi_Mizuki 黑胶 VIP`）；无 `data\` 的目录仍正确停在扫码登录页。
- 构建脚本修坑：`$ErrorActionPreference='Stop'` 下 cargo 往 stderr 打 `Compiling ...` 会被 PowerShell 5.1
  当成 NativeCommandError 中断构建，现统一经 `Invoke-Native` 以 `Continue` 执行并按退出码判定；
  前端改为直接调用 `node_modules\vue-tsc\bin\vue-tsc.js --noEmit` 与 `node_modules\vite\bin\vite.js build`
  （`pnpm run build` 会触发依赖校验重装而在受限主机上失败）。

### Avalonia（旧版界面）
- 客户端真实启动成功、窗口响应、无 fatal 日志。
- 主界面截图：docs/screenshots/main-window.png。
- 对话框（扫码/设置/NCM 导入）已实现并编译通过。
- **真实扫码登录已验证**：扫码确认后成功加载账号、VIP 状态和歌单；完全退出并重启客户端后，
  从便携数据目录的 DPAPI 加密 session.dat 自动恢复登录，无需再次扫码。
- 歌单侧栏支持收起/展开；最左上角不再有独立菜单按钮，“我的歌单”文字被单个歌单图标按钮替代。
  收起后的 88px 封面栏纵向显示全部歌单封面，点击封面即可切换歌单；顶部展开箭头/Ctrl+B 恢复侧栏。
  已用 UI 自动化验证收起/展开两种布局。
- 设置中的文件名模板新增“标题 - 作者”预设，并提供作者-标题、标题-作者、含年份和自定义选项；
  选择预设会同步写入模板值，且输入框默认隐藏，只有选中「自定义」时才出现并允许编辑
  （已用 UI 自动化验证：预设时输入框不存在，切换到自定义后出现）。
- 帧率上限设置已从设置窗口隐藏（配置项 `AppSettings.RenderFrameRate` 保留并继续生效，
  默认垂直同步；`RenderTimerInstaller` 的垂直同步/节流分支未改动）。
- 二级窗口（设置、歌词与标签编辑器、批量获取、NCM 导入、扫码登录、提示框）统一使用液态玻璃：
  `ApplyDialog` 设置 AcrylicBlur，窗口底色为半透明 `DialogGlassBrush`（浅色为 #DCF2F5FA）；已用 UI 自动化读取
  `ActualTransparencyLevel` 确认设置、编辑器、批量获取、NCM 导入四个窗口均为 AcrylicBlur。
- 新增独立歌词/标签编辑器：可从主界面或 `--lyrics <file>` 打开，读取封面/全部常用标签/歌词，
  编辑后写回；已验证标签读取、歌词保存回读和 ReplaceAll 清空字段行为。
- 新增批量获取窗口：入口在歌词编辑窗口右上角，扫描目录、标签/封面与歌词分别使用设置中的
  来源优先级、标题/歌手/时长匹配、逐行/逐字双语歌词、覆盖已有、并发 3 和取消。
  已用测试目录验证：标签/封面来自 QQ 优先、歌词来自网易云优先，标签、封面和双语歌词均写回。
  窗口内显示当前生效的“匹配度阈值 xx%”，与歌词编辑窗口的滑块同源（在一次 GUI 实测中把滑块拖到
  90% 后，批量窗口提示随之变成“匹配度阈值 90%（在「歌词编辑」窗口调整，低于该值的歌曲会被跳过）”）。
- NCM 导入窗口的输入文件夹列表与输出目录自动记忆（默认启用、无开关）：打开窗口恢复上次内容并重新扫描，
  添加文件夹、开始解密或关闭窗口时写回 `settings.json`（`ncmImportFolders` / `ncmImportOutputDirectory`，
  自动去重去空白）；设置窗口保存其他设置时也会原样保留这两个字段。已用预置记忆的 settings.json
  启动验证：窗口打开即显示上次文件夹路径、输出目录正确恢复，关闭后设置文件仍保留记忆。
- NCM 导入窗口新增“解密后自动获取在线标签、封面与歌词”（默认开启）：每个文件解密成功后按
  设置的来源优先级自动在线匹配并写回标题、歌手、专辑、封面与双语歌词；解密/匹配失败都逐条展示。
  已用合成 .ncm + 真实网络端到端验证：解密 → 自动匹配「Lemon - 米津玄師」（网易云音乐）→
  TagLib 读回标题/歌手/专辑、封面与 2000 字符双语歌词全部成功。
- 评分与标签读取参考 MusicTag 桌面端 / 音樂標籤：标题的 “A / B”“A & B” 与歌手的
  “A/B”“A feat. B”“A · B”“CV:” 都按分隔符切段后取最佳组合，避免并列写法拖低相似度；
  本地文件注释里的客户端 `163 key(Don't modify)` 元数据可解出 `musicId`，网易云客户端
  下载/解密出来的文件也能按 ID 精确匹配（`NeteaseClientComment`，已单测覆盖）。
- QQ 音乐搜索修复（对照 LDDC 排查）：旧的 `client_search_cp` 已被 QQ 下线（`c.y.qq.com`、`c6.y.qq.com`
  都返回 HTTP 500），搜索必然抛网络异常，界面只显示“网络连接失败”。改用 `/soso/fcgi-bin/search_cp`
  + `new_json=1` 解析 `songname/songmid/albumname/albummid/songid/pubtime/interval/singer`，年份由
  `pubtime` 折算；`c.y.qq.com` 失败自动切 `c6.y.qq.com`，都失败才提示“QQ 音乐搜索接口不可用”。
  实网验证：编辑器里选 QQ 音乐搜索 “Lemon” 返回 Lemon / Lemon (Live) / Lemon (国语)，详情可取
  逐行 1235 字符、逐字（QRC）7923 字符、双语 2306 字符与 13.5KB 封面。
- 搜索准确率改造（参考 音樂標籤）：新增 `LyricsMatchScorer` 统一构造多变体查询并打分——
  “标题+歌手 → 标题 → 歌手+标题 → 标题主体 → 去 feat. 版本 → 并列第一段”最多 3 组查询、每源前 10 条候选；
  权重 标题 0.55 / 歌手 0.25 / 时长 0.20（无歌手时 标题 0.72 / 时长 0.20 / 专辑 0.08），
  歌手相似度 < 0.30 整体 ×0.75（同名不同人直接淘汰），候选为伴奏版本时直接拒绝。
  实网抽查 `D:\音乐` 中 12 个真实文件：11 个正确匹配（10 个 1.000、1 个 0.995）、0 个错配；
  原先会错配的「レッド・パージ！！！/+α あるふぁきゅん。」从 0.750 降到 0.563，被正确拒绝。
- 歌词编辑窗口新增“匹配度”滑块（30%–100%，默认 60%）：结果按总分从高到低排序并显示“匹配度 xx%”，
  悬停可看四路分值，低于阈值的结果标注“（低于阈值）”且不会被自动选中；滑块值写入
  `settings.json` 的 `matchThreshold`，同时作为批量匹配与 NCM 解锁后自动匹配的命中阈值。
  GUI 实测（文件 `+α あるふぁきゅん。 - ECHO.flac`，QQ 源）：3 条结果为 100% / 73% / 58%（低于阈值），
  自动选中 100% 那条；滑块拖到 90% 后 73% 那条立即变为“（低于阈值）”，搜索后 `matchThreshold` 落盘为 0.9。
- 注意：网易云搜索在多变体连续请求下会触发 `code=406` 限流（匿名与登录态均出现），
  QQ 源同日稳定可用；实测因此以 QQ 为主。
- 歌词编辑窗口的记忆：在线搜索来源（网易云 / QQ 音乐）与歌词格式（逐行 / 逐字优先）跟随上次选择，
  默认开启、无开关；保存在 `settings.json` 的 `lyricsEditorSearchSource` / `lyricsEditorPreferWordLyrics`，
  设置窗口保存其他设置时会原样带回这两个字段。已验证：预置 QQ + 逐字重启后窗口即显示 QQ 音乐 /
  逐字歌词（有则优先），改回网易云后 `settings.json` 立刻写成 `Netease`。
- 编辑器新增网易云音乐 / QQ 音乐联网搜索：点击搜索结果可同时载入标签、封面与歌词；
  歌词格式可选逐行或逐字（网易云 YRC、QQ QRC 逐字均可用），默认生成双语歌词（原文 + 翻译）。
  已验证两个来源的实际搜索结果、封面下载、两种歌词格式切换，以及日文歌曲原文 + 中文翻译的
  双语歌词（无翻译的歌曲自动回退原文）。QQ 翻译改为 `PlayLyricInfo` 接口，已用 Lemon /
  Shape of You 验证能获取翻译。打开本地音频时会以文件名自动搜索，已验证直接打开测试文件后
  自动填入关键词并返回 3 条结果。
- 移除了所有多主题/多材质逻辑与未使用的背景光斑、动画和颜色资源，只保留一套磨砂玻璃外观。
  主窗口使用 Windows AcrylicBlur，窗口后的内容变化会实时反映到磨砂背景上。
- 歌单侧栏加入 240ms 宽度过渡与淡入淡出动画；展开/收起列表用 `OpacityMask` 在上下边缘渐隐，图标渐隐/渐显；
  收起为 88px 封面栏，纵向展示全部歌单并可点击切换；
  按钮和列表项也增加了轻量颜色过渡。
- Salt UI 风格控件（卡片、胶囊按钮、紧凑 Item）保留；设置窗口同步改为卡片式布局。
- 主题：新增深色 / 浅色 / 跟随系统三套选择（默认深色），`App.axaml` 用 `ThemeDictionaries` 提供两套
  调色板，界面统一改为 `DynamicResource`；启动前应用 `settings.Theme` 避免闪烁，设置窗口下拉即时预览、
  取消回退、保存写入 `settings.json` 的 `theme`。GUI 实测（截图像素亮度均值，同一区域）：深色 30.2、
  浅色 201.4、从深色切回浅色 201.4；选择「跟随系统」保存为 `System`，取消预览后亮度回到 201.3
  且 `settings.json` 仍为 `Light`；删除设置值重启后按 `Light` 启动（190.2），歌词编辑器等二级窗口
  在浅色下正常渲染（233.5）。
- 浅色主题视觉修正（DPI 感知截图采样，物理像素）：① 删除歌单栏/封面栏上下共 4 条渐变色带
  （与列表 `OpacityMask` 重复，浅色下是比面板更亮的硬边白条）；② `WindowVeilBrush` 由深色
  #26020609 改为亮色 #CCF3F6FB、`DialogGlassBrush` 由 #B8F5F7FB 提到 #DCF2F5FA，面板/输入框
  同步提亮。修正后浅色主窗口间隔区 203、面板 217、内容卡片 250，设置窗口玻璃 241 与下拉 223–230、
  输入框 230、按钮 211 处于同一亮度层次（修前玻璃仅 185，与 233 的卡片形成暗灰边框式割裂）；
  深色主题数值不变（间隔 28、面板 88），未受影响。
- 顶栏/底栏背景色曾统一为不透明的 `ChromeBarBrush`（深色 #151A23、浅色 #FEFEFE），`CaptionBrush` 取同一个色值。
  该画刷后来被液态玻璃 `ChromeGlassBrush` 取代并删除。统一前半透明时三条栏随玻璃背景漂移：
  深色标题栏 #161C26(27.3) 对顶栏 #14191E(24.1) 对底栏 #131923(24.3)，浅色标题栏 #EFF3F8(242.4)、
  顶栏 #F1F3F6(242.7) 对底栏 #FCFCFD(252.1)，肉眼是两层色；统一后发布版实测深色三条栏均为
  #151A23、浅色均为 #FEFEFE，完全一致，且不再随壁纸变化。
- 标题栏不再与窗口割裂：早先 `DwmCaptionColor` 填 `AppBackgroundBrush` 时，深色标题栏比正文暗
  24 个亮度单位（14.4 对 38），像贴了一条黑条；改成 `DWMWA_COLOR_NONE` 让系统 Acrylic 画标题栏
  又按**系统**主题着色（深色 84、浅色 152，客户区 27–38 与 241），两个方向都更割裂。
  最终由 `CaptionBrush` 直接取与三条栏相同的实色（见上一条），DWM 标题栏与顶栏/底栏像素级一致；
  运行时切换主题（设置里预览/取消）标题栏同步更新，已实测深色 ↔ 浅色双向切换。
- 主窗口改为无边框自绘标题栏（`ExtendClientAreaToDecorationsHint` + `NoChrome`，保留 `CanResize`）：
  68px 顶栏同时是标题栏，按下只记录位置、移动超过 4 DIP 才 `BeginMoveDrag`（直接按下即拖动会
  进入系统移动循环并吞掉第二次按下，双击永远失效），双击在按下事件里切换最大化/还原；三个自绘
  按钮的自动化名称为「最小化窗口 / 最大化窗口 / 还原窗口 / 关闭窗口」，最大化时图标切成双框。
  发布版 DPI 感知截图：深色窗口顶部 y+0..3 与栏中均为 #151A23、浅色均为 #FEFEFE，无 DWM 亮线
  也没有壁纸透出；最大化 rect -9,-9 / 2578x1398（工作区 0,0 / 2560x1380）时屏幕 y=0..3 仍为
  #151A23（根 Grid 的 `OffScreenMargin` 生效）；最大化/还原/最小化用 UIA Invoke 验证，双击用
  PostMessage 投递 WM_LBUTTONDOWN/UP 验证；DWM 1px 边框以 `DWMWA_COLOR_NONE (0xFFFFFFFE)` 关闭。
- 顶栏改为液态玻璃：新增 `ChromeGlassBrush`（深色 #A6151A23 即 65%、浅色 #D0FEFEFE 即 82%），
  68px 顶栏（项目名称 + 账号信息 + 窗口按钮）改用它，Acrylic 模糊从这一栏透出、颜色随桌面内容轻微变化；
  两套主题字典都定义了该键，运行期用 `PrintWindow` 抓窗口内容确认解析成功（深色栏渲染为 #1F232A 一档、
  浅色为 #F3F3F4，并随水平位置轻微波动）。当时桌面被全屏游戏覆盖，真实屏幕采样不可用，故“透出模糊背景”
  只做了结构验证与两主题解析验证，未做逐像素对比。随后按需求把底部「进度 / 日志」栏也换成同一个
  `ChromeGlassBrush`，并删掉不再被引用的 `ChromeBarBrush`（`CaptionBrush` 保留纯色供 DWM 使用）。
- 发布改为轻量框架依赖模式：发布目录约 29MB、zip 约 11.7MB；
  需要独立运行时可用 -SelfContained 切换回自包含模式；发布目录约 27MB、zip 约 11MB。

### NCM
- 使用合成 NCM fixture（对算法做逆变换生成）验证密钥盒、元数据、封面、MP3/FLAC 识别、
  32KB 分块边界、截断/坏数据、取消/原子输出。
- 未使用真实账号下载的 .ncm 文件（版权原因，未随附样本）。

## 已修复的集成问题

- **受限运行环境下“没有访问权限”**：客户端进程若无法写入 `%LOCALAPPDATA%` 或“音乐”目录，
  登录 Cookie 保存会抛出 `UnauthorizedAccessException`。现已增加启动时目录可写性探测与自动降级：
  标准 AppData → 程序目录 `data` → 临时目录；“音乐”目录不可写时下载目录同步降级到 `data\Music`，
  界面日志会显示当前数据目录。真实扫码登录后会话已成功保存；完全退出并重启客户端后
  自动恢复账号和歌单，已验证。便携模式会固定使用程序目录 `data`；发布 zip 会排除该目录，
  避免把用户的加密登录 Cookie 打进安装包。

## 已知限制

1. 扫码登录的 803 确认段需要真实手机扫码；自动化只能验证出码与轮询，无法验证最终 Cookie 获取。
2. 无损/Hi-Res 取决于账号会员权益；无权限时会降级或明确失败，程序不绕过版权限制。
3. 逐字歌词取决于网易云是否为该曲目提供 YRC、QQ 音乐是否提供可解密的 QRC；都没有时回退逐行 LRC。
4. FLAC 封面按 VorbisComment METADATA_BLOCK_PICTURE 写入（TagLibSharp 2.3.0 不支持原生 PICTURE block API），主流播放器兼容。
5. 网易云接口为非公开接口，未来可能变更；相关端点集中在 Core/Netease 便于维护。
6. 客户端暂未加入本地播放功能；当前定位为“歌单同步 / 下载 / 解密 / 标签管理”。
7. Tauri 版依赖 **WebView2 Runtime**；便携目录必须同时放 `lunatic-princess.exe`、`lunatic-princess-server.exe`、
   `WebView2Loader.dll` 三个文件（缺 DLL 会以 0xC0000135 立即退出）。且每次启动使用系统分配的空闲端口，不要写死端口或做端口转发。
8. Tauri 的 NSIS 安装包需要 `tauri-cli`（`build-desktop.ps1 -Bundle`）；默认产物是可便携目录 + zip。
9. 本仓库路径含中文时，mingw `windres` 无法读取图标路径，构建脚本会把图标暂存到 `D:\lpp-icons`
   再通过 `TAURI_CONFIG` 覆盖（链路见 ARCHITECTURE.md）。
