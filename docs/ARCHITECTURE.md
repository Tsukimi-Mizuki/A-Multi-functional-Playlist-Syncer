# 架构设计

## 目标

1. 桌面端（Windows/Avalonia）先把完整业务跑通。
2. 业务逻辑集中在 `NeteaseMusicManager.Core`，不依赖 Avalonia 或 Windows UI。
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

组合根位于 `src/NeteaseMusicManager.Desktop/Services/AppServices.cs`，负责创建 HttpClient、
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
3. 每首本地曲最多匹配一首歌单曲；低于阈值标记为缺失。

### 下载与标签

1. 受会员/版权限制：取链失败或仅试听时明确失败，不绕过版权限制。
2. 下载到临时文件；若文件头是 NCM 则自动解密。
3. 拉取歌词、翻译、封面、歌曲/专辑元数据。
4. 按歌词模式选择普通 LRC 或增强 LRC（逐字）。
5. TagLib 写入 MP3/FLAC/M4A/OGG 标签，原子移动到下载目录。

### NCM 解密

- AES-128-ECB 解出 RC4 密钥盒与元数据；按 32 KB 分块流式 XOR 解密音频。
- 输出格式由首块内容判断（ID3 => mp3，fLaC => flac）。

## 外观与窗口效果（单一磨砂玻璃）

- 只有一套外观：**磨砂玻璃**，基于 Salt UI 的卡片/胶囊按钮/紧凑 Item 结构，不再有主题选择器。
- 主窗口设置 `WindowTransparencyLevel.AcrylicBlur`（回退到 Blur），
  `Background = Transparent`，因此窗口后面的桌面、网页、视频和游戏会被 Windows 实时模糊透出，
  背景内容变化时窗口材质同步变化，不使用静态壁纸或内部模拟光斑。
- 半透明调色板集中在 `App.axaml`：Header/Panel 为 60%–70% 明度遮罩，
  内容区增加一层轻量暗色衬底保证表格可读；标题栏通过 DWM 固定为深色。
- 对话框不参与透明材质，使用深色实底 + 同一标题栏颜色，避免弹窗背景不可读。
- 侧栏通过 `Width` 过渡动画从 272px 收缩到 88px，同时交叉淡化列表内容与封面栏；
  原“我的歌单”文字替换为单个歌单图标按钮，最左上角不再有独立菜单按钮。收起后的封面栏
  使用第二个 ListBox 绑定同一 `Playlists/SelectedPlaylist`，纵向显示所有歌单封面；点击封面
  即切换当前歌单并触发加载。展开和收起状态的列表都使用上下渐变遮罩 + 渐变边缘，图标
  滚入/滚出与状态切换时淡入淡出；顶部展开箭头和 Ctrl+B 恢复完整侧栏。
- 受限环境（远程桌面、关闭透明效果、无 DWM）下 Acrylic 不可用时窗口退回深色玻璃背景。
- 帧率上限由 `RenderTimerInstaller` 在 Avalonia 初始化后接管 Win32 渲染计时器：
  - WinUI Composition 后端本身由 DWM 垂直同步驱动，垂直同步模式直接使用原生 Tick；
  - 固定 FPS 模式对原生 Tick 做间隔节流；
  - 传统 `DefaultRenderTimer` 后端会替换其固定 60FPS 驱动线程；
  - 该设置保存在 `AppSettings.RenderFrameRate`，重启后生效。

## 在线歌词与标签搜索

- `OnlineMusicSearchService` 同时支持网易云音乐和 QQ 音乐：
  - 网易云使用公开搜索接口 `/api/search/get/web`，详情、封面与歌词复用 `INeteaseApiClient`；
  - QQ 音乐使用公开搜索接口 `client_search_cp` 读取标题、歌手、专辑、年份、音轨/碟号、时长和
    专辑 mid（用于封面），歌词使用 `fcg_query_lyric_new` 接口；
  - 两个来源都用独立超时与错误提示；封面通过 CDN 下载，限制大小，失败时保留本地封面；
  - 搜索结果只读取公开元数据、封面和歌词，不下载音频。
- 歌词格式支持“逐行”和“逐字优先”：
  - 网易云同时提供 LRC 与 YRC，逐字模式会转成增强 LRC；
  - QQ 音乐只提供逐行 LRC，逐字模式自动回退逐行。
- 编辑器中的搜索结果列表点击后自动填入标签、封面与对应格式歌词，用户确认后再写入本地音频标签。
- 在线来源通过 `IOnlineMusicSearchService` 抽象，后续可继续扩展其他音乐平台。

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
