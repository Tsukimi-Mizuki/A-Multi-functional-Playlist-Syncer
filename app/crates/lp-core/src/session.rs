//! 登录态持久化（`session.dat`，Windows DPAPI 加密）。
//!
//! 对应 C# 的 `Core/Models/NeteaseSession.cs`、`Core/Storage/ISessionStore.cs` 与
//! `Core/Storage/ProtectedSessionStore.cs`，文件格式与 C# 版**逐字节兼容**，两边写出的文件可以互相读取。
//!
//! ```text
//! +------------------+-----------+--------------------------------------------+
//! | "NMMSES01" (8 B) | mode (1B) | payload                                    |
//! +------------------+-----------+--------------------------------------------+
//!   mode = 1：payload 是 DPAPI(CurrentUser, entropy = "LunaticPrincess.Session.v1") 加密后的 JSON
//!   mode = 2：payload 是明文 JSON（C# 在非 Windows 平台的降级写法，Rust 在无加密器时沿用）
//! ```
//!
//! 加密能力抽象成 [`DataProtector`]：生产环境在 Windows 上用 [`DpapiProtector`]，单测用假实现。
//! `SessionAutoSaver`（Cookie 合并后串行落盘）依赖 API 客户端，留到模块 3。

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::settings::write_atomic;

/// 文件头魔数（沿用旧名 NeteaseMusicManager 的缩写，保持与 C# 版兼容）。
pub const FILE_MAGIC: &[u8; 8] = b"NMMSES01";
/// DPAPI 加密保存的标记。
pub const MODE_PROTECTED: u8 = 1;
/// 明文保存的标记（无加密器时的降级）。
pub const MODE_PLAIN: u8 = 2;
/// DPAPI 的附加熵，必须与 C# `ProtectedSessionStore.Entropy` 完全一致，否则互相解不开。
pub const DPAPI_ENTROPY: &[u8] = b"LunaticPrincess.Session.v1";

/// 登录态。Cookie 是调用网易云 weapi 的唯一凭证，必须持久化。
///
/// JSON 字段名与 C# 的 `NeteaseSession`（`[JsonPropertyName]`）完全一致。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NeteaseSession {
    /// 完整 Cookie 串（`MUSIC_U=…; __csrf=…`）。
    pub cookie: String,
    /// 昵称。
    pub nickname: Option<String>,
    /// 网易云用户 ID。
    pub user_id: i64,
    /// 会员类型。
    pub vip_type: i32,
    /// 头像地址。
    pub avatar_url: Option<String>,
    /// 登录时间，ISO 8601 文本（C# `DateTimeOffset` 的序列化形式），原样保存以便无损往返。
    pub logged_in_at: String,
}

impl Default for NeteaseSession {
    fn default() -> Self {
        Self {
            cookie: String::new(),
            nickname: None,
            user_id: 0,
            vip_type: 0,
            avatar_url: None,
            logged_in_at: now_iso8601_utc(),
        }
    }
}

impl NeteaseSession {
    /// 是否带有可用的 Cookie。
    pub fn is_logged_in(&self) -> bool {
        !self.cookie.trim().is_empty()
    }

    /// 界面显示用的名字，缺失时显示“网易云用户”。
    pub fn display_name(&self) -> &str {
        match self.nickname.as_deref() {
            Some(name) if !name.trim().is_empty() => name,
            _ => "网易云用户",
        }
    }

    /// `__csrf` Cookie 的值，没有时为空串。
    pub fn csrf_token(&self) -> String {
        self.cookie_value("__csrf").unwrap_or_default()
    }

    /// 取指定名字的 Cookie 值（名字不区分大小写，与 C# 的 `GetCookieValue` 一致）。
    pub fn cookie_value(&self, name: &str) -> Option<String> {
        if !self.is_logged_in() {
            return None;
        }
        self.cookie
            .split(';')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .find_map(|part| {
                let index = part.find('=').filter(|&index| index > 0)?;
                part[..index]
                    .eq_ignore_ascii_case(name)
                    .then(|| part[index + 1..].to_owned())
            })
    }
}

/// 当前 UTC 时间的 ISO 8601 文本（`2026-11-02T10:25:00.1234567+00:00`），C# 的 `DateTimeOffset` 可直接解析。
fn now_iso8601_utc() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_unix_time(elapsed.as_secs() as i64, elapsed.subsec_nanos())
}

fn format_unix_time(seconds: i64, nanos: u32) -> String {
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    // Howard Hinnant 的 civil_from_days 算法。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:07}+00:00",
        second_of_day / 3_600,
        second_of_day % 3_600 / 60,
        second_of_day % 60,
        nanos / 100,
    )
}

/// 对一段字节做“只有当前用户能解开”的加解密。
pub trait DataProtector: Send + Sync {
    /// 加密。
    fn protect(&self, plain: &[u8]) -> CoreResult<Vec<u8>>;
    /// 解密；数据损坏或来自其他账号 / 电脑时返回错误。
    fn unprotect(&self, protected: &[u8]) -> CoreResult<Vec<u8>>;
}

#[cfg(windows)]
pub use dpapi::DpapiProtector;

/// Windows 数据保护接口（DPAPI）。
///
/// 选用 `windows-sys` 而不是 `windows`：这里只需要 `CryptProtectData` / `CryptUnprotectData` / `LocalFree`
/// 三个 C 接口，`windows-sys` 是纯 FFI 声明（无 COM 包装、无运行时开销），并且 Tauri 已经依赖同一版本，
/// 不会多编译任何东西。
#[cfg(windows)]
mod dpapi {
    use super::{CoreError, CoreResult, DPAPI_ENTROPY, DataProtector};
    use windows_sys::Win32::Foundation::{GetLastError, LocalFree};
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    /// DPAPI（作用域 CurrentUser）加密器，附加熵固定为 [`DPAPI_ENTROPY`]。
    #[derive(Debug, Clone, Copy, Default)]
    pub struct DpapiProtector;

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr().cast_mut(),
        }
    }

    /// 复制系统分配的输出缓冲并释放它。
    ///
    /// # Safety
    /// `out` 必须是 `CryptProtectData` / `CryptUnprotectData` 成功返回的输出 blob。
    unsafe fn take_blob(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: 调用约定保证 `pbData` 指向 `cbData` 字节的 LocalAlloc 内存。
        unsafe {
            let bytes = if out.pbData.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec()
            };
            if !out.pbData.is_null() {
                LocalFree(out.pbData.cast());
            }
            bytes
        }
    }

    impl DataProtector for DpapiProtector {
        fn protect(&self, plain: &[u8]) -> CoreResult<Vec<u8>> {
            let input = blob(plain);
            let entropy = blob(DPAPI_ENTROPY);
            let mut output = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            // SAFETY: 三个 blob 都指向存活的缓冲；描述串 / 保留参数 / 提示结构传空；输出由 take_blob 释放。
            let ok = unsafe {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    &entropy,
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            };
            if ok == 0 {
                // SAFETY: 紧接着读取线程的最近错误码。
                let code = unsafe { GetLastError() };
                return Err(CoreError::Dpapi(format!(
                    "加密登录信息失败（错误码 {code}）"
                )));
            }
            // SAFETY: 调用成功，`output` 由系统分配。
            Ok(unsafe { take_blob(output) })
        }

        fn unprotect(&self, protected: &[u8]) -> CoreResult<Vec<u8>> {
            let input = blob(protected);
            let entropy = blob(DPAPI_ENTROPY);
            let mut output = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            // SAFETY: 同 `protect`。
            let ok = unsafe {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    &entropy,
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            };
            if ok == 0 {
                // SAFETY: 紧接着读取线程的最近错误码。
                let code = unsafe { GetLastError() };
                return Err(CoreError::Dpapi(format!(
                    "解密登录信息失败（错误码 {code}）"
                )));
            }
            // SAFETY: 调用成功，`output` 由系统分配。
            Ok(unsafe { take_blob(output) })
        }
    }
}

const FAIL_FORMAT: &str = "登录文件格式不正确或已损坏";
const FAIL_NO_DECRYPTOR: &str = "登录文件由 Windows 加密保存，当前系统无法解密";
const FAIL_DECRYPT: &str = "登录文件无法解密（可能来自其他 Windows 账号或其他电脑）";
const FAIL_PARSE: &str = "登录文件内容无法解析";

/// `session.dat` 的读写。
///
/// 对应 C# 的 `ProtectedSessionStore`（含 `ISessionStore.LastLoadFailure` 语义）。
pub struct SessionStore {
    file_path: PathBuf,
    protector: Option<Box<dyn DataProtector>>,
    /// 最近一次 [`load`](Self::load) 的失败原因；同时充当串行化读写的互斥锁（对应 C# 的 `SemaphoreSlim`）。
    last_failure: Mutex<Option<String>>,
}

impl std::fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionStore")
            .field("file_path", &self.file_path)
            .field("encrypted", &self.protector.is_some())
            .finish()
    }
}

impl SessionStore {
    /// `protector` 为 `None` 时以明文（mode 2）保存，并且无法读取 DPAPI（mode 1）文件。
    pub fn new(file_path: impl Into<PathBuf>, protector: Option<Box<dyn DataProtector>>) -> Self {
        Self {
            file_path: file_path.into(),
            protector,
            last_failure: Mutex::new(None),
        }
    }

    /// 使用当前平台的系统加密器：Windows 上是 DPAPI，其他平台没有（明文降级）。
    pub fn with_system_protector(file_path: impl Into<PathBuf>) -> Self {
        #[cfg(windows)]
        let protector: Option<Box<dyn DataProtector>> = Some(Box::new(DpapiProtector));
        #[cfg(not(windows))]
        let protector: Option<Box<dyn DataProtector>> = None;
        Self::new(file_path, protector)
    }

    /// 登录文件路径。
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.last_failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 最近一次 [`load`](Self::load) 失败的原因（中文）。
    ///
    /// 文件不存在属于正常的“未登录”，此时为 `None`；文件损坏、无法解密或读取失败时给出说明。
    pub fn last_load_failure(&self) -> Option<String> {
        self.lock().clone()
    }

    /// 读取登录态。
    ///
    /// - 文件不存在：`Ok(None)`，`last_load_failure()` 为 `None`；
    /// - 文件里没有可用 Cookie（未登录状态）：`Ok(None)`；
    /// - 读取 / 解密 / 解析失败：`Err(CoreError::Session(中文原因))`，同时记录到 `last_load_failure()`。
    pub fn load(&self) -> CoreResult<Option<NeteaseSession>> {
        let mut failure = self.lock();
        *failure = None;
        match self.read_session() {
            Ok(session) => Ok(session),
            Err(reason) => {
                *failure = Some(reason.clone());
                Err(CoreError::Session(reason))
            }
        }
    }

    fn read_session(&self) -> Result<Option<NeteaseSession>, String> {
        let raw = match fs::read(&self.file_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) if error.kind() == ErrorKind::PermissionDenied => {
                return Err(format!("没有权限读取登录文件：{error}"));
            }
            Err(error) => return Err(format!("读取登录文件失败：{error}")),
        };

        let header = FILE_MAGIC.len();
        if raw.len() <= header + 1 || &raw[..header] != FILE_MAGIC {
            return Err(FAIL_FORMAT.to_owned());
        }

        let payload = &raw[header + 1..];
        let json = if raw[header] == MODE_PROTECTED {
            let protector = self
                .protector
                .as_deref()
                .ok_or_else(|| FAIL_NO_DECRYPTOR.to_owned())?;
            protector
                .unprotect(payload)
                .map_err(|_| FAIL_DECRYPT.to_owned())?
        } else {
            payload.to_vec()
        };

        let session: Option<NeteaseSession> =
            serde_json::from_slice(&json).map_err(|_| FAIL_PARSE.to_owned())?;
        Ok(session.filter(NeteaseSession::is_logged_in))
    }

    /// 保存登录态（先写 `.tmp` 再原子替换）。
    pub fn save(&self, session: &NeteaseSession) -> CoreResult<()> {
        let _guard = self.lock();
        let json = serde_json::to_vec(session)?;
        let (mode, payload) = match self.protector.as_deref() {
            Some(protector) => (MODE_PROTECTED, protector.protect(&json)?),
            None => (MODE_PLAIN, json),
        };

        let mut bytes = Vec::with_capacity(FILE_MAGIC.len() + 1 + payload.len());
        bytes.extend_from_slice(FILE_MAGIC);
        bytes.push(mode);
        bytes.extend_from_slice(&payload);
        write_atomic(&self.file_path, &bytes)
    }

    /// 删除登录文件及残留的临时文件；文件本来就不存在不算错误。
    pub fn clear(&self) -> CoreResult<()> {
        let _guard = self.lock();
        remove_if_exists(&self.file_path)?;
        let mut temp = self.file_path.as_os_str().to_owned();
        temp.push(".tmp");
        remove_if_exists(Path::new(&temp))
    }
}

fn remove_if_exists(path: &Path) -> CoreResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// 假加密器：逐字节异或，足以验证“文件里不是明文”和“读写走了加密器”。
    struct XorProtector;

    impl DataProtector for XorProtector {
        fn protect(&self, plain: &[u8]) -> CoreResult<Vec<u8>> {
            Ok(plain.iter().map(|b| b ^ 0x5a).collect())
        }

        fn unprotect(&self, protected: &[u8]) -> CoreResult<Vec<u8>> {
            self.protect(protected)
        }
    }

    /// 解密永远失败，模拟来自其他 Windows 账号的文件。
    struct RefusingProtector;

    impl DataProtector for RefusingProtector {
        fn protect(&self, plain: &[u8]) -> CoreResult<Vec<u8>> {
            Ok(plain.to_vec())
        }

        fn unprotect(&self, _protected: &[u8]) -> CoreResult<Vec<u8>> {
            Err(CoreError::Dpapi("拒绝".to_owned()))
        }
    }

    fn store_in(dir: &TempDir) -> SessionStore {
        SessionStore::new(dir.path().join("session.dat"), Some(Box::new(XorProtector)))
    }

    fn sample() -> NeteaseSession {
        NeteaseSession {
            cookie: "MUSIC_U=secret-value; __csrf=csrf-value; os=pc".to_owned(),
            nickname: Some("测试用户".to_owned()),
            user_id: 123_456_789,
            vip_type: 11,
            avatar_url: Some("https://example.invalid/avatar.jpg".to_owned()),
            logged_in_at: "2026-11-02T18:25:00.1234567+08:00".to_owned(),
        }
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    #[test]
    fn save_load_clear_round_trips_the_session() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let session = sample();

        store.save(&session).unwrap();

        let raw = fs::read(store.file_path()).unwrap();
        assert_eq!(&raw[..8], b"NMMSES01");
        assert_eq!(raw[8], MODE_PROTECTED);
        assert!(!contains(&raw, b"MUSIC_U=secret-value"));
        assert!(!dir.path().join("session.dat.tmp").exists());

        let loaded = store.load().unwrap().expect("应读回登录态");
        assert_eq!(loaded, session);
        assert_eq!(loaded.csrf_token(), "csrf-value");
        assert!(loaded.is_logged_in());
        assert!(store.last_load_failure().is_none());

        store.clear().unwrap();
        assert!(!store.file_path().exists());
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn missing_file_is_not_a_failure() {
        let dir = TempDir::new();
        let store = store_in(&dir);

        assert!(store.load().unwrap().is_none());
        assert!(store.last_load_failure().is_none());
    }

    #[test]
    fn corrupt_files_are_reported_with_a_reason() {
        let dir = TempDir::new();
        let store = store_in(&dir);

        for content in [
            b"not a session".to_vec(),
            b"NMMSES01".to_vec(),
            b"NMMSES01\x01".to_vec(),
            Vec::new(),
        ] {
            fs::write(store.file_path(), &content).unwrap();
            let error = store.load().unwrap_err();
            assert_eq!(error.to_string(), "登录文件格式不正确或已损坏");
            assert_eq!(
                store.last_load_failure().as_deref(),
                Some("登录文件格式不正确或已损坏")
            );
        }
    }

    #[test]
    fn undecryptable_payload_is_reported() {
        let dir = TempDir::new();
        let store = SessionStore::new(
            dir.path().join("session.dat"),
            Some(Box::new(RefusingProtector)),
        );
        let mut bytes = b"NMMSES01\x01".to_vec();
        bytes.extend_from_slice(&[0u8; 64]);
        fs::write(store.file_path(), bytes).unwrap();

        let error = store.load().unwrap_err();

        assert!(error.to_string().contains("无法解密"));
        assert!(
            store
                .last_load_failure()
                .unwrap()
                .contains("其他 Windows 账号")
        );
    }

    #[test]
    fn protected_file_without_a_decryptor_is_reported() {
        let dir = TempDir::new();
        let path = dir.path().join("session.dat");
        let mut bytes = b"NMMSES01\x01".to_vec();
        bytes.extend_from_slice(&[1u8; 16]);
        fs::write(&path, bytes).unwrap();

        let error = SessionStore::new(&path, None).load().unwrap_err();

        assert_eq!(
            error.to_string(),
            "登录文件由 Windows 加密保存，当前系统无法解密"
        );
    }

    #[test]
    fn unparsable_payload_is_reported() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        let mut bytes = b"NMMSES01\x01".to_vec();
        bytes.extend(b"{not json".iter().map(|b| b ^ 0x5a));
        fs::write(store.file_path(), bytes).unwrap();

        assert_eq!(
            store.load().unwrap_err().to_string(),
            "登录文件内容无法解析"
        );
    }

    #[test]
    fn successful_load_clears_a_previous_failure() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        fs::write(store.file_path(), b"not a session").unwrap();
        assert!(store.load().is_err());
        assert!(store.last_load_failure().is_some());

        store
            .save(&NeteaseSession {
                cookie: "MUSIC_U=abc".to_owned(),
                ..Default::default()
            })
            .unwrap();

        assert!(store.load().unwrap().is_some());
        assert!(store.last_load_failure().is_none());
    }

    #[test]
    fn plain_mode_is_used_without_a_protector() {
        let dir = TempDir::new();
        let store = SessionStore::new(dir.path().join("session.dat"), None);
        store.save(&sample()).unwrap();

        let raw = fs::read(store.file_path()).unwrap();
        assert_eq!(raw[8], MODE_PLAIN);
        assert!(contains(&raw, b"MUSIC_U=secret-value"));
        assert_eq!(store.load().unwrap().unwrap(), sample());

        // 带加密器的 store 也能读 mode 2 的文件（C# 同样不看加密器，直接当明文）。
        let with_protector = store_in(&dir);
        assert_eq!(with_protector.load().unwrap().unwrap(), sample());
    }

    #[test]
    fn session_without_cookie_counts_as_logged_out_not_as_failure() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        store
            .save(&NeteaseSession {
                cookie: "   ".to_owned(),
                ..sample()
            })
            .unwrap();

        assert!(store.load().unwrap().is_none());
        assert!(store.last_load_failure().is_none());
    }

    #[test]
    fn json_null_payload_counts_as_logged_out() {
        let dir = TempDir::new();
        let store = SessionStore::new(dir.path().join("session.dat"), None);
        let mut bytes = b"NMMSES01\x02".to_vec();
        bytes.extend_from_slice(b"null");
        fs::write(store.file_path(), bytes).unwrap();

        assert!(store.load().unwrap().is_none());
        assert!(store.last_load_failure().is_none());
    }

    #[test]
    fn clear_removes_leftover_temp_file_and_tolerates_absence() {
        let dir = TempDir::new();
        let store = store_in(&dir);
        store.clear().unwrap();

        fs::write(dir.path().join("session.dat.tmp"), b"half").unwrap();
        store.save(&sample()).unwrap();
        fs::write(dir.path().join("session.dat.tmp"), b"half").unwrap();
        store.clear().unwrap();

        assert!(!store.file_path().exists());
        assert!(!dir.path().join("session.dat.tmp").exists());
    }

    #[test]
    fn save_creates_missing_directories() {
        let dir = TempDir::new();
        let store = SessionStore::new(dir.path().join("a").join("b").join("session.dat"), None);
        store.save(&sample()).unwrap();
        assert!(store.file_path().is_file());
    }

    #[test]
    fn json_uses_the_csharp_property_names() {
        let json: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&sample()).unwrap()).unwrap();
        let object = json.as_object().unwrap();
        let mut keys: Vec<_> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "avatarUrl",
                "cookie",
                "loggedInAt",
                "nickname",
                "userId",
                "vipType"
            ]
        );
        assert_eq!(json["userId"], 123_456_789);
        assert_eq!(json["vipType"], 11);
    }

    #[test]
    fn missing_json_fields_take_defaults_and_unknown_fields_are_ignored() {
        let parsed: NeteaseSession =
            serde_json::from_str(r#"{"cookie":"MUSIC_U=a","extra":1}"#).unwrap();
        assert_eq!(parsed.cookie, "MUSIC_U=a");
        assert_eq!(parsed.user_id, 0);
        assert!(parsed.nickname.is_none());
        assert!(parsed.logged_in_at.ends_with("+00:00"));
    }

    #[test]
    fn cookie_helpers_match_csharp_behaviour() {
        let session = NeteaseSession {
            cookie: "MUSIC_U=abc; __csrf=tok ; os=pc; =bad; novalue".to_owned(),
            ..Default::default()
        };
        assert!(session.is_logged_in());
        assert_eq!(session.cookie_value("MUSIC_U").as_deref(), Some("abc"));
        assert_eq!(session.cookie_value("__CSRF").as_deref(), Some("tok"));
        assert_eq!(session.cookie_value("missing"), None);
        assert_eq!(session.cookie_value("novalue"), None);
        assert_eq!(session.csrf_token(), "tok");
        assert_eq!(NeteaseSession::default().csrf_token(), "");
        assert_eq!(NeteaseSession::default().cookie_value("MUSIC_U"), None);
    }

    #[test]
    fn display_name_falls_back_when_nickname_is_blank() {
        let mut session = sample();
        assert_eq!(session.display_name(), "测试用户");
        session.nickname = Some("  ".to_owned());
        assert_eq!(session.display_name(), "网易云用户");
        session.nickname = None;
        assert_eq!(session.display_name(), "网易云用户");
    }

    #[test]
    fn unix_time_formatting_is_correct() {
        assert_eq!(format_unix_time(0, 0), "1970-01-01T00:00:00.0000000+00:00");
        assert_eq!(
            format_unix_time(951_782_400, 0),
            "2000-02-29T00:00:00.0000000+00:00"
        );
        assert_eq!(
            format_unix_time(1_793_611_500, 123_456_700),
            "2026-11-02T09:25:00.1234567+00:00"
        );
        assert!(now_iso8601_utc().starts_with("20"));
    }

    #[cfg(windows)]
    mod windows_only {
        use super::*;

        #[test]
        fn dpapi_round_trips_and_hides_plaintext() {
            let protector = DpapiProtector;
            let plain = b"MUSIC_U=very-secret; __csrf=abc";
            let protected = protector.protect(plain).unwrap();
            assert_ne!(protected, plain);
            assert!(!contains(&protected, b"very-secret"));
            assert_eq!(protector.unprotect(&protected).unwrap(), plain);
        }

        #[test]
        fn dpapi_rejects_garbage() {
            let error = DpapiProtector.unprotect(&[0u8; 64]).unwrap_err();
            assert!(matches!(error, CoreError::Dpapi(_)));
        }

        #[test]
        fn real_store_round_trips_through_dpapi() {
            let dir = TempDir::new();
            let store = SessionStore::with_system_protector(dir.path().join("session.dat"));
            store.save(&sample()).unwrap();

            let raw = fs::read(store.file_path()).unwrap();
            assert_eq!(raw[8], MODE_PROTECTED);
            assert!(!contains(&raw, b"secret-value"));
            assert_eq!(store.load().unwrap().unwrap(), sample());
        }

        #[test]
        fn dpapi_blob_with_foreign_entropy_cannot_be_read() {
            // 用另一个熵加密的数据（模拟别的程序 / 版本写的文件）不应被本程序解开。
            use windows_sys::Win32::Security::Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
            };
            let plain = b"hello";
            let entropy = b"some other entropy";
            let input = CRYPT_INTEGER_BLOB {
                cbData: plain.len() as u32,
                pbData: plain.as_ptr().cast_mut(),
            };
            let ent = CRYPT_INTEGER_BLOB {
                cbData: entropy.len() as u32,
                pbData: entropy.as_ptr().cast_mut(),
            };
            let mut out = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            // SAFETY: 参数指向存活缓冲，输出在拷贝后立即释放。
            let blob = unsafe {
                assert_ne!(
                    CryptProtectData(
                        &input,
                        std::ptr::null(),
                        &ent,
                        std::ptr::null(),
                        std::ptr::null(),
                        CRYPTPROTECT_UI_FORBIDDEN,
                        &mut out
                    ),
                    0
                );
                let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
                windows_sys::Win32::Foundation::LocalFree(out.pbData.cast());
                bytes
            };
            assert!(DpapiProtector.unprotect(&blob).is_err());
        }
    }
}
