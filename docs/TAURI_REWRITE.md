# Tauri 重写计划

`GUI-Tauri` 分支正在把 Lunatic Princess **彻底重写**：业务层改用 Rust，界面改用 Tauri 2 + React + TypeScript。
C# / Avalonia 版（`src/`、`tests/`、`LunaticPrincess.sln`、`build.ps1` 的 .NET 部分）在整个重写期间**原样保留**，供逐模块对照，直到最后一个模块才删除。

## 目标

- 业务能力与 C# 版一致（登录、歌单、本地对比、下载、NCM 解密、歌词/标签/封面、在线匹配），数据文件（`settings.json`、`session.dat`）互相兼容，用户可无缝切换。
- 核心逻辑全部在平台无关的 Rust 库 `lp-core` 中，带单测；Tauri 外壳只做命令转发、窗口与插件。
- 视觉沿用现有「液态玻璃」基线（深色 / 浅色 / 跟随系统），无边框自绘标题栏。
- 仍然只面向 Windows 10/11 x64；不绕过版权与会员限制。

## 技术栈与版本

| 项 | 版本 / 说明 |
|---|---|
| Rust | 1.99.1 stable（edition 2024），需要 clippy / rustfmt |
| Tauri | crate `tauri` 2.12.5、`tauri-build` 2.7.4；npm `@tauri-apps/cli` / `@tauri-apps/api` 2.12.4 |
| 前端 | React 19.3、TypeScript 7.0、Vite 8.3、`@vitejs/plugin-react` 6.1，TS 严格模式 |
| 包管理 | pnpm 11（锁文件 `app/pnpm-lock.yaml`） |
| Rust 公共依赖 | `serde` 1、`thiserror` 2 |
| 运行环境 | WebView2、VS 2022 Build Tools（C++） |

不引入 UI 组件库（后续有需要再定）。

## 目录布局

```
app/
  package.json  pnpm-lock.yaml  vite.config.ts  tsconfig*.json  index.html
  src/                    # React + TS 前端（components / api / styles）
  Cargo.toml              # Cargo workspace：members = ["src-tauri", "crates/lp-core"]
  crates/lp-core/         # 平台无关业务库，模块与 C# Core 命名空间一一对应
  src-tauri/              # Tauri 外壳：命令、窗口、capabilities、图标；二进制名 LunaticPrincess
```

`lp-core` 模块与 C# 目录的对应关系：

| Rust 模块 | C# 目录 |
|---|---|
| `paths` | `Core/AppPaths.cs` |
| `settings` | `Core/Settings/`（`AppSettings`、`JsonSettingsStore`） |
| `models` | `Core/Models/` 中的设置用枚举，以及 `ThemeMode`、`LyricsSourceKind`（后续模块陆续补充） |
| `session` | `Core/Models/NeteaseSession.cs`、`Core/Storage/`（`ProtectedSessionStore`、`SessionAutoSaver`） |
| `auth` | 原来放在 `MainWindowViewModel` / `QrLoginViewModel` 里的登录编排（启动校验、续期、扫码落盘、退出、运行期复核） |
| `logs` | `MainWindowViewModel` 里的运行日志列表（400 条、内存） |
| `qr` | `QrLoginViewModel.RenderQrCode`（QRCoder → `qrcode` SVG） |
| `netease` | `Core/Netease/` |
| `ncm` | `Core/Ncm/` |
| `lyrics` | `Core/Lyrics/` |
| `tags` | `Core/Tags/` |
| `library` | `Core/Library/` |
| `download` | `Core/Download/` |
| `sync` | `Core/Sync/` |
| `online` | `Core/Online/` |
| `error` | `Core/Utilities/ErrorText.cs` |

构建产物（`app/target`、`app/dist`、`app/node_modules`、`app/src-tauri/gen/schemas`）都已 gitignore。

## 构建与测试

```powershell
.\build.ps1 -Tauri          # pnpm install → cargo fmt --check → clippy -D warnings → cargo test → pnpm tauri build --no-bundle
.\build.ps1 -Tauri -Test    # 只跑检查与测试，不打包
```

产物：`app\target\release\LunaticPrincess.exe`（单个 exe，前端已内嵌）。安装包 / zip 留到模块 12。

手动命令（在 `app/` 下）：

```powershell
pnpm install
cargo fmt --check; cargo clippy --workspace -- -D warnings; cargo test --workspace
pnpm tauri dev       # 开发（Vite 固定端口 1420）
pnpm build           # 只构建前端（tsc + vite）
```

开发注意事项：

- 新开的 shell 若找不到 `cargo` / `pnpm`，先刷新 PATH：`$env:Path = [Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')`。
- **仓库位于 Edge 下载文件夹（低完整性级别）时的行为**：见下文「已知环境问题」。构建脚本会把 `TMP` 指到工作区 `.tmp`，原因相同。
- 窗口背景：Tauri 窗口为透明无边框，Rust 侧在 `setup` 中给窗口铺 Mica，失败回退 Acrylic；前端 `body` 保持透明，由半透明 token 叠加。

## 模块清单与进度

| # | 模块 | 内容 | 状态 |
|---|---|---|---|
| 1 | 项目骨架 | Cargo workspace、`lp-core` 空骨架、Tauri 外壳、自绘标题栏、主题 token、`build.ps1 -Tauri` | 完成 |
| 2 | 基础层 | 数据目录解析、`settings.json`、DPAPI `session.dat`（与 C# 版兼容） | 完成 |
| 3 | 登录 | weapi 加密、HTTP、Cookie 合并与续期、扫码登录 UI | 完成（续期效果待真机长期观察） |
| 4 | 歌单 | 歌单列表与详情（trackIds + 分批 song/detail） | 待做 |
| 5 | 歌词解析 | LRC / 增强 LRC / YRC / QRC（3DES 变体）/ KRC | 待做 |
| 6 | 标签读写 | MP3 / FLAC / M4A / OGG 标签、封面、歌词写入 | 待做 |
| 7 | NCM 解密与导入 | `.ncm` 解密、元数据与封面回写、导入窗口 | 待做 |
| 8 | 本地曲库与对比 | 曲库扫描、与歌单一对一匹配 | 待做 |
| 9 | 下载与同步 | 九档音质取链下载、一键同步编排 | 待做 |
| 10 | 在线匹配 | 网易云 / QQ / 酷狗 / LRCLIB、歌词与标签编辑器、批量匹配 | 待做 |
| 11 | 设置与外观 | 设置窗口、主题切换、帧率等 | 待做 |
| 12 | CI 打包、文档、删除 C# | 安装包 / zip、GitHub Actions、更新 AGENTS.md 与 README、删除 C# 代码 | 待做 |

## 模块 3（登录）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `netease::crypto` | `WeapiCrypto` | AES-128-CBC 双层加密 + 无填充 RSA（`num-bigint` 模幂）；C# 测试里的两组独立向量（Node.js 生成）原样搬来，结果逐字节一致。 |
| `netease::cookie_merger` | `CookieMerger` | 按名覆盖、`Max-Age` 优先于 `Expires`、空值不覆盖、保持顺序；Cookie 日期按 RFC 6265 §5.1.1 手写解析（支持 `01-Jan-1970` 形式、两位年份、asctime），不引入日期库。15 个 C# 用例全部移植。 |
| `netease::http` | `HttpClient` | `HttpTransport` trait（`BoxFuture`，dyn 兼容）；生产实现 `ReqwestTransport`：`reqwest 0.13` + `native-tls`（Windows 即 Schannel，系统证书与系统代理），不启用 cookie 容器；测试用 `test_util::FakeTransport`（按序返回、记录请求、可挂起响应制造“在途”窗口），**测试完全不联网**。 |
| `netease::client` | `NeteaseApiClient.cs` | 会话 + 会话代数（`SetSession` 后在途旧请求的 Set-Cookie 不再合并）；`post_weapi` / `get_json` / 裸表单；独立 20 秒超时（`tokio::time::timeout`，与传输层超时无关）；合并后会丢 `MUSIC_U` 时忽略；变化时同步通知订阅者（回调里 panic 会被隔离）；`build_playback_cookie_header` 去重；`NeteaseError`（`code` / `is_http_status` / `is_not_logged_in()` 仅业务 301 / `is_transient_network()` / `user_message()`）。 |
| `netease::login` | `NeteaseApiClient.Login.cs` | `create_qr_login`（桌面端 `type=3`，任何失败降级 web `type=1`）、`poll_qr_login`（801 / 802 / 803 / 800 / 其他，轮询回到出码时的 host 与 type）、803 后合并 Set-Cookie 与响应体 cookie、`__csrf_token` → `__csrf` 别名、读取资料失败仍保留 Cookie 登录态、`get_current_user`、`refresh_login`（新增 `try_refresh_login` 带出失败原因）。 |
| `session::SessionAutoSaver` | `SessionAutoSaver` | 订阅客户端会话变化；写入时取**当前**会话、连续变化合并、已退出则跳过、`flush()` 排空；写盘在 `spawn_blocking` 里串行执行。存储抽象成 `SessionStorage` trait（`SessionStore` 是文件实现，测试用 `RecordingStore`，可用闸门卡住写入来构造竞争）。 |
| `auth::LoginManager` | `MainWindowViewModel` / `QrLoginViewModel` 里的登录逻辑 | `restore`（同步读 `session.dat`）、`validate`（启动校验）、`qr_start` / `qr_poll`、`logout`、`handle_runtime_auth_error`、`ensure_keepalive`、`shutdown`；见下文不变量。 |
| `logs::LogHub` | 日志列表 | 最多 400 条的内存环形缓冲 + 实时监听器；条目 `id` 单调递增，前端启动时先监听、再 `get_logs`、按 `id` 去重，不漏也不重复。 |
| `qr` | QRCoder | `qrcode` 0.14（只开 `svg`），纠错级别 Q，白底黑码含静区，深色主题下也能扫。前端用 `data:image/svg+xml` 的 `<img>` 显示，不需要二维码库。 |

### 登录不变量（与 C# v1.3 一致，均有单测）

- **「登录失效」只认接口业务 `code == 301`**，或 `account/get` 返回 200 但 account / profile 为空；HTTP 401 / 302 / 5xx、406、-460、超时、网络错误一律只记“暂时无法验证登录状态，已保留登录”，不清除。
- 判定失效前先 `refresh_login` 再复核一次，仍明确未登录才清除（`301 → 续期成功 → 恢复`、`301 → 仍未登录 → 清除`、`复核时网络错误 → 保留` 都有用例）。
- 清除登录态顺序：先 `set_session(None)`、再 `flush` 自动落盘、最后删 `session.dat`，已排队的落盘因“当前已无会话”被跳过，不会写回（用写盘闸门构造了竞争场景）。
- 运行期 301 复核同一时间只做一次（RAII 复位标志，复核被取消也会放开）。
- 定时续期默认 12 小时，间隔可注入（单测用暂停时钟推进）；首次在“启动校验”或“扫码成功”时启动，幂等，可停止、可重启。
- 扫码确认时先落盘再让登录态生效；保存失败则不留“内存已登录、重启就丢”的半截状态。

### 与 C# 行为的差异（有意为之）

- 扫码轮询遇到**暂时性网络错误**（超时 / 连接失败）时，前端最多连续容忍 3 次再提示刷新；C# 把它们都包成 `NeteaseApiException`，一次失败就报错。
- 803 拿到 Cookie 却没读到资料时，Rust 在后台补取一次账号资料并刷新界面。
- 续期失败的日志带原因：`登录态续期未成功（续期登录状态失败：接口返回 code=400），将继续使用当前登录状态`。
- JSON 用 `serde_json::Value`（键按字母序），C# 是插入序；服务端不关心键序，`weapi` 业务 JSON 的解密结果只在内容上与 C# 相同。

### Tauri 外壳与前端

- 命令：`qr_start`（key + SVG + 有效期）、`qr_poll`（错误折叠进返回值：`state` 为 `waiting` / `scanned` / `expired` / `failed` / `success` / `error`，`transient` 标记可重试）、`login_status`、`validate_login`、`logout`、`recheck_login`（给后续模块在 301 时调用）、`get_logs`、既有的 `get_app_state`（会话摘要新增完整账号信息，仍不含 Cookie）。
- 事件：`app:log`（新日志）、`auth:state`（登录成功 / 退出 / 失效 / 资料刷新，载荷是账号摘要）。应用退出（`RunEvent::Exit`）时停掉续期任务并 `flush` 自动落盘。
- 前端：标题栏右侧账号区（未登录「扫码登录」；已登录圆形头像 + 昵称 + 「黑胶 VIP」/「网易云用户」+「退出登录」，退出前有确认对话框）、玻璃卡片风格的模态扫码对话框（二维码、状态文字、过期 / 失败时「刷新二维码」、成功后自动关闭、约 2 秒轮询、关闭对话框即停止轮询、Esc / 点遮罩关闭）、底部日志面板（400 条，可收起，新日志自动滚动）。所有可交互控件都有中文 `aria-label`。头像 `http://` 在后端升级为 `https://`（CSP 不允许混合内容），前端追加 `?param=64y64`。

### 新增依赖

| crate | 版本 | 用途 |
|---|---|---|
| `tokio` | 1.54（`rt` `sync` `time`；测试加 `macros` `test-util`） | 超时、定时续期、`watch`、`spawn_blocking` |
| `reqwest` | 0.13.5（`default-features = false`，`native-tls` `http2` `system-proxy` `form` `gzip` `deflate`） | 生产 HTTP |
| `aes` / `cbc` | 0.9.3 / 0.2.1（`alloc`） | weapi AES-128-CBC |
| `num-bigint` | 0.5.1 | 无填充 RSA 模幂 |
| `base64` / `rand` | 0.23.1 / 0.10.3 | Base64、随机 secret |
| `qrcode` | 0.14.1（`svg`） | 二维码 SVG |

### 验证

- 单测：`lp-core` lib 199 个（模块 3 新增约 120 个，含 `weapi` 向量、`CookieMerger`、客户端 / 扫码 / 续期、`SessionAutoSaver`、`LoginManager` 编排、日志、二维码），外壳 7 个，`csharp_compat` 5 个；`.\build.ps1 -Test` C# 仍 404/404；`cargo fmt --check`、`clippy -D warnings`、`pnpm build`、`.\build.ps1 -Tauri` 全部通过；`lp-core` 连跑 15 次无抖动。
- 联网探针（一次性，已删除）：`unikey`（桌面端出码）拿到 36 位 key，轮询返回 801（`等待扫码`），二维码 SVG 生成成功。
- 实机（exe 复制到 `D:\claude\`、`LUNATIC_PRINCESS_DATA_DIR=D:\claude\lp-data`，未触碰真实登录态；测完已清理）：「扫码登录」按钮、对话框、二维码与「请使用网易云音乐 App 扫码」显示正常。测试期间有真人用手机扫了码，因此顺带走通了**真实的 803 确认流程**：登录成功、头像 / 昵称 / 「黑胶 VIP」显示、日志「登录成功」；重启后 DPAPI 的 `session.dat` 恢复登录、启动校验「账号信息同步完成」；退出登录确认对话框 → `session.dat` 被删除、回到「扫码登录」。

### 已知问题与待真机观察

- **`/weapi/login/token/refresh` 对这次桌面端扫码得到的会话返回 `code=400`**（日志：`登录态续期未成功（续期登录状态失败：接口返回 code=400）`）。请求与 C# 完全一致，所以 C# 版大概率同样如此；登录态本身有效（`account/get` 成功）。这正是 AGENTS.md 里“续期效果需真机长期观察”的那一点：目前能确认的是“Set-Cookie 合并 + 落盘”这条路径有单测，但服务端是否真的靠 refresh 下发新 `MUSIC_U` 还没有实证。若长期观察发现 `MUSIC_U` 到期掉线，需要改查 `eapi` 续期端点。
- 需要用户真机验证的点：**①** 长时间运行（超过 12 小时 / 数天）后登录是否保持；**②** 真正的 301（`MUSIC_U` 被服务端作废）时“先续期再复核、仍未登录才清除”的体验与日志；**③** 对话框关闭后确实不再发轮询请求（实现上由 `cancelled` 标志保证，未做网络层抓包）。
- 前端没有自动化测试（与 C# 的 ViewModel 一样靠实机验证）。

## 模块 2（基础层）实现记录

### 内容

| Rust | 对照 C# | 要点 |
|---|---|---|
| `paths` | `AppPaths.cs` | 数据目录：环境变量（新名优先，空白再看旧名）→ 程序目录下已有 `data\`（便携）→ `%LOCALAPPDATA%\LunaticPrincess`（只有旧目录 `NeteaseMusicManager` 时整体迁移）→ 程序目录 `data\` → `%TEMP%\LunaticPrincess`；每个候选都做真实写入探测，不可写就降级。默认下载目录同理（便携 `data\Music` → `%MyMusic%\LunaticPrincess`，旧目录已存在则沿用 → `data\Music` → `%TEMP%`）。环境变量、程序目录、已知文件夹收拢成 `PathEnv`，解析逻辑可用临时目录完整单测；已知文件夹用 `SHGetKnownFolderPath`。 |
| `settings` / `models` | `AppSettings.cs`、`JsonSettingsStore.cs` | 21 个字段、默认值、`normalize()`（并发 clamp 1–8，阈值银行家舍入到 2 位并 clamp 0.30–1.00，NCM 文件夹去重等）；camelCase、枚举按名字、键名读取时不区分大小写；旧值迁移 `Higher`→`ExHigh`、未知音质丢弃；临时文件 + rename 原子写入。 |
| `session` | `NeteaseSession.cs`、`ProtectedSessionStore.cs` | 文件头 `NMMSES01` + mode（1=DPAPI，2=明文）+ payload；DPAPI 为 CurrentUser、entropy `LunaticPrincess.Session.v1`，与 C# 完全一致。`DataProtector` trait 抽象加密，单测用 XOR 假实现，Windows 上另有真实 DPAPI 往返测试。`load` 区分“文件不存在 / 没有可用 Cookie”（`Ok(None)`）与“损坏、无法解密、无法解析、读取失败”（`Err` + `last_load_failure()`，原因文案与 C# 相同）。 |
| `error` | `ErrorText.cs` | 新增 `Json` / `Dpapi` / `Path` / `Session` 变体；IO 错误按权限不足、占用、磁盘已满、路径过长给出中文提示。 |

Windows API 选用 `windows-sys` 0.61 而不是 `windows`：只需要 `CryptProtectData` / `CryptUnprotectData` / `LocalFree` / `SHGetKnownFolderPath` 几个 C 接口，`windows-sys` 是纯 FFI 声明、没有 COM 包装，并且 Tauri 已经依赖同一版本，不增加编译量。

Tauri 侧：启动时解析路径、加载设置与登录态并放入托管状态 `AppState`；命令 `get_app_state` 返回数据目录、便携模式、设置摘要与登录摘要（只有是否登录与昵称，**不含 Cookie**），读取失败的中文原因一并返回，前端占位卡片显示「数据目录 / 便携模式 / 已保存登录」。

### 与 C# 行为的差异（有意为之）

- 设置文件里某个字段类型错误（例如枚举写成未知名字）时，C# 会整份重置为默认值，Rust 只让该字段回落到默认，其余保持。文件整体无法解析 / 不是对象时两边都用默认设置。
- 设置读取遇到“不存在 / 损坏”以外的 IO 错误（权限不足、被占用）时，Rust 返回 `Err`，外壳改用默认设置并在界面提示；C# 对 `IOException` 静默使用默认值。
- `NeteaseSession.logged_in_at` 以 ISO 8601 字符串原样保存（C# 的 `DateTimeOffset` 序列化形式），无损往返；Rust 自己生成时使用 UTC（`+00:00`），C# 可直接解析。

### 兼容性实证

用临时 C# 探针（调用现有 `ProtectedSessionStore` / `JsonSettingsStore`，数据全是假的：`MUSIC_U=fake; __csrf=abc`、昵称「测试」，探针放在仓库外并已删除）与 Rust 互读：

- C# 写 → Rust 读：`settings.json`（21 个字段全部取非默认值）与默认设置逐字段一致；DPAPI 的 `session.dat` 读出的 cookie / 昵称 / userId / vipType / 头像 / `loggedInAt` 完全一致。
- Rust 写 → C# 读：`settings.json` 21 个字段正确；`session.dat`（含 Rust 自己生成的默认 `loggedInAt`）被 C# 正常读取，`LastLoadFailure` 为空。
- 无隐私的 C# 输出已作为夹具提交在 `app/crates/lp-core/tests/fixtures/`（`settings.csharp-*.json`、`session.csharp.json`、`session.csharp-mode2.dat`），`tests/csharp_compat.rs` 校验字段、键顺序与往返。DPAPI 文件绑定 Windows 用户，无法作为夹具，所以互读 DPAPI 只记录在这里。

### 已知环境问题：仓库位于 Edge 下载文件夹时 WebView2 创建失败

现象：在 `D:\edge浏览器下载\DSH\…` 下直接运行 `LunaticPrincess.exe`，`WebView2 error: 0x800700AA`（请求的资源正在使用中）；复制到 `D:\claude\…` 等其他位置正常。

**根因不是中文路径，也不是代码页**。`D:\edge浏览器下载` 是 Edge 的默认下载文件夹，带有 **Low Mandatory Level（低完整性级别）** 的强制标签，并向下继承给其中的每个文件。进程的完整性级别取“父进程”与“可执行文件”中较低者，所以放在该目录树下的 exe（包括 `cargo test` 生成的测试程序、仓库里的 `.dotnet\dotnet.exe`）都以低完整性运行，不能写入普通完整性的位置（如 `%LOCALAPPDATA%`、`%TEMP%`、`D:\claude\`），报 `os error 5`。Tauri 默认把 WebView2 用户数据目录放在 `%LOCALAPPDATA%\<identifier>\EBWebView`，低完整性进程无法使用，于是创建环境失败。用同样的目录名放到别的位置（`D:\claude\载`、`D:\claude\edge浏览器下载`、很深的路径）都正常，只有直接位于该文件夹下才失败；探针程序复制到该目录树下也复现了“无法在 `D:\claude` 建目录”。

修复：
1. 主窗口改为在 `setup` 里用 `WebviewWindowBuilder::from_config`（`tauri.conf.json` 中 `create: false`）创建，并显式 `data_directory(<数据目录>\webview)`，窗口尺寸、无边框、透明、Mica→Acrylic 回退不变。
2. 数据目录解析本身带写入探测：低完整性进程写不进 `%LOCALAPPDATA%` 时会自动降级到程序目录下的 `data\`（便携模式），而该目录同样位于低完整性目录树内、可写，因此 `webview\` 也可写。
3. `build.ps1 -Tauri` 把 `TMP` / `TEMP` 指到工作区 `.tmp`（与 .NET 流程一致），否则测试程序在该仓库路径下无法创建临时目录。

验证：在仓库原路径运行 `app\target\release\LunaticPrincess.exe`，窗口与卡片正常显示（数据目录落在程序目录 `data\` 或显式指定的、低完整性可写的目录）。**副作用**：在这种位置运行时，登录态与设置会落在程序目录 `data\` 而不是 `%LOCALAPPDATA%\LunaticPrincess`，与用普通方式启动时不是同一份；若要共用，请把仓库移出 Edge 下载文件夹，或把 exe 复制到普通位置运行。根治办法是移走仓库或去掉该文件夹的低完整性标签（属于系统安全设置，本项目不代为修改）。
