using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Netease;

public sealed partial class NeteaseApiClient
{
    private const int UserPlaylistPageSize = 1000;
    private const int UserPlaylistMaxPages = 10;
    private const int SongDetailBatchSize = 500;

    /// <inheritdoc />
    public async Task<IReadOnlyList<PlaylistSummary>> GetUserPlaylistsAsync(
        long userId,
        CancellationToken cancellationToken = default)
    {
        if (userId <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(userId), "用户 ID 必须大于 0。");
        }

        var playlists = new List<PlaylistSummary>();
        var seenIds = new HashSet<long>();
        var offset = 0;

        // 单页 1000 已能覆盖绝大多数账号；仍按 more/页长防御性翻页，最多 10 页。
        for (var page = 0; page < UserPlaylistMaxPages; page++)
        {
            var payload = new JsonObject
            {
                ["uid"] = userId,
                ["limit"] = UserPlaylistPageSize,
                ["offset"] = offset,
                ["includeVideo"] = false,
            };
            using var document = await PostWeapiAsync(
                    "/weapi/user/playlist",
                    payload,
                    "获取用户歌单",
                    cancellationToken)
                .ConfigureAwait(false);

            var root = document.RootElement;
            var items = root.GetArrayOrNull("playlist");
            if (items is null)
            {
                break;
            }

            var pageItemCount = 0;
            foreach (var item in items.Value.EnumerateArray())
            {
                pageItemCount++;
                var summary = NeteaseMappers.MapPlaylistSummary(item);
                if (summary.Id > 0 && seenIds.Add(summary.Id))
                {
                    playlists.Add(summary);
                }
            }

            if (pageItemCount < UserPlaylistPageSize)
            {
                break;
            }

            if (root.GetBoolOrNull("more") == false)
            {
                break;
            }

            offset += UserPlaylistPageSize;
        }

        return playlists;
    }

    /// <inheritdoc />
    public async Task<PlaylistDetail> GetPlaylistDetailAsync(
        long playlistId,
        CancellationToken cancellationToken = default)
    {
        if (playlistId <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(playlistId), "歌单 ID 必须大于 0。");
        }

        var payload = new JsonObject
        {
            ["id"] = playlistId,
            ["n"] = 1000,
            ["s"] = 8,
        };
        using var document = await PostWeapiAsync(
                "/weapi/v6/playlist/detail",
                payload,
                "获取歌单详情",
                cancellationToken)
            .ConfigureAwait(false);

        var playlistElement = document.RootElement.GetPropertyOrNull("playlist");
        if (playlistElement is not { ValueKind: JsonValueKind.Object } playlist)
        {
            throw new NeteaseApiException($"歌单 {playlistId} 不存在或无权访问", 404);
        }

        // 1) 先收下内嵌 tracks（v6 详情首次最多返回 n 首）。
        //    注意：maxbr/fee 在响应顶层 privileges 数组里，需要按 id 关联。
        var privilegesById = NeteaseMappers.MapPrivileges(document.RootElement);
        var songsById = new Dictionary<long, NeteaseSong>();
        var embeddedSongs = new List<NeteaseSong>();
        if (playlist.GetArrayOrNull("tracks") is { } tracks)
        {
            foreach (var track in tracks.EnumerateArray())
            {
                JsonElement? privilege = privilegesById.TryGetValue(track.GetInt64("id"), out var privilegeElement)
                    ? privilegeElement
                    : null;
                var song = NeteaseMappers.MapSong(track, privilege);
                if (song.Id <= 0 || !songsById.TryAdd(song.Id, song))
                {
                    continue;
                }

                embeddedSongs.Add(song);
            }
        }

        // 2) trackIds 是全量顺序来源。
        var trackIds = new List<long>();
        var trackIdSet = new HashSet<long>();
        if (playlist.GetArrayOrNull("trackIds") is { } trackIdElements)
        {
            foreach (var element in trackIdElements.EnumerateArray())
            {
                var id = element.ValueKind == JsonValueKind.Object ? element.GetInt64("id") : element.AsInt64();
                if (id > 0 && trackIdSet.Add(id))
                {
                    trackIds.Add(id);
                }
            }
        }

        if (trackIds.Count == 0)
        {
            trackIds.AddRange(embeddedSongs.Select(song => song.Id));
        }

        // 3) 缺失曲目按 trackIds 顺序批量补齐（每批 <= 500）。
        var missingIds = trackIds.Where(id => !songsById.ContainsKey(id)).ToArray();
        foreach (var batch in missingIds.Chunk(SongDetailBatchSize))
        {
            try
            {
                var fetchedSongs = await FetchSongDetailBatchAsync(batch, cancellationToken).ConfigureAwait(false);
                foreach (var song in fetchedSongs)
                {
                    songsById[song.Id] = song;
                }
            }
            catch (NeteaseApiException)
            {
                // 该批次的歌曲信息无法恢复：交给 IsPartial 标记，继续补齐其他批次。
            }
        }

        var orderedSongs = new List<NeteaseSong>(trackIds.Count);
        foreach (var id in trackIds)
        {
            if (songsById.TryGetValue(id, out var song))
            {
                orderedSongs.Add(song);
            }
        }

        var trackCount = playlist.GetInt32("trackCount", trackIds.Count);
        var creator = playlist.GetPropertyOrNull("creator");
        return new PlaylistDetail
        {
            Id = playlist.GetInt64("id", playlistId),
            Name = playlist.GetStringOrNull("name") ?? string.Empty,
            CoverUrl = playlist.GetStringOrNull("coverImgUrl"),
            Description = playlist.GetStringOrNull("description"),
            CreatorNickname = creator is { } creatorJson ? creatorJson.GetStringOrNull("nickname") : null,
            TrackCount = trackCount,
            Songs = orderedSongs,
            TrackIds = trackIds,
            IsPartial = orderedSongs.Count < trackIds.Count,
        };
    }

    /// <summary>调用 /weapi/v3/song/detail 获取一批歌曲（最多 500 个 id）。</summary>
    private async Task<IReadOnlyList<NeteaseSong>> FetchSongDetailBatchAsync(
        IReadOnlyList<long> songIds,
        CancellationToken cancellationToken)
    {
        var idTexts = songIds.Select(id => id.ToString(CultureInfo.InvariantCulture)).ToArray();
        var idsJson = "[" + string.Join(",", idTexts) + "]";
        var cJson = "[" + string.Join(",", idTexts.Select(id => "{\"id\":" + id + "}")) + "]";
        var payload = new JsonObject
        {
            ["c"] = cJson,
            ["ids"] = idsJson,
        };
        using var document = await PostWeapiAsync(
                "/weapi/v3/song/detail",
                payload,
                "获取歌曲详情",
                cancellationToken)
            .ConfigureAwait(false);

        var songs = new List<NeteaseSong>(songIds.Count);
        if (document.RootElement.GetArrayOrNull("songs") is { } songElements)
        {
            var privilegesById = NeteaseMappers.MapPrivileges(document.RootElement);
            foreach (var element in songElements.EnumerateArray())
            {
                JsonElement? privilege = privilegesById.TryGetValue(element.GetInt64("id"), out var privilegeElement)
                    ? privilegeElement
                    : null;
                var song = NeteaseMappers.MapSong(element, privilege);
                if (song.Id > 0)
                {
                    songs.Add(song);
                }
            }
        }

        return songs;
    }
}
