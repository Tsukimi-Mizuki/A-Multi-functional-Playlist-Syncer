# 架构设计

> 适用于 2.0 起的 Rust + Tauri 2 + React 版。旧的 C# / Avalonia 版（v1.x）在 `CSharp-GUI` 分支，架构见该分支的同名文件。
> 逐模块的实现细节、与 C# 版的差异和验证记录见 `docs/TAURI_REWRITE.md`。

## 目标

1. 业务能力完整（登录、歌单、本地对比、下载、NCM 解密、歌词 / 标签 / 封面、在线匹配），数据文件（`settings.json`、`session.dat`）与 C# 版互通。
2. 业务逻辑全部集中在平台无关的 Rust 库 `lp-core`，不依赖 Tauri 或任何界面框架，带单测；外壳只做命令转发、事件与窗口。
3. 界面是 Tauri 2 + React + TypeScript 的「液态玻璃」风格，深色 / 浅色 / 跟随系统，无边框自绘标题栏。
4. 只面向 Windows 10/11 x64；不绕过版权与会员限制。

## 分层

```
React 前端（app/src）
  api/*.ts        类型化的 invoke 封装与 DTO 类型
  state/*.ts      业务 hook（登录、歌单、本地对比、下载同步、日志、拖放、编辑器、批量修改标签、外观）
  components/*    纯展示 + 交互
        │  Tauri IPC：命令（invoke）与事件（listen）
        ▼
Tauri 外壳（app/src-tauri/src/lib.rs）
  AppState = 组合根：路径、设置、登录态、NeteaseClient、各服务与管理器、任务槽
  命令函数：参数转换 → 调用 lp-core → 把进度转成事件、错误转成 { kind, message }
        │  普通函数调用（trait 对象注入）
        ▼
lp-core（app/crates/lp-core，无 Tauri 依赖）
  paths settings models session auth logs qr
  netease  playlist  lyrics  tags  ncm  library  download  sync  online  editor
  utilities error
```

组合根在 `AppState::load`：解析数据目录、读取设置与登录态、构造 `ReqwestTransport` / `NeteaseClient` 并交给各服务；前端永远不直接接触 Cookie（`get_app_state` 只带昵称等摘要）。
`lp-core` 里 Windows 专属的只有 DPAPI（`session`）、已知文件夹（`paths`）、进程令牌提权检测（`editor`），都经 `windows-sys`。

## lp-core 模块

| 模块 | 内容 |
|---|---|
| `paths` | 数据目录解析（环境变量 → 便携 `data\`（安装包与便携版 zip 都自带它）→ `%LOCALAPPDATA%\LunaticPrincess` → 程序目录 → `%TEMP%`，每个候选做写入探测）、默认下载目录、`webview\` 子目录 |
| `settings` / `models` | `AppSettings`（31 个键）、`normalize()`、严格补丁 `apply_patch`（未知键 / 类型不对 / 枚举无法识别 / 越界整份拒绝）、`effective_window_material`（内置背景不铺系统材质）、`JsonSettingsStore`（临时文件 + rename 原子写）；`settings::liquid_glass`：液态玻璃 20 个数值参数 + 模糊边缘 + 色调的一张范围表（默认值、宽容读取、严格补丁、只改提到的参数）；枚举 `AudioQuality`、`LyricsWriteMode`、`OnlineSourcePriority`、`ThemeMode`、`WindowMaterial`、`BackgroundSource`、`GradientPreset`、`LyricsSourceKind` |
| `session` | `NeteaseSession`、`SessionStore`（`NMMSES01` 头 + DPAPI CurrentUser / 明文）、`SessionAutoSaver`（订阅会话变化、串行落盘、`flush`） |
| `auth` | `LoginManager`：`restore` / `validate`（启动校验）/ `qr_start` / `qr_poll` / `logout` / `handle_runtime_auth_error` / `ensure_keepalive`（12 小时续期）/ `shutdown` |
| `netease` | `crypto`（weapi：AES-128-CBC 双层 + 无填充 RSA；eapi：MD5 摘要 + AES-128-ECB，目前只给搜索用）、`http`（`HttpTransport` trait、`ReqwestTransport`、流式响应）、`cookie_merger`、`client`（会话代数、20 s 超时、Set-Cookie 合并）、`login`（二维码）、`playlists`、`songs`、`player`（取链与逐级降档）、`lyrics`、`search`、`mappers` / `models` / `json` / `error` |
| `playlist` | `PlaylistService`：歌单列表与曲目加载、写运行日志、301 走登录复核 |
| `lyrics` | `models` / `parser`（LRC、增强 LRC、YRC、QRC、译文合并、序列化）、`qrc` + `QqTripleDes`、`krc`、`zlib`、`html`、`service`（歌词预览：按「歌词来源」依次问网易云 / QQ / 备用词源）、`scorer`（匹配打分与查询变体）、`batch`（`BatchLyricsService`、`BatchEnricher`、`BatchManager`） |
| `tags` | `AudioTagService` trait + `LoftyTagService`；`text`（多值拆分、网易云 ID 标记、MIME 判定）、`view`（DTO、封面 data URL）、`table`（批量修改标签：列表行 `TagRowView`、只写提到的字段的 `TagPatch`、并行读 / 写） |
| `ncm` | `NcmDecoder` trait + `FileNcmDecoder`（流式）、`client_comment`（`163 key` 解密取 `musicId`）、`synthetic`（仅测试） |
| `library` | `LocalLibraryService`（并行扫描）、`matcher`（一对一匹配）、`text`（NFKC / 字符类别，按 UTF-16 码元复现 .NET 语义）、`LibraryManager`（缓存与编排）、`view` |
| `download` | `DownloadService`、`media`（文件名 / 原子落盘 / 标签数据）、`fetch`（流式下载）、`pool`（并发任务池）、`task`（`TransferManager`：下载选中 / 下载缺失 / 同步）、`ncm_import`（`NcmImportService` + 扫描 + 任务槽） |
| `sync` | `SyncService`：本地命中 / NCM 解密 / 下载三分支 |
| `online` | `OnlineMusicSearchService`（网易云 + QQ）、`qq`、`kugou`、`lrclib`、`net`、`OnlineManager`（编辑器用的搜索 / 载入 / 备用词源）、`view` |
| `editor` | 编辑器记忆、`resolve_drop`、`parse_startup_args`、`read_cover_file`、`is_elevated` |
| `update` | `UpdateChecker`（读 GitHub `releases/latest`、比较版本、处理已忽略版本、写运行日志）、`Version`（tag 归一与 SemVer 优先级，规则同 `scripts/resolve-version.ps1`）、`release_notes_text`、`open_release_page`（只打开本项目发布页） |
| `logs` / `qr` / `error` / `utilities` | `LogHub`（400 条环形缓冲 + 监听器）、二维码 SVG、`CoreError`、`error_text` / `file_name_sanitizer` / `audio_file_types` / `cancel`（`CancelToken`） |

## Tauri 命令与事件

命令的失败 `Err` 统一是中文的 `String`，或 `{ kind, message }` 对象（`kind` 供前端分支，如 `busy`、`noSongs`、`playlistNotLoaded`、`cancelled`、`notLoggedIn`、`sessionExpired`、`failed`）。DTO 全部 camelCase。

| 领域 | 命令 |
|---|---|
| 应用 / 日志 | `app_info`、`get_app_state`（路径、便携模式、设置摘要、账号摘要、九档音质选项、读取警告）、`get_logs` |
| 登录 | `login_status`、`validate_login`、`qr_start`、`qr_poll`、`logout`、`recheck_login` |
| 歌单 / 歌词 | `list_playlists`、`get_playlist_tracks`、`get_song_lyrics` |
| 标签 | `read_tags`、`write_tags`、`read_cover_file`；批量修改标签：`read_tag_rows`（一批文件的标签摘要，不带封面数据，读不出来的文件也有一行）、`patch_tags`（把同一份改动写进一批文件：没出现的字段不动，`null` / 空白表示清除，逐个文件返回结果与写入后的那一行） |
| NCM 导入 | `ncm_get_state`、`ncm_scan`、`ncm_save_state`、`ncm_import`、`ncm_cancel` |
| 本地曲库 | `scan_library`、`match_playlist`、`library_cancel` |
| 下载 / 同步 | `set_quality`、`download_tracks`、`download_missing`（`trackIds`：确认框里仍勾选的歌，只下载其中仍然缺失的）、`sync_playlist`、`download_cancel` |
| 在线匹配 | `online_search`、`online_load_result`、`lyrics_source_fetch`、`get_match_threshold`；在线标签：`online_tag_candidates`、`online_cover_size`、`online_load_tags` |
| 批量获取 | `batch_scan`、`batch_run`、`batch_cancel` |
| 编辑器 / 拖放 | `editor_get_state`、`editor_remember_options`、`resolve_drop`、`take_startup_file` |
| 设置 / 外观 | `get_settings`、`save_settings`（只传改动的键）、`preview_appearance`（不写盘） |
| 更新 | `check_for_updates`（`manual` 为 false 且关了自动检查时不联网、返回 `null`；失败是 `{ kind: network \| rateLimited \| noRelease \| invalid, message }`）、`ignore_update`、`open_release_page` |

| 事件 | 载荷 |
|---|---|
| `app:log` | 新日志条目（`id` 单调递增；前端先监听、再 `get_logs`、按 `id` 去重） |
| `auth:state` | 账号摘要（登录成功 / 退出 / 失效 / 资料刷新） |
| `playlist:progress` | `{ playlistId, loaded, total }`（只有需要分批补齐时才发） |
| `library:progress` | `{ scanned, total, fileName }`（节流 80 ms，最后一条必发） |
| `ncm:progress` | `{ completed, total, index, path, fileName, stage: decrypting \| matching \| finished, message, result? }` |
| `download:started` / `download:progress` | 任务计划 / 逐曲进度（阶段、百分比、字节、最后一条带 `finish`） |
| `batch:progress` | `{ completed, total, index, path, fileName, stage: started \| finished, result }` |

另外，拖放用 Tauri 窗口事件 `tauri://drag-enter / over / drop / leave`，前端 `useFileDrop` 监听。命令载荷与字段的完整列表见 `app/src/api/*.ts` 和 `docs/TAURI_REWRITE.md`。

## 关键流程

### 扫码登录与续期

1. `POST interface.music.163.com/api/login/qrcode/unikey`（`type=3`，失败降级 web `type=1`）获取 unikey。
2. 把 `https://music.163.com/login?codekey=<unikey>` 渲染成二维码 SVG（`qrcode` crate，纠错级别 Q，白底黑码含静区）。
3. 前端每约 2 秒调 `qr_poll`：`801` 等待、`802` 已扫待确认、`803` 成功、`800` 过期；暂时性网络错误前端最多连续容忍 3 次。
4. 803 时合并 Set-Cookie 与响应体 cookie，`__csrf_token` 别名成 `__csrf`，构造 `NeteaseSession`，**先落盘再让登录态生效**（保存失败不留「内存已登录、重启就丢」的半截状态）。
5. 启动时 `validate_login` 用 `/weapi/nuser/account/get` 校验，成功后启动 12 小时一次的续期任务；应用退出（`RunEvent::Exit`）时停掉续期并 `flush` 自动落盘。

续期与保持：

- `NeteaseClient` 对每个带会话的响应用 `CookieMerger` 合并 Set-Cookie（按名覆盖、`Max-Age` 优先于 `Expires`、过期即删除、空值不覆盖、顺序保持）；合并后会丢 `MUSIC_U`，或请求期间会话被切换（会话代数变了），则丢弃本次合并。变化时通知订阅者，`SessionAutoSaver` 把**当前**会话串行写盘（连续变化合并，已退出则跳过）。
- 「登录失效」只认接口业务 `code == 301`，或 `account/get` 返回 200 但 account / profile 为空。HTTP 401/302/5xx、406 限流、-460 风控、超时与网络错误都只提示「暂时无法验证登录状态，已保留登录」。判定失效前先 `refresh_login` + 再次 `get_current_user` 复核，仍明确未登录才清除。
- 清除顺序：`set_session(None)` → `flush` → 删 `session.dat`；运行期 301 复核同一时间只做一次。
- `SessionStore::load` 区分「文件不存在 / 没有可用 Cookie」（正常未登录）与「损坏、无法解密、无法解析、读取失败」（日志里给出原因与数据目录）。
- **续期走 eapi**：`refresh_login` / `try_refresh_login` 调 `NeteaseClient::post_eapi_with_session`，`POST https://interface.music.163.com/eapi/login/token/refresh`——表单只有 `params`，明文是 `/api/login/token/refresh` + 业务 JSON（`{"e_r":false,"header":{os,appver,osver,deviceId,channel,versioncode,mobilename,buildver,resolution,requestId,__csrf,MUSIC_U,NMTID}}`）+ MD5 摘要，AES-128-ECB 后大写 hex；`header` 同时作为 Cookie 头发出（只带这些，不带 `MUSIC_R_U` 等）；桌面 UA，不发 Referer。成功（`code=200`、`bizCode=200`）时服务端换发新的 `MUSIC_U`（`Max-Age` 约 180 天）、`__csrf`（约 15 天）与 `MUSIC_R_U`，并且新旧令牌都有效，可以连续续期。weapi 的同名端点对桌面端会话返回 `code=400`，带桌面 Cookie 时是 `code=200 bizCode=201` 的空操作，**不能用**。
- **同名多 Path 的 Set-Cookie**：服务端会在同一响应里对 `MUSIC_A_T` / `MUSIC_R_T` 等下发二十多条不同 `Path`（`/api/feedback`、`/neapi/clientlog` …）的设值，其中夹一条 `Path=/` 的 `Max-Age=0` 删除，顺序随机。`CookieMerger` 不区分 `Path`，所以规则是：带非根 `Path` 的删除永远忽略；同一响应里既有删除又有设值的名字，删除忽略、设值生效；其余删除照常。详见 `netease::cookie_merger` 的模块文档与 `docs/TAURI_REWRITE.md`「登录续期修复」。

### 歌单与歌曲

- weapi 请求：AES-128-CBC 双重加密 + 无填充 RSA 的 `encSecKey`。
- `/weapi/user/playlist` 以 `limit=1000` 翻页（最多 10 页，按 id 去重）。
- 大歌单先取 `trackIds`（`/weapi/v6/playlist/detail`，`n=1000, s=8`），再按 500 首 / 批调用 `/weapi/v3/song/detail` 补齐，最后按 `trackIds` 顺序还原；某批失败重试一次，仍失败置 `is_partial`；业务 301 立即向上返回走登录复核。
- 前端表格是固定行高 48 px 的自写虚拟滚动，3000 首时 DOM 里只有约 24 行。

### 本地对比

1. 扫描目录（扩展名 `.mp3 .flac .m4a .mp4 .ogg .opus .wav .ncm`，最多 8 个线程并行读标签，结果按路径排序；符号链接不跟随，无权限目录跳过并计数）：普通音频读标签，NCM 读文件头内嵌元数据；标题 / 歌手缺失时从文件名推断。
2. 匹配（`library::matcher`）：第一阶段网易云 ID 精确匹配（`LunaticPrincess:<id>` 注释，或客户端文件里的 `163 key(Don't modify):<base64>`——AES-128-ECB 解出 `music:{...}` 取 `musicId`）；第二阶段所有候选进优先队列，综合分 = 标题 0.55 + 歌手 0.30 + 时长 0.15，**阈值固定 0.72**，每首本地文件最多匹配一首歌单曲。
3. `LibraryManager` 在内存里缓存最近一次扫描结果（曲库目录文本变了自动作废），同一时间只跑一个扫描，进度节流；歌单里重复出现的同一首歌沿用第一次出现那一行的匹配结果。

### 下载与标签

音质档位映射为网易云 `level`：

| 档位 | level | 档位 | level |
|---|---|---|---|
| 标准 | `standard` | 沉浸环绕声 (Surround Audio) | `sky` |
| 极高 (HQ) | `exhigh` | 超清母带 (Master) | `jymaster` |
| 无损 (SQ) | `lossless` | 臻音全景声 (Audio Vivid) | `audio_vivid`（别名 `vivid`） |
| 高解析度无损 (Hi-Res) | `hires` | 杜比全景声 (Dolby Atmos) | `dolby` |
| 高清臻音 (SpatialAudio) | `jyeffect` | | |

单曲下载（`DownloadService`）：

1. 按文件名模板算基名；`skip_existing && !overwrite_existing` 且下载目录已有同基名音频 → 直接跳过，不发起网络请求。
2. 取链（`netease::player`）：从请求档位开始逐级降档，每档先试 `/weapi/song/enhance/player/url/v1`、再试经典接口；取链请求带桌面客户端 Cookie（`os=pc; appver=…; deviceId=LunaticPrincessPc; channel=netease`），缺少时服务端按网页端策略静默降档。没有地址 → 失败，试听片段（`freeTrialInfo`）一律判失败。
3. 服务端 `level` 与请求档位不同 → 提示「实际音质：…（… 不可用，已自动降级）」。
4. 流式下载到 `.lp-<进程号>-<序号>.part`（总超时 30 分钟、读取间隔 60 s）；HTTP 非 2xx、内容为空、声明长度与实收不符都判失败。
5. 文件头是 `CTENFDAM`（NCM）→ 改名并用 `NcmDecoder` 解密，输出格式以解密结果为准，封面优先用 NCM 内嵌的。
6. 歌词（逐行普通 LRC；逐字模式另写增强 LRC，译文默认写入）、封面（网易云图床按「最长边 4000、输出 JPEG」取图，见 `netease::cover`；> 10 MB 或失败跳过）。
7. lofty 写标签（标题 / 歌手 / 专辑 / 专辑艺术家 / 音轨 / 碟号 / 年份 / 封面 / 歌词 / 网易云 ID / 备注 `LunaticPrincess`）并原子落盘；冲突加 `(歌曲ID)` 后缀，`overwrite_existing` 则覆盖。标签失败仍保留音频并算成功。
8. 在线补全（`BatchEnricher`，阈值、歌词写入方式与「写入封面」取设置）：标签与封面网易云优先、QQ 兜底，歌词 QQ 逐字优先、网易云兜底（两者来源不同时结果里写明歌词来源）；文件里已有封面时只换成像素更多的（`image_dimensions` 读文件头比较），关掉「写入封面」时不写封面；失败只作提示。QQ 封面从 1200×1200 起，服务端没有这一档才依次换 800 / 500 / 300。

一键同步（`SyncService`）：本地是 NCM → 解密到下载目录下的**临时子目录**、写 NCM 内嵌标签、取最新歌曲详情、按在线数据重写标签后原子落盘；本地已有普通音频且不要求重新下载 → 跳过；否则下载。与下载共用并发任务池（`maxConcurrency`），结果顺序与输入一致，取消时保留已完成结果。

二次确认（前端 `useTransferConfirm`）：「下载缺失」与「一键同步」先弹确认框，确认后才调命令；「下载选中」不确认。

- 先拿本地对比结果（`useLocalMatch.ensure`）：已经对比过直接用；没有就先对比（必要时扫描，表格同时刷新），扫描被取消或失败就不弹窗（原因显示在对比工具条上）；曲库目录不存在时与后端一样按本地为空处理，确认框里给出提示。
- 计划由 `planTransfer` 算出：按歌曲 ID 去重，沿用第一次出现那一行的匹配结果（与后端 `unique_rows` 一致）。
- 「下载缺失」（`DownloadMissingDialog`）：列出缺失歌曲（固定行高虚拟滚动，默认全选；点行或复选框切换，Shift 连选，表头三态全选），底部按钮写明要下载的首数，一首都没勾时不可点；待解密的 NCM 不在清单里，另起一行说明。确认后把仍勾选的 ID 交给 `download_missing(trackIds)`，后端在当时的对比结果里只挑这些 ID 中**仍然**缺失的（确认之后才出现在曲库里的不再下载；空列表报 `noSongs`）。没有任何缺失时不弹窗，只在下载工具条里提示。
- 「一键同步」（`SyncConfirmDialog`）：歌单共几首、本地已存在跳过（设置开了「覆盖已存在文件」则写明重新下载）/ NCM 解密 / 需要下载各几首，音质与下载目录（确认时读一次设置），歌单里有重复的歌时说明只处理一次。

### NCM 解密

- AES-128-ECB 解出 RC4 密钥盒与元数据；32 KB 分块边读边解边写同目录临时文件（`.<主干>.<pid>-<序号>.ncmtmp`），成功后原子改名，取消 / 失败都清理，**音频不会整文件进内存**。
- 输出格式由解密后首块判断（`ID3` / 帧同步字 → mp3，`fLaC` → flac，其余按 flac）。可预期的失败（文件头不符、长度非法、被截断、没有音频、目标已存在）返回 `success == false` 的结果，元数据损坏绝不中断音频解密。
- `NcmImportService` 解密成功后写回内嵌标题 / 歌手 / 专辑 / 封面 / 网易云 ID；勾选「解密后自动获取在线标签、封面与歌词」（默认勾选）时再逐个调 `OnlineEnricher`。
- 导入窗口的输入文件夹列表与输出目录自动记忆（只改 `ncmImportFolders` / `ncmImportOutputDirectory` 两个字段；设置文件当初没读出来时不写盘）。

### 在线歌词与标签匹配

- `OnlineMusicSearchService` 支持网易云与 QQ 音乐：网易云搜索走匿名 eapi `POST /eapi/cloudsearch/pc`（桌面端 `header`、不带登录态，海外 IP 下也是明文；`interface.music.163.com` 连接失败换 `music.163.com` 再试一次；eapi 被明确拒绝或返回无法识别的内容时回退网页接口 `/api/search/get/web`，它在海外 IP 下返回密文所以只作回退；`406` / `-460` / 429 限流风控不回退），详情 / 封面 / 歌词复用 `NeteaseClient`；QQ 搜索 `/soso/fcgi-bin/search_cp`（`new_json=1`，`c.y.qq.com` 失败切 `c6.y.qq.com`），歌词 `music.musichallSong.PlayLyricInfo`（`qrc=1&crypt=1&roma=1`，QRC 密文由 `lyrics::qrc` 解出逐字），旧 `fcg_query_lyric_new` 作回退。
- 歌词格式「逐行」与「逐字优先」，默认生成双语：译文行追加在原文之后，所有时间戳统一 3 位毫秒（`[mm:ss.fff]` / `<mm:ss.fff>`），`//` 等占位译文不写入。
- 打分（`lyrics::scorer`）：标题里的 `A / B`、`A & B` 按分隔符切段取最佳段，歌手字段的 `A/B`、`feat.`、`·`、`CV:` 切成多值两两比较；有歌手时标题 0.55 / 歌手 0.25 / 时长 0.20，无歌手时标题 0.72 / 时长 0.20 / 专辑 0.08；歌手相似度 < 0.30 整体 ×0.75；伴奏 / 纯音乐版本直接排除；阈值默认 0.60（0.30–1.00，设置里以百分数输入）。查询变体最多 6 组，每源最多 3 组、每组前 10 条。
- 网易云 / QQ 都拿不到歌词时按顺序回落备用词源 `LyricsSource`：酷狗（KRC 逐字 + 译文，三段链路）、LRCLIB（逐行，`?q=` 多变体 → `track_name` 兜底，429 / 503 退避重试一次）。备用词源命中的候选同样经打分，写回时标注「（歌词来源：酷狗音乐 / LRCLIB）」，可在设置里单独开关（运行期读共享设置，保存后下一首歌生效）。
- **歌词预览**（主界面双击歌曲，`get_song_lyrics` → `LyricsService::preview`）跟随设置里的「歌词来源」（`lyricsSourcePriority`，每次调用时读取，保存后下一次预览就生效）：按 `batch::order_sources` 的次序逐个尝试，网易云按歌曲 ID 精确取词，QQ 音乐用歌名 / 歌手 / 专辑 / 时长经 `BatchLyricsService::find_source_lyrics` 检索打分，主来源都没有时再按备用词源开关问酷狗、LRCLIB。某个来源出错就问下一个；网易云确认登录失效时立即返回；都没有歌词时，途中出过错就报第一个错误，否则显示空预览并列出试过的来源。QQ 没有译文时「双语」版本与原文相同，预览不把它算作有翻译。
- **任何搜索都不自动应用结果**：编辑器里只高亮排序，用户点选后才载入表单，避免悄悄覆盖正在编辑的内容。
- 批量在线获取（`BatchManager`，入口在「批量修改标签」窗口）：对列表里选中的歌（一首都没选就是整个清单）→ 最多 3 个文件并发 → 逐文件 `started` / `finished` 事件 → 汇总；取消后不再启动新文件并保留已完成结果。

### 歌词与标签编辑器、拖放

- 编辑器与批量窗口都是叠在主窗口上的模态面板（`Modal` 组件，模态栈：Esc 只关最上面一个），不是独立 Tauri 窗口——拖放事件、IPC、主题 token 与「未保存修改」确认都在同一个 WebView 里。
- 读取 → 编辑 → `write_tags(replaceAll: true)` 保存 → 重新读取；保存保留网易云 ID 标记，没提供封面则封面不动，`removeCover` 才删除。
- **多值字段的保存**（`useLyricsEditor::multiValuePlan`）：艺术家等多值字段在界面上是用 `/` 接起来的一行字。保存时：这行字还是在线结果接出来的那一串 → `artists` 用在线结果的歌手清单（`OnlineSong.artists`，边界是来源给的）；这行字和打开文件时一样 → 字段名放进 `keep`，后端不碰文件里的这个字段（`AudioTagData::keep` 优先于 `replace_all`、给的值与 `clear`；备注不支持）；用户改过 → 按 `/ ; ； 、` 拆。套用了在线清单而文字没变时也算「有未保存的修改」。
- **同名歌词文件**：`read_tags` 读完标签后，若文件里没有内嵌歌词，就读旁边同名的 `.lrc`（`tags::sidecar::read_sidecar_lyrics`：UTF-8 / 带 BOM 的 UTF-16，上限 2 MB，任何问题都当作没有）放进 `TagView.sidecarLyrics`。编辑器把它填进歌词框（不算「有未保存的修改」），歌词标题下与状态栏注明来源；保存时随表单写进音频标签，`.lrc` 文件本身不动。批量修改标签的列表行同样回落（`TagRowView.lyricsFile` 是歌词文件名），「查看标签」也显示它。
- 封面旁显示格式、像素尺寸、大小与图片类型（`TagView` 的 `coverMimeType` / `coverWidth` / `coverHeight` / `coverSize` / `coverPictureType`；换成本地图片或在线封面时显示新图片的参数，类型是保存时写入的「正面封面」）。
- 拖放：`useFileDrop` 监听 `tauri://drag-*`；离开后 180 ms 才隐藏提示层（期间再次进入则取消，避免闪烁）。`editor::resolve_drop` 把拖入的目标展开成音频文件（文件直接取；文件夹展开为其中含子文件夹的全部音频，规则同批量获取扫描目录；按拖入顺序、不区分大小写去重）：只有一首时用编辑器打开（`audio`），两首及以上交给「批量修改标签」（`batch` + `files`，窗口里显示「拖入的 N 首音频」，不再扫描目录，「改用目录…」可切回），一首都没有时拖入了 `.ncm` 就提示改用「导入 NCM」，否则给出第一个失败原因；无法处理的目标跳过并在日志里说明。编辑器开着时由编辑器处理（多首打开叠在它上面的批量窗口）；批量窗口开着时新拖入的文件（一首也算）替换它的清单，正在处理时只提示、不替换。C# 版拖入多个只打开第一个，这里有意不同。
- **在线标签**（「标签信息」右上角的按钮，`OnlineTagsDialog` + `useOnlineTags`）：只找标签与封面，不取歌词。打开时用表单里现在的标题 + 艺术家（标题空着用文件名推）搜一次，关键词可改了重搜；打分用表单里现在的标题 / 艺术家 / 专辑与文件时长。
  - `online_tag_candidates`（`OnlineManager::tag_candidates`）：同一个关键词并行搜网易云与 QQ 音乐，每个来源最多 10 条，合并后按总分从高到低（同分网易云在前）；一个来源失败时写进 `failures`、另一个照常返回，两个都失败才报错。每条带 `thumbnailUrl`（网易云 `?imageView&thumbnail=240x240&type=jpg`，QQ 换成 300 档，一律 https——CSP 只放行 https 图片），由界面直接加载。
  - `online_cover_size`：逐行探测「下载下来会是多少像素」（`OnlineHttp::probe_cover_size`：流式读图片开头，`image_dimensions` 认出尺寸就断开，最多读 512 KB；网易云探测的是带 4000 上限参数的下载地址，QQ 按 1200 / 800 / 500 / 300 换档）。前端最多 4 条同时探测，没探到显示「尺寸未知」。
  - `online_load_tags`：用户按「确定」后才取选中那一条——`OnlineMusicSearch::get_tag_details` 不发歌词请求（网易云只调详情接口，QQ 的标签就是搜索摘要），`cover: false` 时连封面也不下载。前端按勾选的「标签」「封面」填进表单（`useLyricsEditor::applyOnlineTags`：在线结果里空着的字段保持原样，歌词不动），保存仍走编辑器的「保存标签与歌词」。
- 管理员身份运行时 Windows 用 UIPI 阻断资源管理器向本程序拖放（系统限制），界面顶部与启动日志给出提示。
- `--edit <文件>` / `--lyrics <文件>` 启动参数由 `parse_startup_args` 解析，前端启动后调 `take_startup_file`（只交出一次）。

### 批量修改标签

布局参考用户给的标签编辑器截图（见 `docs/design/README.md`）：右边是歌曲列表，选中歌曲后左边显示它们的标签。组件 `BatchTagsDialog` + 状态 `useBatchTags`，从编辑器的「批量修改」按钮或拖入多首打开。

- **列表**：选目录（`batch_scan`，可含子文件夹；带着目录打开时自动列出）或直接用拖入的文件；标签由 `read_tag_rows` 每 100 个文件一批读出（后端最多 8 个线程并行），边读边填，读不出来的文件也占一行、「状态」栏写原因。固定行高 34 px 的虚拟滚动；列多于可视宽度时横向滚动，表头粘在滚动区顶部一起移动。列的次序同参考图（文件名、目录、格式、标题、艺术家、专辑、专辑艺术家、年份、音轨号、碟号、风格、作曲家、作词家），多出来的「状态」放在文件名后面。
- **选择**：单击选一首，Ctrl 加选 / 减选，Shift 连选，↑ ↓ PageUp PageDown Home End 移动（带 Shift 连选），Ctrl+A 全选。
- **左边的面板**：字段次序同参考图（标题、艺术家、专辑、年份 + 音轨号 + 碟号并排、风格、专辑艺术家、作曲家、作词家、注释、歌词、封面），1280×800 的默认窗口里不用滚动。选中的歌某个字段都一样就显示它，不一样显示「〈保持〉」；动过的字段高亮、可单独还原，动过又留空显示「〈清空〉」。保存（按钮或 Ctrl+S）时 `patch_tags` 只带动过的字段，每 20 个文件一批写入（后端最多 4 个线程并行），逐个文件把结果写进「状态」栏并刷新那一行；有文件失败或中途取消时面板里的改动保留，方便再保存一次。封面可以换成一张图片或移除（对所有选中的歌）；只选一首时用 `read_tags` 读出封面预览与参数。
- **`TagPatch` → `AudioTagData`**（`tags::table`）：没出现的字段不动；给了值的写入；`null` / 空白 / 空数组的进 `AudioTagData::clear`（非 `replace_all` 时清除这些字段，音轨号 / 碟号被清除时总数一并清除）。改动里动了备注时先读出这个文件的网易云 ID 随备注一起写回，`LunaticPrincess:<id>` 标记不会跟着旧备注丢掉。歌词不在面板里改：面板只预览第一行，「编辑…」（或双击一行）用歌词与标签编辑器打开这一首（叠在批量窗口上，不显示「打开音频文件」与「批量修改」），关闭后刷新这一行。
- **在线获取**：对选中的歌（没选就是全部）调 `batch_run`，选项（覆盖已有、同时获取歌词、歌词格式）在列表上方；逐文件结果写进「状态」栏，成功的文件重新读标签。面板里有没保存的改动时不能开始。
- **没保存的改动**：换一批选中的歌、重新列目录、换成拖入的文件、打开编辑器、关闭窗口之前都先确认。从编辑器打开的批量窗口关闭后，编辑器里的文件如果没有未保存的修改就重新读一遍，避免之后把旧内容整份写回去。

### 标签写入的安全性

复制成同目录临时副本 `<原名>.lp-tmp-<进程号>-<序号>` → 改动全部写进副本 → 重新解析确认类型没变 → `rename` 原子替换；任何一步失败都清理副本、保留原文件。底层用 lofty 的通用 `Tag` + `ItemKey`，写回时转成容器自己的标签类型（ID3v2 / VorbisComments / ilst / RIFF INFO / APE），编辑器没展示的字段（TXXX、PRIV、UFID、ReplayGain、自定义 Vorbis 字段等）原样保留。各容器的字段映射与怪癖见 `docs/TAURI_REWRITE.md` 模块 6。

### 检查更新

- 启动约 3 秒后（前端 `useUpdates`，让登录校验与歌单加载先走），设置里「自动检查更新」（`autoCheckUpdates`，默认开）开着就调 `check_for_updates(manual = false)`：后端匿名请求 GitHub `GET /repos/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases/latest`（只返回正式版，不含预发布与草稿；15 s 超时、不重试），tag 按 `scripts/resolve-version.ps1` 的规则归一（`v2.1` → `2.1.0`），与当前版本按 SemVer 比较。
- 结果三种：`available` → 弹「发现新版本」（版本、发布日期、Release 正文整理成的纯文本：去掉标题的 `#` 与自动生成的 Full Changelog 行，最多 4000 字）；`ignored`（正好是 `ignoredUpdateVersion` 记住的版本）→ 自动检查不弹窗；`upToDate` → 只写日志。出了比忽略的版本更新的版本照常提醒。
- 弹窗三个选择：「跳转至下载页面」用默认浏览器（`ShellExecuteW`）打开发布页，成功后关窗、失败把原因和地址显示在弹窗里；「忽略本次更新」把归一后的版本写进 `ignoredUpdateVersion`（只改这一个键；设置文件读取失败时只改内存）；「稍后提醒」（Esc、点遮罩也算）只关窗，下次启动再提醒。程序不下载、不安装更新。
- 设置「关于」里的「立即检查」不受开关影响，发现新版本（包括忽略过的，弹窗里注明）照样弹窗，叠在设置面板上；已是最新或失败原因（网络 / 超时、403 / 429 限流、404 没有正式版、内容无法识别）显示在按钮旁。自动检查失败只记一条普通日志（离线时不该每次启动都报警告），手动检查失败记警告。
- `open_release_page` 只接受以 `https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases` 开头、只含 URL 路径安全字符的地址；GitHub 返回的 `html_url` 不合格时改用 `/releases/latest`。

## 外观与窗口效果（液态玻璃，深色 / 浅色）

- **窗口层**：主窗口在 `setup` 里用 `WebviewWindowBuilder::from_config` 创建（`tauri.conf.json` 里 `create: false`），无边框、透明，WebView2 用户数据目录显式放到 `<数据目录>\webview`。背景（`background`）选内置背景时整个窗口由网页自己画，不铺系统材质（`effective_window_material` 为 `None`，启动、保存与预览都按它）；选「系统材质」时 `apply_window_material` 给窗口铺系统材质：`Acrylic`（默认，实时模糊并透出窗口后面的桌面与其他窗口）/ `Mica`（只取壁纸色调）/ `None`；首选材质不可用时自动退到另一种，两种都不可用（远程桌面等）保持纯透明，日志与设置面板都会提示实际生效的材质。材质底色随主题走（`window.set_theme`，跟随系统时交还系统）。
- **内置背景（WebGL 液态玻璃）**：`GlassBackdrop` 在 `.app-shell` 最下层铺一张全窗口画布（WebGL2，`glass/renderer.ts`）：背景（渐变光斑四套配色 / 自选图片经 `read_cover_file` 读成 data URL / 歌单封面直接从网易云图床取，图床返回 `Access-Control-Allow-Origin: *` 才能当纹理）+ 面板投影 → 按模糊半径选 mipmap 层级降采样再做两遍高斯模糊（单方向最多 48 个采样）→ 玻璃合成。带 `data-glass` 的元素（标题栏、日志栏是 `chrome`，侧栏、歌曲区是 `panel`）的矩形并成一个超椭圆圆角 SDF；贴着窗口边的长条向窗口外延伸，只让朝里的那条边折射。玻璃内：按斯涅尔定律算边缘偏折量、往内侧取样并按色散增益把 R / B 通道错开，叠色调（浓度同 CSS 的 `--panel` / `--chrome-glass` 公式），再加菲涅尔亮边与按法线角度分布的高光（提亮在 OKLab 里做）。算法改编自 liquid-glass-studio（MIT，见 THIRD_PARTY_NOTICES.md）。只在尺寸、面板位置（ResizeObserver）、外观参数（`subscribeAppearance`）或背景图变化时重画一帧，不跑动画循环。WebGL2 不可用时 `<html data-webgl="off">`，界面退回系统材质的样式，并把窗口材质铺回来。
- **界面层**：`styles/theme.css` 为每套主题定义底色三元组与高光 token，各层不透明度都由 `--glass-strength`（设置 `glassOpacity`，0.10–0.95，默认 0.45）推导；`styles/glass.css` 统一给标题栏、侧栏、歌曲区、对话框、设置卡片、按钮加上顶部柔光渐变、内侧 1 px 高光、阴影、`::after` + mask 画出的 1 px 边缘亮环。液态玻璃参数经 `glassCssVars` 换算成 `--lg-*` 变量（亮环按高光角度分布、菲涅尔亮边、色散彩边、阴影、圆角与 `corner-shape: superellipse()`），系统材质下的面板与两种背景下的按钮、对话框、卡片都用它近似；浅色主题下亮环的非高光部分是很淡的深色描边。内置背景时 `<html data-backdrop="builtin">`，标题栏、侧栏、歌曲区、日志栏的 CSS 底色 / 边框 / 阴影去掉，由 WebGL 画。`body` 保持透明，由半透明 token 叠加。
- **景深**：给下一层加 `filter`：有对话框时整个主界面 `.app-shell` 平滑虚化（`body:has(> .modal-backdrop:not(.modal-leaving)) .app-shell`），叠放对话框时被盖住的对话框也虚化，拖放提示出现时同样虚化。虚化多重由「背景雾度」（设置 `backdropHaze`，0–1，默认 0.30，写到 `--backdrop-haze`）决定：主界面 `h × 30 px`、被叠住的对话框 `h × 20 px`，对话框后的遮罩 `--scrim` 不透明度为 `h·(0.4+s)·(1−h) + 0.92·h²`（s 为玻璃浓度）——0 时不虚化、不遮暗，能看清下层的一切；1 时虚化 30 px、遮罩约 92%，几乎看不见下层。默认 0.30 与加入这个设置之前的固定效果一致（9 px / 6 px）。设置面板的「下一层 / 玻璃层」示意图按同样的雾度虚化下一层。对话框与拖放提示层用 `createPortal` 挂到 `body`，避免被上一层的 filter 截成局部定位容器。早期 WebView2 的透明窗口里 `backdrop-filter` 不生效（当时实机确认）；2026-11 用 WebView2 156 实测它对页面内容已经生效，但对话框仍然**不用** `backdrop-filter`：它会在折射之后再模糊一遍，把下面这条的折射完全抹掉。
- **对话框折射下层界面**（`glass/underGlass.ts`）：只有一个对话框时，在 `#root` 上套一个 SVG 滤镜：`feFlood` 中性灰 + `feImage` 位移图（按对话框的尺寸、圆角与折射参数在 2D 画布上按半分辨率生成，与着色器同一套斯涅尔公式）→ `feDisplacementMap`；色散增益大于 0 时 R / G / B 各用不同的位移量取样，再用 `feColorMatrix` 拆通道、`feComposite arithmetic` 拼回并乘回 3。先模糊（`.app-shell`）再折射（`#root`），与参考实现的顺序一致；url() 滤镜不能插值，和可过渡的模糊分在两个元素上，雾度的淡入淡出才不受影响。对话框进场动画结束（`animationend`）后才套上，尺寸变化（ResizeObserver）、窗口缩放、参数变化时重算；叠放对话框时不折射。
- **动效**：`theme.css` 统一时长（120 / 200 / 320 / 460 ms）与缓动（ease-out、ease-in、弹簧）；对话框进场缩放 + 上移 + 淡入，退场由 `Modal` 卸载时留下的静态「残影」播放淡出后自动移除；主题切换在设置面板里用 View Transition 整窗交叉淡化；按钮、开关、分段控件（`Segmented`）、滑块、设置导航指示条都有统一的弹簧过渡。`reduceMotion` 或系统「减少动画」时所有过渡缩到 1 ms（加载指示器照常转）。
- **主题与外观设置**：外观类设置（主题、背景、材质、玻璃浓度、背景雾度、液态玻璃参数）改动立即预览整个窗口（设置面板本身就是对话框，拖动雾度时身后的主界面实时变化）（`preview_appearance` 不写盘）；取消、Esc 或面板被卸载时还原到已保存的外观；只有「保存」才写盘（`save_settings` 保存后立即应用）。上次保存的外观缓存在 `localStorage`，只为避免启动时主题闪一下，以后端设置为准。
- **高级设置与即时预览**：外观页最下面的「高级设置」（可展开）按四组列出参考项目的参数（折射与色散 / 菲涅尔与高光 / 模糊、色调与阴影 / 形状，表在 `glass/params.ts` 的 `PARAM_GROUPS`，范围必须与 Rust 的表一致），带「重置为默认」。上方的即时预览（`GlassPreview`，滚动时停在顶部）用自己的 `GlassRenderer` 画一个缩小的主界面（侧栏、卡片、真实的 `.btn` 按钮）和一块可拖动（也可用方向键移动）的玻璃；内置背景下迷你面板也是 WebGL 玻璃，系统材质下是 CSS 近似、背景用渐变光斑代替桌面；渐变背景叠一层细斜纹，平滑的渐变被折射后看不出变化。
- **标题栏**：68 px 顶栏即标题栏，自绘最小化 / 最大化 / 关闭按钮，`core:window:allow-start-dragging` 拖动，双击最大化；窗口最小 960×600。侧栏展开 272 px / 收起 88 px，`Ctrl+B` 切换。
- 系统材质的透出效果只在 Windows 11 上完整；亚克力在拖动 / 缩放窗口时比云母更耗电。

## 错误提示与运行日志

- `utilities::error_text` 把 I/O 错误翻译成可操作的中文：识别权限不足、磁盘空间不足、文件被占用（含 33 锁定冲突）、路径过长；`.NET` 异常消息里带路径而 `io::Error` 通常不带，所以写入场景由调用方传目标目录。`error::CoreError::Io` 也走这里。
- 下载、同步、NCM 导入、标签读写的失败都经它转成「是什么 + 怎么办」；命令层不把英文异常抛给前端。
- 运行日志（`LogHub`）只写内存（400 条，不落盘），前端底部日志面板可收起；后端错误同时写入日志。面板收起 / 展开时在固定高度（168 px ↔ 标题行 32 px + 1 px 边框）之间过渡，列表不卸载、只被裁掉并淡出（收起时 `inert`）。收起状态与侧栏的一样只是界面偏好，记在 `localStorage`（`lp.logCollapsed`，随 WebView2 用户数据目录保存），下次启动直接按上次的样子显示。

## 线程、取消与并发

- 网络 I/O 全部 `async`（tokio）；标签读写、NCM 解密、目录扫描这类阻塞工作在调用方用 `spawn_blocking`。
- 取消：`CancelToken`（`Arc<AtomicBool>` + `Notify`），`cancellable(token, future)` 在令牌被取消时丢弃正在等待的 future，所以取消在等网络时立刻生效，不必等下一个分块。取消后返回 `cancelled: true` 与已完成的结果，临时文件被清理。
- 长任务（扫描、下载 / 同步、NCM 导入、批量）各占一个任务槽，同一时间只有一个（`busy`）；下载 / 同步用设置的快照，任务运行期间改设置不影响正在跑的任务。
- 下载 / 同步用 `run_pool`：按输入顺序、拿到并发名额才启动下一个，结果按下标归位，任务 panic 只影响那一首。
- 前端高频事件（下载进度）先写进 ref，每帧最多刷新一次界面，任务结束后以返回的汇总覆盖逐行状态（事件与命令返回值不保证先后）。

## 数据文件与兼容性

- `settings.json`：camelCase、枚举按名字、读取时键名不区分大小写；Tauri 版新增 `windowMaterial` / `glassOpacity` / `backdropHaze` / `reduceMotion` / `autoCheckUpdates` / `ignoredUpdateVersion` / `background` / `gradientPreset` / `backgroundImage` / `liquidGlass`（对象），C# 版读取时忽略未知键；某字段类型错误只让该字段回落默认（C# 会整份重置）。设置文件读取失败（权限 / 被占用）时用默认设置并在界面提示，**自动记忆**（音质、NCM 位置、编辑器来源、忽略的更新版本）只改内存不写盘，用户在设置面板里**主动保存**才写盘。
- `session.dat`：`NMMSES01` + mode（1 = DPAPI，2 = 明文）+ payload，DPAPI 用 CurrentUser 与 entropy `LunaticPrincess.Session.v1`，与 C# 版互读。
- 与 C# 版互通由集成测试 `tests/csharp_*.rs` 与夹具固化（见 AGENTS.md §9）。

## 安全边界

- Cookie 仅存于本机；日志不打印 Cookie；`get_app_state` 不含 Cookie。
- CSP：`default-src 'self'; img-src 'self' data: https:; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost`；capabilities 只授予主窗口必要的窗口操作与打开文件对话框。头像、封面 URL 在后端把 `http://` 升级为 `https://`。
- 检查更新只读 GitHub 公开接口，不带 Cookie 或账号信息；交给系统打开的地址只能是本项目的发布页（前端传来的地址在后端再校验一次）。
- 不集成「解灰」/ 跨平台替换音源等规避版权能力；下载能力以用户自身账号权益为限。

## 复用边界

`lp-core` 与外壳分离，理论上可被其他前端复用。要移植到别的平台需要替换：登录态加密（DPAPI → 平台密钥库）、已知文件夹查询、提权检测、窗口材质；HTTP 传输是 `HttpTransport` trait，可换实现。目前没有任何其他平台的实现。
