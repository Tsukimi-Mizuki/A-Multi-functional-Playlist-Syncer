//! 网易云接口错误。对应 C# 的 `NeteaseApiException` 与 Desktop `ErrorFormatter` 里对它的中文转换。

use std::fmt;

/// 错误的来源，用来区分“暂时性网络问题”与“接口明确拒绝”。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeteaseErrorKind {
    /// 请求超时。
    Timeout,
    /// 连接 / TLS / 传输层失败。
    Network,
    /// 服务端返回了非 2xx 的 HTTP 状态码。
    HttpStatus,
    /// HTTP 200，但 JSON 里的业务 `code` 不是 200。
    Api,
    /// 响应不是合法 JSON，或结构不符合预期。
    Protocol,
}

/// 网易云接口调用失败。
///
/// 只有 `code == 301`（且不是 HTTP 状态码）才算“接口明确说未登录”，其余一律不能据此清除登录态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeteaseError {
    message: String,
    code: Option<i32>,
    kind: NeteaseErrorKind,
}

impl NeteaseError {
    /// 通用构造：`code` 为接口业务 code（`kind == Api`）或 HTTP 状态码（`kind == HttpStatus`）。
    pub fn new(message: impl Into<String>, code: Option<i32>, kind: NeteaseErrorKind) -> Self {
        Self {
            message: message.into(),
            code,
            kind,
        }
    }

    /// 接口业务错误，例如 `code=301`。
    pub fn api(message: impl Into<String>, code: i32) -> Self {
        Self::new(message, Some(code), NeteaseErrorKind::Api)
    }

    /// HTTP 层状态码错误（401 / 302 / 502 等）。
    pub fn http_status(message: impl Into<String>, status: u16) -> Self {
        Self::new(
            message,
            Some(i32::from(status)),
            NeteaseErrorKind::HttpStatus,
        )
    }

    /// 响应格式不符合预期。
    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(message, None, NeteaseErrorKind::Protocol)
    }

    /// 英文细节已翻译好的中文描述。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 接口 JSON 里的业务 code，或（[`is_http_status`](Self::is_http_status) 为真时）HTTP 状态码。
    pub fn code(&self) -> Option<i32> {
        self.code
    }

    /// 错误来源。
    pub fn kind(&self) -> NeteaseErrorKind {
        self.kind
    }

    /// `code` 是 HTTP 状态码而不是接口业务 code（例如 401 / 302 / 502）。
    pub fn is_http_status(&self) -> bool {
        self.kind == NeteaseErrorKind::HttpStatus
    }

    /// 接口明确返回“未登录”（业务 `code == 301`）；HTTP 层状态码不算。
    pub fn is_not_logged_in(&self) -> bool {
        !self.is_http_status() && self.code == Some(301)
    }

    /// 超时或网络层失败：多半是暂时的，调用方可以重试。
    pub fn is_transient_network(&self) -> bool {
        matches!(
            self.kind,
            NeteaseErrorKind::Timeout | NeteaseErrorKind::Network
        )
    }

    /// 适合直接展示给用户的中文提示（对应 C# `ErrorFormatter.ToFriendlyMessage`）。
    pub fn user_message(&self) -> String {
        match self.kind {
            NeteaseErrorKind::Timeout => "请求超时，请稍后重试".to_owned(),
            NeteaseErrorKind::Network => "网络连接失败，请检查网络后重试".to_owned(),
            _ => match self.code {
                Some(code) => format!("网易云接口错误（{code}）：{}", self.message),
                None => format!("网易云接口错误：{}", self.message),
            },
        }
    }
}

impl fmt::Display for NeteaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for NeteaseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_business_code_301_means_not_logged_in() {
        assert!(NeteaseError::api("需要登录", 301).is_not_logged_in());
        assert!(!NeteaseError::http_status("HTTP 301", 301).is_not_logged_in());
        for code in [-460, 406, -2, 200] {
            assert!(!NeteaseError::api("x", code).is_not_logged_in());
        }
        for status in [302, 401, 500] {
            let error = NeteaseError::http_status("x", status);
            assert!(error.is_http_status());
            assert!(!error.is_not_logged_in());
        }
    }

    #[test]
    fn user_message_is_chinese_and_distinguishes_transient_errors() {
        let timeout = NeteaseError::new("超时", None, NeteaseErrorKind::Timeout);
        assert_eq!(timeout.user_message(), "请求超时，请稍后重试");
        assert!(timeout.is_transient_network());

        let network = NeteaseError::new("断开", None, NeteaseErrorKind::Network);
        assert_eq!(network.user_message(), "网络连接失败，请检查网络后重试");
        assert!(network.is_transient_network());

        let api = NeteaseError::api("获取当前用户失败：接口返回 code=-460", -460);
        assert!(api.user_message().starts_with("网易云接口错误（-460）"));
        assert!(!api.is_transient_network());

        let protocol = NeteaseError::protocol("不是合法 JSON");
        assert_eq!(protocol.user_message(), "网易云接口错误：不是合法 JSON");
    }
}
