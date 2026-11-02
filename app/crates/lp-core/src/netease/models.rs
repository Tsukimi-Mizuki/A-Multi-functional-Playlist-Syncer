//! 登录相关的领域模型。对应 C# `Core/Models/NeteaseModels.cs` 中的 `NeteaseUser`、`QrLogin*`。

use std::time::Duration;

use crate::session::NeteaseSession;

/// 当前登录账号的资料。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeteaseUser {
    /// 用户 ID。
    pub user_id: i64,
    /// 昵称。
    pub nickname: String,
    /// 头像地址。
    pub avatar_url: Option<String>,
    /// 会员类型，0 为非会员。
    pub vip_type: i32,
}

/// 刚申请到的登录二维码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrLoginStart {
    /// 轮询用的 unikey。
    pub key: String,
    /// 二维码里编码的内容（`https://music.163.com/login?codekey=…`）。
    pub qr_content: String,
    /// 二维码有效期（约 180 秒）。
    pub lifetime: Duration,
}

/// 一次扫码状态轮询的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrLoginStatus {
    /// 801：等待扫码。
    Waiting,
    /// 802：已扫码，等待手机端确认。
    Scanned,
    /// 803：已确认登录。`session` 一定带有可用 Cookie，但账号资料可能没取到（`user_id == 0`）。
    Confirmed {
        /// 登录态。
        session: NeteaseSession,
        /// 面向用户的说明。
        message: String,
    },
    /// 800：二维码已过期。
    Expired,
    /// 其他 code 或 803 却没有 Cookie。
    Failed {
        /// 面向用户的说明。
        message: String,
    },
}

impl QrLoginStatus {
    /// 面向用户的中文说明。
    pub fn message(&self) -> &str {
        match self {
            Self::Waiting => "等待扫码",
            Self::Scanned => "已扫码，请在手机端确认登录",
            Self::Confirmed { message, .. } | Self::Failed { message } => message,
            Self::Expired => "二维码已过期，请重新获取",
        }
    }

    /// 是否是终态（不需要继续轮询）。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Confirmed { .. } | Self::Expired | Self::Failed { .. }
        )
    }
}
