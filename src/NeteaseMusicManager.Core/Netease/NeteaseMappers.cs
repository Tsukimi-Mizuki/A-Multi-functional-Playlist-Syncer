using System.Globalization;
using System.Text.Json;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Netease;

/// <summary>把网易云原始 JSON 归一化成 Core.Models 中的强类型模型。</summary>
internal static class NeteaseMappers
{
    private const int DefaultDiscNumber = 1;

    /// <summary>
    /// 映射一首歌曲（playlist/detail 的 tracks、song/detail 的 songs 通用）。
    /// <paramref name="privilege"/> 用于补充接口顶层 privileges 数组里的 maxbr/fee（真实接口里 privileges 与 songs 平级）。
    /// </summary>
    public static NeteaseSong MapSong(JsonElement song, JsonElement? privilege = null)
    {
        if (song.ValueKind != JsonValueKind.Object)
        {
            return new NeteaseSong();
        }

        var artists = MapArtists(song.GetPropertyOrNull("ar"))
                      ?? MapArtists(song.GetPropertyOrNull("artists"))
                      ?? Array.Empty<ArtistRef>();

        var albumElement = song.GetPropertyOrNull("al") ?? song.GetPropertyOrNull("album");
        var album = MapAlbumRef(albumElement);

        var publishTime = song.GetInt64("publishTime");
        if (publishTime <= 0 && albumElement is { } albumJson)
        {
            publishTime = albumJson.GetInt64("publishTime");
        }

        var discNumber = song.GetInt32("cd", song.GetInt32("disc", DefaultDiscNumber));
        if (discNumber <= 0)
        {
            discNumber = DefaultDiscNumber;
        }

        // 部分接口把 privilege 内嵌在歌曲对象里，v3/song/detail 与 v6/playlist/detail 则放在顶层 privileges 数组。
        var effectivePrivilege = song.GetPropertyOrNull("privilege") ?? privilege;
        var maxBitrate = 0;
        var privilegeFee = 0;
        if (effectivePrivilege is { ValueKind: JsonValueKind.Object } privilegeJson)
        {
            maxBitrate = privilegeJson.GetInt32("maxbr", privilegeJson.GetInt32("playMaxbr"));
            privilegeFee = privilegeJson.GetInt32("fee");
        }

        return new NeteaseSong
        {
            Id = song.GetInt64("id"),
            Name = song.GetStringOrNull("name") ?? string.Empty,
            Artists = artists,
            Album = album,
            Alias = MapAlias(song.GetPropertyOrNull("alias") ?? song.GetPropertyOrNull("alia")),
            DurationMs = song.GetInt32("dt", song.GetInt32("duration")),
            TrackNumber = song.GetInt32("no", song.GetInt32("trackNo")),
            DiscNumber = discNumber,
            ReleaseDate = FormatReleaseDate(publishTime),
            Fee = song.GetInt32("fee", privilegeFee),
            MaxBitrate = maxBitrate,
        };
    }

    /// <summary>读取接口顶层 privileges 数组，按歌曲 id 建索引（用于补充 maxbr/fee）。</summary>
    public static Dictionary<long, JsonElement> MapPrivileges(JsonElement root)
    {
        var privilegesById = new Dictionary<long, JsonElement>();
        if (root.GetArrayOrNull("privileges") is not { } privileges)
        {
            return privilegesById;
        }

        foreach (var privilege in privileges.EnumerateArray())
        {
            if (privilege.ValueKind != JsonValueKind.Object)
            {
                continue;
            }

            var id = privilege.GetInt64("id");
            if (id > 0)
            {
                privilegesById[id] = privilege;
            }
        }

        return privilegesById;
    }

    /// <summary>映射歌单列表项（/weapi/user/playlist 返回的 playlist 数组元素）。</summary>
    public static PlaylistSummary MapPlaylistSummary(JsonElement playlist)
    {
        if (playlist.ValueKind != JsonValueKind.Object)
        {
            return new PlaylistSummary(0, string.Empty, null, 0);
        }

        var creator = playlist.GetPropertyOrNull("creator");
        return new PlaylistSummary(
            playlist.GetInt64("id"),
            playlist.GetStringOrNull("name") ?? string.Empty,
            playlist.GetStringOrNull("coverImgUrl"),
            playlist.GetInt32("trackCount"),
            creator is { } creatorJson ? creatorJson.GetStringOrNull("nickname") : null,
            playlist.GetStringOrNull("description"));
    }

    /// <summary>映射专辑（/weapi/v1/album/{id} 与 /api/album/{id} 通用）。</summary>
    public static AlbumInfo MapAlbum(JsonElement album)
    {
        var artists = MapArtists(album.GetPropertyOrNull("artists"))
                      ?? MapSingleArtist(album.GetPropertyOrNull("artist"))
                      ?? Array.Empty<ArtistRef>();

        return new AlbumInfo(
            album.GetInt64("id"),
            album.GetStringOrNull("name") ?? string.Empty,
            album.GetStringOrNull("picUrl"),
            album.GetStringOrNull("description"),
            FormatReleaseDate(album.GetInt64("publishTime")),
            artists);
    }

    /// <summary>映射播放地址接口响应（data 数组第一项）。</summary>
    public static SongPlaybackInfo MapPlaybackInfo(JsonElement root, long songId)
    {
        var data = root.GetArrayOrNull("data");
        if (data is null)
        {
            return new SongPlaybackInfo { SongId = songId, ErrorMessage = "接口未返回播放数据（data 缺失）" };
        }

        foreach (var item in data.Value.EnumerateArray())
        {
            return MapPlaybackItem(item, songId);
        }

        return new SongPlaybackInfo { SongId = songId, ErrorMessage = "接口未返回播放数据（data 为空）" };
    }

    /// <summary>映射歌词接口响应；nolyric/uncollected 视为没有歌词（空结果而非失败）。</summary>
    public static RawLyrics MapLyrics(JsonElement root)
    {
        if (root.GetBoolOrNull("nolyric") == true || root.GetBoolOrNull("uncollected") == true)
        {
            return new RawLyrics(null);
        }

        return new RawLyrics(
            ReadLyricSection(root, "lrc"),
            ReadLyricSection(root, "tlyric"),
            ReadLyricSection(root, "romalrc"),
            ReadLyricSection(root, "yrc"));
    }

    /// <summary>把毫秒时间戳格式化为 yyyy-MM-dd；非法值返回 null。</summary>
    internal static string? FormatReleaseDate(long publishTimeMilliseconds)
    {
        if (publishTimeMilliseconds <= 0)
        {
            return null;
        }

        try
        {
            return DateTimeOffset.FromUnixTimeMilliseconds(publishTimeMilliseconds)
                .ToLocalTime()
                .ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);
        }
        catch (ArgumentOutOfRangeException)
        {
            return null;
        }
    }

    private static SongPlaybackInfo MapPlaybackItem(JsonElement item, long songId)
    {
        if (item.ValueKind != JsonValueKind.Object)
        {
            return new SongPlaybackInfo { SongId = songId, ErrorMessage = "接口未返回播放数据（data 项非法）" };
        }

        var url = item.GetStringOrNull("url");
        if (string.IsNullOrWhiteSpace(url))
        {
            url = null;
        }

        var code = item.GetInt32("code", url is null ? 0 : 200);
        var fee = item.GetInt32("fee");
        var hasTrial = false;
        int? trialStart = null;
        int? trialEnd = null;
        if (item.GetPropertyOrNull("freeTrialInfo") is { ValueKind: JsonValueKind.Object } trial)
        {
            hasTrial = true;
            if (trial.GetPropertyOrNull("start") is not null)
            {
                trialStart = trial.GetInt32("start");
            }

            if (trial.GetPropertyOrNull("end") is not null)
            {
                trialEnd = trial.GetInt32("end");
            }
        }

        return new SongPlaybackInfo
        {
            SongId = item.GetInt64("id", songId),
            Url = url,
            Bitrate = item.GetInt32("br"),
            Size = item.GetInt64("size"),
            Format = item.GetStringOrNull("type"),
            Level = item.GetStringOrNull("level"),
            Fee = fee,
            IsTrial = hasTrial,
            TrialStartSeconds = trialStart,
            TrialEndSeconds = trialEnd,
            ExpiresInSeconds = item.GetInt32("expi"),
            ErrorMessage = url is null ? DescribePlaybackFailure(code, fee) : null,
        };
    }

    private static string DescribePlaybackFailure(int code, int fee)
    {
        if (fee == 1 && code == 404)
        {
            return "歌曲需要 VIP 会员或版权受限，没有可用的播放地址（code=404）";
        }

        return code switch
        {
            0 => "接口未返回播放地址",
            404 => "歌曲版权受限或已下架，没有可用的播放地址（code=404）",
            403 => "无权访问该歌曲（code=403）",
            _ => $"获取播放地址失败（code={code}）",
        };
    }

    private static string? ReadLyricSection(JsonElement root, string sectionName)
    {
        var section = root.GetPropertyOrNull(sectionName);
        if (section is not { ValueKind: JsonValueKind.Object } sectionJson)
        {
            return null;
        }

        var lyric = sectionJson.GetStringOrNull("lyric");
        return string.IsNullOrWhiteSpace(lyric) ? null : lyric;
    }

    private static IReadOnlyList<ArtistRef>? MapArtists(JsonElement? artistsElement)
    {
        if (artistsElement is not { ValueKind: JsonValueKind.Array } array)
        {
            return null;
        }

        var artists = new List<ArtistRef>();
        foreach (var item in array.EnumerateArray())
        {
            if (item.ValueKind != JsonValueKind.Object)
            {
                continue;
            }

            var name = item.GetStringOrNull("name");
            if (string.IsNullOrWhiteSpace(name))
            {
                continue;
            }

            artists.Add(new ArtistRef(item.GetInt64("id"), name));
        }

        return artists;
    }

    private static IReadOnlyList<ArtistRef>? MapSingleArtist(JsonElement? artistElement)
    {
        if (artistElement is not { ValueKind: JsonValueKind.Object } artist)
        {
            return null;
        }

        var name = artist.GetStringOrNull("name");
        if (string.IsNullOrWhiteSpace(name))
        {
            return null;
        }

        return new[] { new ArtistRef(artist.GetInt64("id"), name) };
    }

    private static AlbumRef MapAlbumRef(JsonElement? albumElement)
    {
        if (albumElement is not { ValueKind: JsonValueKind.Object } album)
        {
            return new AlbumRef(0, string.Empty, null);
        }

        return new AlbumRef(
            album.GetInt64("id"),
            album.GetStringOrNull("name") ?? string.Empty,
            album.GetStringOrNull("picUrl"));
    }

    private static string? MapAlias(JsonElement? aliasElement)
    {
        if (aliasElement is not { ValueKind: JsonValueKind.Array } array)
        {
            return null;
        }

        var parts = new List<string>();
        foreach (var item in array.EnumerateArray())
        {
            if (item.ValueKind != JsonValueKind.String)
            {
                continue;
            }

            var text = item.GetString();
            if (!string.IsNullOrWhiteSpace(text))
            {
                parts.Add(text);
            }
        }

        return parts.Count == 0 ? null : string.Join("/", parts);
    }
}
