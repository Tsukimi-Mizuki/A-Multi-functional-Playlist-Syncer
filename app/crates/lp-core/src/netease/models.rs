//! 网易云领域模型。对应 C# `Core/Models/NeteaseModels.cs`：登录相关（`NeteaseUser`、`QrLogin*`）
//! 与歌单 / 歌曲相关（`PlaylistSummary`、`PlaylistDetail`、`NeteaseSong`、`ArtistRef`、`AlbumRef`）。
//!
//! 歌单 / 歌曲模型按 camelCase 序列化，Tauri 命令可以直接交给前端。

use std::time::Duration;

use serde::Serialize;

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

/// 歌手引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRef {
    /// 歌手 ID，未知为 0。
    pub id: i64,
    /// 歌手名。
    pub name: String,
}

/// 专辑引用。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRef {
    /// 专辑 ID，未知为 0。
    pub id: i64,
    /// 专辑名，未知为空串。
    pub name: String,
    /// 封面地址。
    pub cover_url: Option<String>,
}

/// 歌单中的一首歌。字段来自网易云 `song/detail` 与 `playlist/detail` 的归一化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeteaseSong {
    /// 歌曲 ID。
    pub id: i64,
    /// 歌名。
    pub name: String,
    /// 歌手。
    pub artists: Vec<ArtistRef>,
    /// 专辑。
    pub album: AlbumRef,
    /// 别名 / 副标题（多个以 `/` 连接）。
    pub alias: Option<String>,
    /// 时长（毫秒）。
    pub duration_ms: i32,
    /// 曲目序号。
    pub track_number: i32,
    /// 碟号，默认 1。
    pub disc_number: i32,
    /// 发行日期 `yyyy-MM-dd`。
    pub release_date: Option<String>,
    /// 付费类型：0 免费、1 VIP、4 付费专辑、8 低音质免费。
    pub fee: i32,
    /// 最高码率（bps），未知为 0。
    pub max_bitrate: i32,
}

impl Default for NeteaseSong {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            artists: Vec::new(),
            album: AlbumRef::default(),
            alias: None,
            duration_ms: 0,
            track_number: 0,
            disc_number: 1,
            release_date: None,
            fee: 0,
            max_bitrate: 0,
        }
    }
}

impl NeteaseSong {
    /// 封面地址（取自专辑）。
    pub fn cover_url(&self) -> Option<&str> {
        self.album.cover_url.as_deref()
    }

    /// 歌手名以 `/` 连接。
    pub fn artist_text(&self) -> String {
        self.artists
            .iter()
            .map(|artist| artist.name.as_str())
            .filter(|name| !name.trim().is_empty())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// 带别名的显示名，如 `歌名 (别名)`。
    pub fn display_name(&self) -> String {
        match self
            .alias
            .as_deref()
            .filter(|alias| !alias.trim().is_empty())
        {
            Some(alias) => format!("{} ({alias})", self.name),
            None => self.name.clone(),
        }
    }
}

/// 歌单列表项（`/weapi/user/playlist` 返回的一项）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSummary {
    /// 歌单 ID。
    pub id: i64,
    /// 歌单名。
    pub name: String,
    /// 封面地址。
    pub cover_url: Option<String>,
    /// 曲目数。
    pub track_count: i32,
    /// 创建者昵称。
    pub creator_nickname: Option<String>,
    /// 简介。
    pub description: Option<String>,
}

/// 歌单详情，含完整曲目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetail {
    /// 歌单 ID。
    pub id: i64,
    /// 歌单名。
    pub name: String,
    /// 封面地址。
    pub cover_url: Option<String>,
    /// 简介。
    pub description: Option<String>,
    /// 创建者昵称。
    pub creator_nickname: Option<String>,
    /// 接口声明的曲目数（可能大于 `songs.len()`，例如有歌曲下架）。
    pub track_count: i32,
    /// 已取得详情的歌曲，顺序与歌单一致。
    pub songs: Vec<NeteaseSong>,
    /// 歌单里全部歌曲 ID（按歌单顺序）。前端不需要，不序列化。
    #[serde(skip)]
    pub track_ids: Vec<i64>,
    /// 有歌曲详情没能取回（某批请求失败）。
    pub is_partial: bool,
}
