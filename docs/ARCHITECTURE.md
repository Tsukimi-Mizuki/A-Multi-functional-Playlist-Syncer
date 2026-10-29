# 架构设计

## 目标

1. 桌面端（Windows/Avalonia）先把完整业务跑通。
2. 业务逻辑集中在 `LunaticPrincess.Core`，不依赖 Avalonia 或 Windows UI。
3. 为 Android 复用预留边界：网络、存储、设置、选择器均通过接口注入。

## 分层

```
Desktop (Avalonia + MVVM)
    │ 依赖接口，不依赖具体实现细节
    ▼
Core
  ├─ Netease (INeteaseApiClient)
  ├─ Ncm     (INcmDecoder)
  ├─ Lyrics  (ILyricParser)
  ├─ Tags    (IAudioTagService)
  ├─ Library (ILocalLibraryService / ISongMatcher)
  ├─ Download(IDownloadService / INcmImportService)
  ├─ Sync    (ISyncService)
  └─ Storage/Settings (ISessionStore / ISettingsStore)
```

组合根位于 `src/LunaticPrincess.Desktop/Services/AppServices.cs`，负责创建 HttpClient、
各 Core 服务实例以及登录态/设置存储；ViewModel 只接收接口。

## 关键流程

### 扫码登录

1. `POST interface.music.163.com/api/login/qrcode/unikey`（type=3）获取 unikey。
2. 将 `https://music.163.com/login?codekey=<unikey>` 渲染为二维码。
3. 每约 2 秒轮询 `/api/login/qrcode/client/login`：801 等待、802 已扫待确认、803 成功、800 过期。
4. 803 时从 Set-Cookie 与响应 cookie 字段收集 Cookie，构造 `NeteaseSession`。
5. 使用 DPAPI 加密写入 session.dat；启动时恢复并通过 `/weapi/nuser/account/get` 校验。

### 歌单与歌曲

- weapi 请求需 AES-CBC 双重加密 + 无填充 RSA 加密的 `encSecKey`。
- 大歌单先取 `trackIds`，再按 500 首/批调用 `/weapi/v3/song/detail` 补齐，保持原顺序。

### 本地对比

1. 扫描目录：普通音频读 TagLib 标签，NCM 读文件内嵌加密元数据。
2. 匹配优先级：标签内的网易云 ID > 标题/歌手/时长模糊评分 > 文件名猜测。
   网易云 ID 有两个来源：本程序写入的 `LunaticPrincess:<id>` 注释，以及客户端文件注释里的
   `163 key(Don't modify):<base64>`（AES-128-ECB 解出 `music:{...}` 后取 `musicId`，
   见 `NeteaseClientComment`；思路同 MusicTag / 音樂標籤），因此客户端下载或解密出来的
   文件也能按 ID 精确命中。
3. 每首本地曲最多匹配一首歌单曲；低于阈值标记为缺失。

### 下载与标签

1. 音质档位映射为网易云 level：标准 `standard`、极高 `exhigh`、无损 `lossless`、Hi-Res `hires`、高清臻音 `jyeffect`、沉浸环绕声 `sky`、超清母带 `jymaster`、臻音全景声 `audio_vivid`（回退别名 `vivid`）、杜比全景声 `dolby`；`/api/song/enhance/player/url/v1` 逐档请求，
   服务端无对应特权时静默降档，`AudioQualityExtensions.FromApiLevel()` 解析实际 level 并在进度中提示
   “实际音质：…（… 不可用，已自动降级）”。设置文件里的旧值 `Higher` 在加载时迁移为 `ExHigh`。
   取链请求额外附带桌面客户端 Cookie（`os=pc; appver=2.10.6; osver=…; deviceId=LunaticPrincessPc; channel=netease`，
   见 `NeteaseApiClient.BuildPlaybackCookieHeader`，只有播放/下载接口使用）：缺少这些字段时服务端按网页端策略
   把 Hi-Res 及以上档位静默标成 `lossless`（文件实际仍是 24bit，只是 level 被降级）。
2. 受会员/版权限制：取链失败或仅试听时明确失败，不绕过版权限制。
3. 下载到临时文件；若文件头是 NCM 则自动解密。
4. 拉取歌词、翻译、封面、歌曲/专辑元数据。
5. 按歌词模式选择普通 LRC 或增强 LRC（逐字）。
6. TagLib 写入 MP3/FLAC/M4A/OGG 标签，原子移动到下载目录。

### NCM 解密

- AES-128-ECB 解出 RC4 密钥盒与元数据；按 32 KB 分块流式 XOR 解密音频。
- 输出格式由首块内容判断（ID3 => mp3，fLaC => flac）。
- `NcmImportService` 解密成功后先写回内嵌元数据/封面/网易云 ID；导入窗口开启
  “解密后自动获取在线标签、封面与歌词”（默认开启，对应 `NcmImportOptions.AutoMatchOnline`）时，
  再逐个调用 `IBatchLyricsService.FetchForFileAsync` 按设置中的来源优先级在线匹配，
  写回标题/歌手/专辑/封面与双语歌词；匹配失败不影响解密结果，逐文件状态通过
  `NcmDecryptResult.EnrichmentMessage` 展示在导入窗口。
- 导入窗口的输入文件夹列表与输出目录默认自动记忆（无开关）：打开时从 `AppSettings.NcmImportFolders` /
  `NcmImportOutputDirectory` 恢复并重新扫描；添加文件夹、开始解密和关闭窗口时写回设置文件，
  输出目录为空时回退到下载目录；设置窗口保存时原样携带这两个字段。

## 外观与窗口效果（液态玻璃，深色 / 浅色）

- 两套配色共享同一套**液态磨砂玻璃**材质，基于 Salt UI 的卡片/胶囊按钮/紧凑 Item 结构。
  `App.axaml` 的 `Application.Resources` 用 `ResourceDictionary.ThemeDictionaries` 提供 `Default`（深色）
  与 `Light`（浅色）两个字典；界面里的调色板一律用 `{DynamicResource *Brush}` 引用（全 Desktop 共
  170 处替换），主题切换时自动重绘；转换器类资源必须继续用 `StaticResource`。
- 主题由 `ThemeService.Apply(ThemeMode)` 统一切换：设置 `Application.RequestedThemeVariant`
  （`Dark` / `Light` / `Default` 即跟随系统），刷新 `ThemeBrushes`，并在下一帧
  （`DispatcherPriority.Loaded`）再次刷新窗口标题栏。`ThemeBrushes` 持有 7 个可变 `SolidColorBrush`
  给状态标签、日志级别等转换器复用——Binding 不会因主题切换重新求值，只能改实例 `Color` 触发重绘。
- 启动时 `App.axaml.cs` 在创建主窗口前同步读取 `settings.Theme` 并应用，避免闪一下深色；
  设置窗口「外观」下拉即时预览，点取消调用 `RevertThemePreview()` 回退，点保存把
  `theme`（`Dark` / `Light` / `System`，默认 `Dark`，未知值归一为 `Dark`）写入 `settings.json`。
- 主窗口设置 `WindowTransparencyLevel.AcrylicBlur`（回退到 Blur），
  `Background = Transparent`，因此窗口后面的桌面、网页、视频和游戏会被 Windows 实时模糊透出，
  背景内容变化时窗口材质同步变化，不使用静态壁纸或内部模拟光斑。
- 半透明调色板集中在 `App.axaml`：Header/Panel 为 60%–80% 明度遮罩，
  内容区增加一层轻量衬底保证表格可读（深色用暗色 `ContentScrimBrush`，浅色用极淡的白色蒙版）；
  浅色主题的 `WindowVeilBrush` 是**亮色**（#CCF3F6FB）而不是深色：Acrylic 会把桌面（常见深色壁纸）
  模糊进来，若再叠深色遮罩，近白卡片与灰蓝背景之间会出现明显“割裂感”，改用高不透明度亮色遮罩后
  主窗口间隔区约 203、面板约 217、内容卡片约 250，层次连续；
  窗口深浅色与 DWM 客户区标题栏颜色仍由 `VisualThemeManager.Refresh` 写 `DwmUseImmersiveDarkMode`、
  `DwmCaptionColor`、`DwmTextColor`。
  顶栏（项目名称 + 账号信息 + 窗口按钮，即自绘标题栏）用 `ChromeGlassBrush` 做成**液态玻璃**：
  深色 #A6151A23（65% 不透明）、浅色 #D0FEFEFE（82% 不透明），Acrylic 模糊会从这一栏透出来，
  栏的颜色随背后的桌面内容轻微变化，与面板/卡片使用同一套材质语汇。
  底部「进度 / 日志」区早期用 `ChromeGlassBrush` 贴边铺满内容卡片（方形直角 + 顶部 1px 分割线），
  与圆角玻璃卡片语汇不一致；现改为内容卡片内的一层**圆角玻璃面板**（`PanelAltBrush` + `BorderBrush`
  1px + `CornerRadius="8"`，四周留 20/12/20/14 外边距），与 NCM 导入窗口的结果面板等同一种材质。
  代价是文字会随壁纸明暗轻微漂移（曾实测不透明时深色顶栏与底栏会差 3~5 个亮度单位，浅色 242.7 对 252.1，
  统一成同一种不透明色可消除），这里按界面观感取舍；`CaptionBrush` 保留同色纯色值，供 DWM 属性使用
  （无边框窗口下实际不绘制）。
  曾试过把 `DwmCaptionColor` 设成 `DWMWA_COLOR_NONE` 让系统 Acrylic 材质来画标题栏（更早还试过让标题栏取
  `AppBackgroundBrush`，深色下比客户区暗 24 个亮度单位），
  但系统材质按**系统**主题着色，与 App 主题无关：深色下标题栏 84（客户区 27–38，偏亮），
  浅色下 152（客户区 241，明显发灰），反而更割裂，因此保留校准过的纯色方案。
- 主窗口使用**无边框自绘标题栏**（`ExtendClientAreaToDecorationsHint="True"`、
  `ExtendClientAreaChromeHints="NoChrome"`、`ExtendClientAreaTitleBarHeightHint="-1"`，
  保留 `CanResize` 以继续拥有 `WS_THICKFRAME`，缩放/贴边不受影响）：
  - 根 `Grid` 的 `Margin="{Binding $parent[Window].OffScreenMargin}"` 让内容避开 Win11 最大化时
    伸到屏幕外的边框：实测最大化窗口 rect 为 -9,-9 / 2578x1398（工作区 0,0 / 2560x1380），
    屏幕 y=0..3 的最上面几行像素仍是 #151A23，内容没有被推出屏幕。
  - 68px 顶栏本身就是标题栏（`OnTitleBarPointerPressed/Moved/Released/CaptureLost`）：
    按下时只记录位置，指针移动超过 4 DIP 才调用 `BeginMoveDrag`——按下即拖动会进入系统移动
    循环并吞掉随后的第二次按下，`ClickCount == 2` 永远收不到；双击改在按下事件里调用
    `ToggleWindowState()`（实测普通 322,12 1650x1075 → 最大化 -9,-9 2578x1398 → 双击还原）。
  - 右侧三个 `Classes="caption"` 按钮（46x32、直角、透明底、10x10 `Path` 图标）替代系统按钮，
    `AutomationProperties.Name` 为「最小化窗口 / 最大化窗口 / 关闭窗口」，最大化后图标切成双框、
    名称变「还原窗口」；hover/pressed 用 `CaptionButtonHoverBrush` / `CaptionButtonPressedBrush`，
    关闭按钮 hover 为 `CaptionCloseHoverBrush`（#E81123）红底白叉。
  - `VisualThemeManager.Refresh` 对启用 `ExtendClientAreaToDecorationsHint` 的窗口写
    `DwmBorderColor = DWMWA_COLOR_NONE (0xFFFFFFFE)`，否则 DWM 会在自绘标题栏外沿画一圈
    #3C3C3C 的 1px 浅灰边框（深色主题下像贴了道亮线）。
- 所有二级窗口（设置、歌词与标签编辑器、批量获取、NCM 导入、扫码登录、提示框）走同一套材质：
  `VisualThemeManager.ApplyDialog` 设置 `WindowTransparencyLevel.AcrylicBlur`（回退 Blur/None），
  窗口 `Background` 由 XAML 的 `DialogGlassBrush`（深色 #A8080D15 / 浅色 #DCF2F5FA，半透明）提供；代码侧不再覆盖
  窗口背景，否则会盖掉模糊层。已用 UI 自动化读取 `ActualTransparencyLevel`，确认设置窗口、
  歌词编辑器、批量获取窗口和 NCM 导入窗口实际都是 AcrylicBlur。
- 主窗口里原本硬编码的渐变（压暗 veil、表格衬底）改为
  `WindowVeilBrush` / `ContentScrimBrush`，两套主题各一份；
  歌单栏与歌曲列表顶部/底部曾经各叠了一条渐变色带（`PanelFadeDownBrush` / `PanelFadeUpBrush`），
  与列表自身的 `OpacityMask` 渐隐功能重复：深色下像柔和阴影，浅色下则是比面板更亮的硬边白条
  （面板 #A8FFFFFF 之上再叠 #E6F8FAFD），因此删除这 4 条 overlay 与对应画刷，只保留与主题无关的
  `#00FFFFFF` 透明度遮罩（OpacityMask）。
- 二级窗口在 `Opened` 后由 `VisualThemeManager.ScheduleClamp` 夹回屏幕工作区：
  `CenterOwner` 定位在高 DPI/小屏下可能把设置窗口底部推出屏幕，超出部分按工作区缩放后再居中。
- 侧栏通过 `Width` 过渡动画从 272px 收缩到 88px，同时交叉淡化列表内容与封面栏；
  原“我的歌单”文字替换为单个歌单图标按钮，最左上角不再有独立菜单按钮。收起后的封面栏
  使用第二个 ListBox 绑定同一 `Playlists/SelectedPlaylist`，纵向显示所有歌单封面；点击封面
  即切换当前歌单并触发加载。展开和收起状态的列表都用 `OpacityMask` 在上下边缘渐隐，图标
  滚入/滚出与状态切换时淡入淡出；顶部展开箭头和 Ctrl+B 恢复完整侧栏。
- 受限环境（远程桌面、关闭透明效果、无 DWM）下 Acrylic 不可用时窗口退回当前主题的不透明玻璃背景。
- 设置窗口不再显示「界面与性能 → 帧率上限」，`AppSettings.RenderFrameRate` 继续保留并生效
  （默认垂直同步，已有配置原样沿用）。帧率上限由 `RenderTimerInstaller` 在 Avalonia 初始化后接管 Win32 渲染计时器：
  - WinUI Composition 后端本身由 DWM 垂直同步驱动，垂直同步模式直接使用原生 Tick；
  - 固定 FPS 模式对原生 Tick 做间隔节流；
  - 传统 `DefaultRenderTimer` 后端会替换其固定 60FPS 驱动线程；
  - 该设置保存在 `AppSettings.RenderFrameRate`，重启后生效。

## 在线歌词与标签搜索

- `OnlineMusicSearchService` 同时支持网易云音乐和 QQ 音乐：
  - 网易云使用公开搜索接口 `/api/search/get/web`，详情、封面与歌词复用 `INeteaseApiClient`；
  - QQ 音乐使用公开搜索接口 `/soso/fcgi-bin/search_cp`（`new_json=1`，老接口 `client_search_cp`
    已被 QQ 下线、两个镜像域名都直接返回 HTTP 500，界面原本只会显示“网络连接失败”）读取标题、
    歌手、专辑、年份（`pubtime` 折算）、音轨/碟号、时长和专辑 mid（用于封面），歌词使用
    `fcg_query_lyric_new` 接口；`c.y.qq.com` 失败时自动换 `c6.y.qq.com` 重试，两个都失败才报
    “QQ 音乐搜索接口不可用”，不再误报成网络问题；
  - 两个来源都用独立超时与错误提示；封面通过 CDN 下载，限制大小，失败时保留本地封面；
  - 搜索结果只读取公开元数据、封面和歌词，不下载音频。
- 歌词格式支持“逐行”和“逐字优先”，并默认生成双语歌词：
  - 网易云同时提供 LRC、翻译与 YRC；逐字模式会转成增强 LRC；
  - QQ 音乐通过 `music.musichallSong.PlayLyricInfo`（`qrc=1&crypt=1&roma=1`）获取 LRC 和翻译
    （旧 fcg 接口作为回退）；`lyric` 字段是 QRC 密文时由 `QrcDecryptor`/`QqTripleDes` 解密：
    QQ 的 3DES 变体位/字节编号与标准 DES 不同（已知向量 key=`133457799BBCDFF1`、
    plain=`0123456789ABCDEF` → `F63A972CE52AA327`，而非教科书的 `85E813540F0AB405`），
    因此按 LDDC 的参考实现移植，密文按 8 字节块 ECB 解密后 zlib 解压得到 `<Lyric_1 LyricContent="…"/>`，
    再按 QRC 行/词时间戳解析；因此 QQ 也能产出逐字（QRC）歌词。
  - 双语输出使用 `ILyricParser.ToPlainLrc/ToEnhancedLrc(document, includeTranslation)`，
    把翻译行追加在原文之后；所有时间戳（行首、词起止）统一输出 3 位毫秒 `[mm:ss.fff]` /
    `<mm:ss.fff>`，因此翻译行与原文行使用完全相同的时间戳，不再人为顺延时间；
    `//` 等占位翻译不写入，无翻译时使用原文。
- 编辑器中的搜索结果列表点击后自动填入标签、封面与对应格式歌词，用户确认后再写入本地音频标签。
- 结果列表按 `LyricsMatchScorer` 的总匹配度从高到低排序，每条显示“匹配度 xx%”（鼠标悬停可看
  标题/歌手/专辑/时长四路分值）；低于“匹配度”滑块阈值的结果标注“（低于阈值）”。
- “匹配度”滑块范围 30%–100%，默认 60%，改动即保存到 `AppSettings.MatchThreshold`，并同时作为
  批量匹配与 NCM 解锁后自动匹配的命中阈值。
- 打开/重新读取本地文件后，以去掉扩展名（并清理常见音轨前缀）的文件名作为关键词，
  自动向当前选中的在线来源搜索一次；不会自动应用结果，仍由用户点击确认。
- 在线来源通过 `IOnlineMusicSearchService` 抽象，后续可继续扩展其他音乐平台。

## 批量标签、封面与歌词

- `BatchLyricsService`（内部沿用历史命名）扫描目录内 MP3/FLAC/M4A/OGG/WAV/AAC/WMA/OPUS 文件（可选递归）。
- 每个文件优先读取标签中的标题/歌手/时长；没有标题时直接使用清理过的文件名作为查询，
  避免把“标题 - 歌手”或“歌手 - 标题”拆错。
- 标签/封面和歌词分别按 `AppSettings.TagSourcePriority` / `LyricsSourcePriority` 的
  来源顺序尝试（网易云优先、QQ 优先或仅单一来源），以标题、歌手和时长相似度评分选择最佳结果，
  低于阈值不会被强行写入。
- 打分参考 MusicTag 桌面端与 LDDC：标题里的 “A / B”“A & B” 按分隔符切段后取最佳段（0.99 降权），
  歌手字段的 “A/B”“A feat. B”“A · B”“CV:” 同样切成多值后两两比较，避免并列写法被整串相似度拖低。
- 查询变体与打分集中在 `LyricsMatchScorer`（参考 音樂標籤 的多变体检索）：按“标题+歌手 → 标题 →
  歌手+标题 → 标题主体 → 去 feat. 版本 → 并列第一段”构造最多 3 组查询，每个来源取前 10 条候选逐条打分。
- 权重：有歌手时 标题 0.55 / 歌手 0.25 / 时长 0.20；无歌手时 标题 0.72 / 时长 0.20 / 专辑 0.08。
  歌手相似度低于 0.30 时整体再乘 0.75（同名不同人的歌直接淘汰）；候选是伴奏/纯音乐版本时标记拒绝。
- 命中阈值 `AppSettings.MatchThreshold`（默认 0.60，范围 0.30–1.00）由歌词编辑窗口的“匹配度”滑块调整，
  同时作用于批量匹配与 NCM 解锁后的自动匹配；低于阈值的候选只做标注，不会被自动选中或写入。
- 对最佳结果获取标签/封面与歌词后，更新 Title/Artists/Album/Year/Track/Disc/Genre/Composer/Cover
  以及 Lyrics/WordLyrics；歌词按逐行或逐字（有则优先）写入双语内容。
- 网易云 / QQ 都取不到歌词（或检索落空）时，按顺序尝试备用词源 `ILyricsSource`：
  - 酷狗音乐（`KugouLyricsSource`）：KRC 逐字歌词 + 逐行译文。三段链路为
    `mobilecdn.kugou.com/api/v3/search/song` 取 hash（可缺省）→ `krcs.kugou.com/search` 取
    id/accesskey → `lyrics.kugou.com/download?fmt=krc` 取内容；base64 解码后前 4 字节明文魔数 `krc1`，
    其后按 16 字节固定密钥 XOR、再 zlib 解压；`[language:<base64 JSON>]` 里 type=1 是逐行译文、
    type=0 是逐字罗马音，`KrcLyricsParser` 统一转成 `LyricDocument`。
  - LRCLIB（`LrclibLyricsSource`）：仅逐行 `syncedLyrics`。先用 `?q=` 全文检索（最多 3 个标题变体，
    对超长/带修饰的日文标题命中率明显更高），全部落空再用 `?track_name=&artist_name=` 兜底；
    服务端 429/503 属瞬时失败，退避后重试一次，避免整首歌词白白丢失。
  - 备用词源命中的候选同样经 `LyricsMatchScorer` 打分并校验时长/伴奏标记，低于阈值的直接丢弃；
    写回时消息追加「（歌词来源：酷狗音乐/LRCLIB）」。酷狗在本机偶发
    `RemoteCertificateNameMismatch`，异常被吞掉后自动回落到下一个词源，不影响主流程。
  - 两个备用词源可在设置窗口的「备用词源」卡片里单独开关（`fallbackKugouLyrics` /
    `fallbackLrclibLyrics`，默认都开启）。`AppServices.ApplyFallbackLyricsSettings` 把它们翻译成
    `BatchLyricsService.FallbackSourceFilter`，被关掉的源在 `FindFallbackLyricsAsync` 里直接跳过；
    过滤器是运行期委托，设置保存后立即生效，不需要重建批量服务。
- 可选择覆盖已有标签与歌词；未开启时只补缺失字段、缺失封面和缺失歌词。
- 批量任务使用 3 个并发请求、进度回报和取消令牌；界面为独立窗口，从歌词编辑窗口右上角的
  “批量获取”按钮以编辑器为 Owner 打开。

## 歌词与标签编辑器

- 独立窗口 `LyricsEditorWindow`，从主界面“歌词编辑”或命令行 `--lyrics <file>` 打开。
- `LyricsEditorViewModel` 通过 `IAudioTagService` 读取文件标签：
  标题、艺术家、专辑、专辑艺术家、年份、音轨/碟号、风格、作曲家、注释、封面、歌词、
  文件大小/格式/时长/码率；无标签字段保持为空。
- 保存使用 `AudioTagData.ReplaceAll = true`，完整替换编辑器展示的标签字段；网易云 ID 标记、
  封面和未展示的标签会保留。清空某个字段会真正清除对应标签。
- 歌词按原始文本写入 TagLib 的 Lyrics 字段（支持 LRC / 增强 LRC / 逐字文本），
  写入后重新读取即可验证。
- 文件选择通过 `IDesktopUiService.PickAudioFileAsync` 调用 Avalonia StorageProvider。
- 在线搜索来源是 `LyricsSourceKind`（网易云音乐 / QQ 音乐 / 酷狗音乐 / LRCLIB）：选主源时走
  `IOnlineMusicSearchService` 并同时提供标签、封面与歌词；选备用源时按当前文件的标题、歌手和时长
  直接问该 `ILyricsSource`，结果标注“仅歌词”，载入只填歌词输入框，标签与封面保持当前内容。
  来源选择记忆在 `settings.json` 的 `lyricsEditorSearchSource`。

## 错误提示与运行日志

- `LunaticPrincess.Core.Utilities.ErrorText` 把 I/O 异常统一翻译成可操作的中文：`DescribeWrite(ex, targetDirectory)`
  优先用调用方给出的目标目录，其次用 `QuotedPathPattern` 从异常消息（兼容 .NET 英文与中文本地化）里
  抠出带盘符的路径；识别权限不足（`UnauthorizedAccessException` / `SecurityException` / HRESULT `0x80070005` /
  `is denied` / 拒绝访问 等）、磁盘空间不足（`0x80070070`）、文件被占用（`0x80070020`）、路径过长（`0x800700CE`），
  未知异常回退原始消息。
- 下载（`DownloadService`）、同步（`MusicSyncService` 的 NCM 解密与解密后处理）、NCM 导入
  （`NcmImportService` 的导入 / 在线匹配 / 读文件头 / 写标签）都改为走 `ErrorText`；桌面层
  `ErrorFormatter` 在通用分支之前新增 `ErrorText.IsKnownIoError` 判定，其余异常仍按原有规则格式化。
- 运行日志仍只写内存（`MaxLogEntries = 400`，不落盘）。默认展开，使用原有 92 DIP 固定高度面板；
  工具栏“收起日志 / 展开日志”命令切换 `MainWindowViewModel.IsLogExpanded`，收起时
  `LogScrollViewer.IsVisible = false`，整块日志从布局中移除（所在行是 Auto，不留空白）。

## 线程与取消

- 所有 I/O 使用 async；ViewModel 命令接受 CancellationToken。
- 批量下载通过 SemaphoreSlim 限制并发；进度用 IProgress 回报。
- 关闭窗口/取消操作时传递取消令牌，临时文件清理。

## Android 复用边界

| 能力 | Core 接口 | Android 需要替换 |
|---|---|---|
| 登录态加密 | ISessionStore | 平台 Keystore / EncryptedSharedPreferences |
| 设置 | ISettingsStore | SharedPreferences/DataStore |
| HTTP | 构造时注入 HttpClient | 可复用，配置 OkHttp 或保持默认 |
| 文件目录 | 由调用方传路径 | SAF / 应用私有目录 |
| 下载通知/后台 | 无 UI 依赖，可放前台服务 | WorkManager/ForegroundService |
| 二维码渲染 | UI 层职责 | Android 二维码组件 |

## 安全边界

- Cookie 仅存于本机；日志不打印 Cookie。
- 不集成“解灰”/跨平台替换音源等规避版权能力。
- 下载能力以用户自身账号权益为限。
