//! 网易云音乐非公开接口客户端：weapi 加密、扫码登录、Cookie 合并与续期、歌单、歌曲详情、歌词与播放地址（取链），后续模块会补专辑/搜索。
//!
//! 对应 C# 的 `Core/Netease/`（`NeteaseApiClient` 及其分文件、`WeapiCrypto`、`CookieMerger`）。
//! HTTP 经 [`http::HttpTransport`] 抽象，单测用假传输，不联网。

pub mod client;
pub mod cookie_merger;
pub mod crypto;
pub mod error;
pub mod http;
pub mod json;
pub mod login;
pub mod lyrics;
pub mod mappers;
pub mod models;
pub mod player;
pub mod playlists;
pub mod songs;

pub use client::{NeteaseClient, SubscriptionId};
pub use error::{NeteaseError, NeteaseErrorKind};
pub use http::{HttpTransport, ReqwestTransport};
pub use models::{
    AlbumRef, ArtistRef, NeteaseSong, NeteaseUser, PlaylistDetail, PlaylistSummary, QrLoginStart,
    QrLoginStatus,
};
pub use player::SongPlaybackInfo;
pub use playlists::ProgressFn;
