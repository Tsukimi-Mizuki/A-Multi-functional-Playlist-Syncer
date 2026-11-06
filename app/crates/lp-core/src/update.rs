//! 检查 GitHub 上有没有新版本（启动时的自动检查与设置里的「立即检查」）。
//!
//! 只读 GitHub Releases 的 `releases/latest`（GitHub 保证它不含预发布与草稿），把 tag 按
//! `scripts/resolve-version.ps1` 同样的规则归一成 SemVer 再与当前版本比较。程序不下载、不安装：
//! 发现新版本后由用户在浏览器里打开发布页自己下载，所以这里也只允许打开本项目的发布页。

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::logs::LogHub;
use crate::netease::http::HttpTransport;
use crate::online::net::{HttpFailure, OnlineHttp};
use crate::utilities::CancelToken;

/// 最新正式版的接口地址（未登录 GitHub 时每个 IP 每小时 60 次，启动时查一次绰绰有余）。
pub const LATEST_RELEASE_API: &str = "https://api.github.com/repos/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases/latest";
/// 本项目的发布页；只有以它开头的地址才会交给浏览器打开。
pub const RELEASES_PAGE: &str =
    "https://github.com/Tsukimi-Mizuki/A-Multi-functional-Playlist-Syncer/releases";
/// 检查更新的独立超时。
pub const UPDATE_TIMEOUT: Duration = Duration::from_secs(15);
/// 弹窗里的更新说明最多保留这么多字符。
const MAX_NOTES_CHARS: usize = 4000;

/// SemVer 版本号（只保留比较需要的部分，构建元数据 `+...` 读取时丢弃）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    /// 预发布标识（`-rc.1` 里的 `rc`、`1`）；为空表示正式版。
    pre: Vec<String>,
}

impl Version {
    /// 解析 tag 或版本号：与 `scripts/resolve-version.ps1` 相同，接受 `v2.1`（补成 `2.1.0`）、`V2.0.0-rc.1`、
    /// 历史 tag `v.1.0.0`（去掉前导点）与 `refs/tags/` 前缀；不是 SemVer 的返回 `None`。
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix("refs/tags/").unwrap_or(text);
        let text = text
            .strip_prefix(['v', 'V'])
            .unwrap_or(text)
            .trim_start_matches('.');
        let (text, build) = match text.split_once('+') {
            Some((text, build)) => (text, Some(build)),
            None => (text, None),
        };
        if build.is_some_and(|build| !valid_identifiers(build)) {
            return None;
        }
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        if pre.is_some_and(|pre| !valid_identifiers(pre)) {
            return None;
        }

        let mut numbers = [0u64; 3];
        for (index, part) in core.split('.').enumerate() {
            let valid = !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            if !valid || index == numbers.len() {
                return None;
            }
            numbers[index] = part.parse().ok()?;
        }
        Some(Self {
            major: numbers[0],
            minor: numbers[1],
            patch: numbers[2],
            pre: pre
                .map(|pre| pre.split('.').map(str::to_owned).collect())
                .unwrap_or_default(),
        })
    }

    /// 是否是预发布版本。
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

/// 点分隔、每段非空且只含 `[0-9A-Za-z-]`。
fn valid_identifiers(text: &str) -> bool {
    text.split('.').all(|part| {
        !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if self.is_prerelease() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

impl Ord for Version {
    /// SemVer 优先级：先比主 / 次 / 修订号；相同时正式版高于预发布；预发布逐段比较，
    /// 纯数字段按数值且低于含字母的段，其余按 ASCII，前面都相同时段数多的更高。
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_prerelease(&self.pre, &other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_prerelease(left: &[String], right: &[String]) -> Ordering {
    for (a, b) in left.iter().zip(right) {
        let order = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

/// GitHub 上的一个正式版。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    /// 归一后的版本号（如 `2.1.0`），「忽略本次更新」记住的就是它。
    pub version: String,
    /// GitHub 上的 tag 名（如 `v2.1`）。
    pub tag: String,
    /// 整理成纯文本的更新说明（可能为空）。
    pub notes: String,
    /// 发布页地址（只会是本项目的 Releases 页面）。
    pub url: String,
    /// 发布日期 `YYYY-MM-DD`（UTC）。
    pub published_on: Option<String>,
}

/// 与当前版本比较的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateStatus {
    /// 当前就是最新版（或比 GitHub 上的还新，例如自己构建的预发布）。
    UpToDate,
    /// 有新版本。
    Available,
    /// 有新版本，但用户选了「忽略本次更新」：自动检查不再弹窗，手动检查仍然展示。
    Ignored,
}

/// 一次检查的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    /// 当前运行的版本。
    pub current_version: String,
    /// 比较结果。
    pub status: UpdateStatus,
    /// GitHub 上的最新正式版。
    pub release: ReleaseInfo,
}

/// 检查失败的类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateErrorKind {
    /// 网络失败、超时或 GitHub 返回了其他错误状态。
    Network,
    /// GitHub 限制了访问频率（HTTP 403 / 429）。
    RateLimited,
    /// 仓库还没有正式版（HTTP 404）。
    NoRelease,
    /// 返回内容无法识别。
    Invalid,
}

/// 检查更新失败，`message` 可以直接展示给用户。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateError {
    /// 失败类别。
    pub kind: UpdateErrorKind,
    /// 中文说明。
    pub message: String,
}

impl UpdateError {
    fn new(kind: UpdateErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn from_http(failure: HttpFailure) -> Self {
        match failure {
            HttpFailure::Status(403 | 429) => Self::new(
                UpdateErrorKind::RateLimited,
                "GitHub 暂时限制了访问频率，请过一会儿再检查更新",
            ),
            HttpFailure::Status(404) => {
                Self::new(UpdateErrorKind::NoRelease, "GitHub 上还没有正式发布的版本")
            }
            HttpFailure::Status(status) => Self::new(
                UpdateErrorKind::Network,
                format!("检查更新失败：GitHub 返回 HTTP {status}，请稍后再试"),
            ),
            other => Self::new(
                UpdateErrorKind::Network,
                format!("检查更新失败：{}，请检查网络后重试", other.describe()),
            ),
        }
    }

    fn invalid(detail: impl fmt::Display) -> Self {
        Self::new(
            UpdateErrorKind::Invalid,
            format!("GitHub 返回的版本信息无法识别：{detail}"),
        )
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UpdateError {}

/// 这次检查是谁发起的，以及设置里与更新有关的两项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRequest {
    /// 用户在设置里点了「立即检查」（不受自动检查开关影响）。
    pub manual: bool,
    /// 设置里的「自动检查更新」。
    pub auto_check: bool,
    /// 设置里记住的已忽略版本（归一后的版本号，空表示没有）。
    pub ignored_version: String,
}

/// 从 GitHub 读取最新正式版并与当前版本比较。
pub struct UpdateChecker {
    http: OnlineHttp,
    current: Version,
}

impl UpdateChecker {
    /// `current_version` 通常是 `app_info().version`；万一无法解析就按 0.0.0 处理（任何正式版都算新版本）。
    pub fn new(transport: Arc<dyn HttpTransport>, current_version: &str) -> Self {
        Self {
            http: OnlineHttp::new(transport, UPDATE_TIMEOUT),
            current: Version::parse(current_version).unwrap_or_default(),
        }
    }

    /// 读取最新正式版并比较。不重试：自动检查失败下次启动再查，手动检查由用户自己再点。
    pub async fn check(&self, ignored_version: &str) -> Result<UpdateCheck, UpdateError> {
        let headers = vec![
            (
                "Accept".to_owned(),
                "application/vnd.github+json".to_owned(),
            ),
            ("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned()),
            // GitHub 拒绝没有 User-Agent 的请求。
            (
                "User-Agent".to_owned(),
                format!("LunaticPrincess/{}", self.current),
            ),
        ];
        let text = self
            .http
            .get_text(LATEST_RELEASE_API, headers, &CancelToken::new())
            .await
            .map_err(UpdateError::from_http)?;
        let (latest, release) = parse_release(&text)?;
        Ok(UpdateCheck {
            current_version: self.current.to_string(),
            status: classify(&self.current, &latest, ignored_version),
            release,
        })
    }

    /// 按请求检查并写运行日志。自动检查且开关关闭时不联网，返回 `Ok(None)`。
    ///
    /// 自动检查失败只记一条普通日志（离线时每次启动都失败，不该显示成警告）；手动检查失败记警告。
    pub async fn run(
        &self,
        request: &UpdateRequest,
        logs: &LogHub,
    ) -> Result<Option<UpdateCheck>, UpdateError> {
        if !request.manual && !request.auto_check {
            return Ok(None);
        }
        match self.check(&request.ignored_version).await {
            Ok(check) => {
                let release = &check.release;
                match check.status {
                    UpdateStatus::Available => logs.info(format!(
                        "发现新版本 {}（当前 {}），可以到 GitHub 发布页下载",
                        release.version, check.current_version
                    )),
                    UpdateStatus::Ignored => logs.info(format!(
                        "GitHub 上的最新版本 {} 已被忽略，有更新的版本时再提醒",
                        release.version
                    )),
                    UpdateStatus::UpToDate => {
                        logs.info(format!("已是最新版本（{}）", check.current_version))
                    }
                }
                Ok(Some(check))
            }
            Err(error) if request.manual => {
                logs.warning(&error.message);
                Err(error)
            }
            Err(error) => {
                logs.info(format!("自动检查更新没有完成：{}", error.message));
                Err(error)
            }
        }
    }
}

/// 比较当前版本与最新正式版；`ignored_version` 只在它正好等于最新版时生效，出了更新的版本照常提醒。
pub fn classify(current: &Version, latest: &Version, ignored_version: &str) -> UpdateStatus {
    if latest <= current {
        UpdateStatus::UpToDate
    } else if Version::parse(ignored_version).as_ref() == Some(latest) {
        UpdateStatus::Ignored
    } else {
        UpdateStatus::Available
    }
}

/// `releases/latest` 返回的字段（只取用得到的）。
#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// 解析 `releases/latest` 的响应。
pub fn parse_release(json: &str) -> Result<(Version, ReleaseInfo), UpdateError> {
    let release: GithubRelease = serde_json::from_str(json).map_err(UpdateError::invalid)?;
    if release.draft || release.prerelease {
        return Err(UpdateError::new(
            UpdateErrorKind::NoRelease,
            "GitHub 上还没有正式发布的版本",
        ));
    }
    let tag = release.tag_name.trim().to_owned();
    let version = Version::parse(&tag)
        .ok_or_else(|| UpdateError::invalid(format!("tag「{tag}」不是版本号")))?;
    let url = release
        .html_url
        .filter(|url| is_release_page(url))
        .unwrap_or_else(|| format!("{RELEASES_PAGE}/latest"));
    let info = ReleaseInfo {
        version: version.to_string(),
        tag,
        notes: release_notes_text(release.body.as_deref().unwrap_or_default()),
        url,
        published_on: release.published_at.as_deref().and_then(date_part),
    };
    Ok((version, info))
}

/// 把 Release 正文整理成弹窗里显示的纯文本：去掉标题的 `#`、自动生成的「Full Changelog」链接与多余空行，
/// 太长时截断。正文来自打 tag 时写的改动列表，本来就是纯文本列表。
pub fn release_notes_text(body: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in body.lines() {
        let line = line.trim_end();
        if line.trim_start().starts_with("**Full Changelog**") {
            continue;
        }
        let line = match line.trim_start_matches('#') {
            heading if heading.len() < line.len() && heading.starts_with(' ') => heading.trim(),
            _ => line,
        };
        if line.is_empty() && lines.last().is_none_or(|last| last.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|last| last.is_empty()) {
        lines.pop();
    }
    let text = lines.join("\n");
    if text.chars().count() <= MAX_NOTES_CHARS {
        return text;
    }
    let mut cut: String = text.chars().take(MAX_NOTES_CHARS).collect();
    cut.push('…');
    cut
}

/// `2026-11-05T08:00:00Z` -> `2026-11-05`。
fn date_part(timestamp: &str) -> Option<String> {
    let date = timestamp.get(..10)?;
    let bytes = date.as_bytes();
    let shaped = bytes.iter().enumerate().all(|(index, byte)| match index {
        4 | 7 => *byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    shaped.then(|| date.to_owned())
}

/// 是否是本项目的发布页（`RELEASES_PAGE` 本身或它下面的路径，只含 URL 路径里常见的安全字符）。
pub fn is_release_page(url: &str) -> bool {
    let Some(rest) = url.strip_prefix(RELEASES_PAGE) else {
        return false;
    };
    rest.is_empty()
        || (rest.starts_with('/')
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-._~%+".contains(&b)))
}

/// 用默认浏览器打开本项目的发布页；其他地址一律拒绝（前端传来的地址不可信）。
pub fn open_release_page(url: &str) -> Result<(), String> {
    if !is_release_page(url) {
        return Err("只能打开本项目在 GitHub 上的发布页".to_owned());
    }
    open_in_browser(url)
}

#[cfg(windows)]
fn open_in_browser(url: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
    let operation = wide("open");
    let file = wide(url);
    // SAFETY: 两个字符串都以 0 结尾并在调用期间存活，其余指针参数按文档传空。
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // 返回值大于 32 表示成功，否则是错误码。
    let code = result as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(format!("无法打开浏览器（错误码 {code}），请手动访问 {url}"))
    }
}

#[cfg(not(windows))]
fn open_in_browser(url: &str) -> Result<(), String> {
    Err(format!("当前平台不能自动打开浏览器，请手动访问 {url}"))
}

#[cfg(test)]
mod tests;
