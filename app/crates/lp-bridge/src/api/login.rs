//! 扫码登录：启动校验、出码、轮询、退出。登录态（Cookie）只留在 Rust 这边，Dart 永远拿不到。

use std::sync::Arc;

use lp_core::auth::{QrPollOutcome, ValidationOutcome};

use super::app::AccountDto;
use crate::state::{bridge, run};

/// 启动后联网校验的结果。
pub struct ValidationDto {
    /// `notLoggedIn` / `valid` / `unverified`（网络等原因暂时无法验证，登录保留）/ `loggedOut`（确认失效，已清除）。
    pub status: String,
    /// `unverified` 时的中文说明。
    pub message: Option<String>,
    pub account: AccountDto,
}

/// 登录二维码。
pub struct QrStartDto {
    /// 轮询用的 key。
    pub key: String,
    /// 白底黑码的 SVG 文本。
    pub svg: String,
    /// 有效期（秒）。
    pub lifetime_secs: u64,
}

/// 轮询一次的结果。
pub struct QrPollDto {
    /// `waiting` / `scanned`（已扫码，等手机确认）/ `expired` / `failed` / `success` / `error`。
    pub state: String,
    /// `failed` / `error` 时的中文说明。
    pub message: Option<String>,
    /// `error` 时是否是暂时性的网络问题（界面可以继续轮询几次）。
    pub transient: bool,
    /// `success` 时的账号。
    pub account: Option<AccountDto>,
}

/// 启动后的联网校验：读取账号并续期；网络类错误保留登录，明确 301 才清除。
pub async fn validate_login() -> Result<ValidationDto, String> {
    let login = Arc::clone(&bridge()?.login);
    let (outcome, account) = run(async move {
        let outcome = login.validate().await;
        (outcome, login.account())
    })
    .await?;
    let (status, message) = match outcome {
        ValidationOutcome::NotLoggedIn => ("notLoggedIn", None),
        ValidationOutcome::Valid => ("valid", None),
        ValidationOutcome::Unverified { message } => ("unverified", Some(message)),
        ValidationOutcome::LoggedOut => ("loggedOut", None),
    };
    Ok(ValidationDto {
        status: status.to_owned(),
        message,
        account: account.into(),
    })
}

/// 申请登录二维码。
pub async fn qr_start() -> Result<QrStartDto, String> {
    let bridge = bridge()?;
    let login = Arc::clone(&bridge.login);
    match run(async move { login.qr_start().await }).await? {
        Ok(start) => Ok(QrStartDto {
            key: start.key,
            svg: start.svg,
            lifetime_secs: start.lifetime_secs,
        }),
        Err(error) => {
            let message = error.user_message();
            bridge
                .logs
                .warning(format!("获取登录二维码失败：{message}"));
            Err(message)
        }
    }
}

/// 轮询一次扫码状态（界面约每 2 秒调一次）；错误折叠进返回值。
pub async fn qr_poll(key: String) -> Result<QrPollDto, String> {
    let login = Arc::clone(&bridge()?.login);
    let result = run(async move { login.qr_poll(&key).await }).await?;
    let plain = |state: &str| QrPollDto {
        state: state.to_owned(),
        message: None,
        transient: false,
        account: None,
    };
    Ok(match result {
        Ok(QrPollOutcome::Waiting) => plain("waiting"),
        Ok(QrPollOutcome::Scanned) => plain("scanned"),
        Ok(QrPollOutcome::Expired) => plain("expired"),
        Ok(QrPollOutcome::Failed { message }) => QrPollDto {
            message: Some(message),
            ..plain("failed")
        },
        Ok(QrPollOutcome::Success { account }) => QrPollDto {
            account: Some(account.into()),
            ..plain("success")
        },
        Err(error) => QrPollDto {
            message: Some(error.user_message()),
            transient: error.is_transient_network(),
            ..plain("error")
        },
    })
}

/// 退出登录：清内存会话 → 排空自动落盘 → 删除 `session.dat`。
pub async fn logout() -> Result<(), String> {
    let login = Arc::clone(&bridge()?.login);
    run(async move { login.logout().await }).await
}
