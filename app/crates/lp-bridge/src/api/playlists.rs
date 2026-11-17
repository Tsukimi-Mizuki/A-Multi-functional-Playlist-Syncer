//! 歌单：当前账号的歌单列表与一个歌单的完整曲目。

use std::sync::Arc;

use lp_core::netease::{NeteaseSong, PlaylistSummary};

use crate::state::{bridge, run};

/// 歌单列表里的一项。
pub struct PlaylistDto {
    pub id: i64,
    pub name: String,
    /// 封面地址。
    pub cover_url: Option<String>,
    pub track_count: i32,
    /// 创建者昵称。
    pub creator_nickname: Option<String>,
}

impl From<PlaylistSummary> for PlaylistDto {
    fn from(summary: PlaylistSummary) -> Self {
        Self {
            id: summary.id,
            name: summary.name,
            cover_url: summary.cover_url,
            track_count: summary.track_count,
            creator_nickname: summary.creator_nickname,
        }
    }
}

/// 一首歌（表格里的一行）。
pub struct SongDto {
    pub id: i64,
    pub title: String,
    /// 别名 / 译名（显示在标题后面）。
    pub alias: Option<String>,
    /// 歌手，用「 / 」连接。
    pub artists: String,
    pub album: String,
    pub duration_ms: i32,
}

impl From<NeteaseSong> for SongDto {
    fn from(song: NeteaseSong) -> Self {
        Self {
            id: song.id,
            title: song.name,
            alias: song.alias,
            artists: song
                .artists
                .iter()
                .map(|artist| artist.name.as_str())
                .collect::<Vec<_>>()
                .join(" / "),
            album: song.album.name,
            duration_ms: song.duration_ms,
        }
    }
}

/// 一个歌单的完整曲目。
pub struct PlaylistDetailDto {
    pub id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub description: Option<String>,
    pub creator_nickname: Option<String>,
    pub track_count: i32,
    pub songs: Vec<SongDto>,
    /// 有一批曲目详情没有取回（界面给提示与「重新加载」）。
    pub is_partial: bool,
}

/// 当前账号的全部歌单。失败时是中文说明。
pub async fn list_playlists() -> Result<Vec<PlaylistDto>, String> {
    let playlists = Arc::clone(&bridge()?.playlists);
    let list = run(async move { playlists.list().await })
        .await?
        .map_err(|error| error.to_string())?;
    Ok(list.into_iter().map(PlaylistDto::from).collect())
}

/// 一个歌单的完整曲目（超过 500 首时分批补齐；原型阶段不推送进度）。
pub async fn playlist_tracks(id: i64) -> Result<PlaylistDetailDto, String> {
    let playlists = Arc::clone(&bridge()?.playlists);
    let detail = run(async move { playlists.tracks(id, &|_loaded, _total| {}).await })
        .await?
        .map_err(|error| error.to_string())?;
    Ok(PlaylistDetailDto {
        id: detail.id,
        name: detail.name,
        cover_url: detail.cover_url,
        description: detail.description,
        creator_nickname: detail.creator_nickname,
        track_count: detail.track_count,
        songs: detail.songs.into_iter().map(SongDto::from).collect(),
        is_partial: detail.is_partial,
    })
}
