using System.Globalization;
using System.Text.Json.Nodes;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Netease;

public sealed partial class NeteaseApiClient
{
    /// <inheritdoc />
    public async Task<IReadOnlyList<NeteaseSong>> GetSongDetailsAsync(
        IReadOnlyCollection<long> songIds,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(songIds);

        var uniqueIds = songIds.Where(id => id > 0).Distinct().ToArray();
        if (uniqueIds.Length == 0)
        {
            return Array.Empty<NeteaseSong>();
        }

        var songsById = new Dictionary<long, NeteaseSong>(uniqueIds.Length);
        foreach (var batch in uniqueIds.Chunk(SongDetailBatchSize))
        {
            var songs = await FetchSongDetailBatchAsync(batch, cancellationToken).ConfigureAwait(false);
            foreach (var song in songs)
            {
                songsById[song.Id] = song;
            }
        }

        // 按调用方传入顺序返回（接口返回顺序不可依赖；找不到的 id 跳过）。
        var result = new List<NeteaseSong>(songIds.Count);
        foreach (var id in songIds)
        {
            if (id > 0 && songsById.TryGetValue(id, out var song))
            {
                result.Add(song);
            }
        }

        return result;
    }

    /// <inheritdoc />
    public async Task<SongPlaybackInfo> GetSongPlaybackInfoAsync(
        long songId,
        AudioQuality quality,
        CancellationToken cancellationToken = default)
    {
        if (songId <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(songId), "歌曲 ID 必须大于 0。");
        }

        var attempts = GetQualityFallbackChain(quality);
        var v1Available = true;
        NeteaseApiException? lastError = null;
        string? lastMessage = null;

        foreach (var attempt in attempts)
        {
            if (v1Available)
            {
                try
                {
                    var v1Info = await RequestPlaybackUrlV1Async(songId, attempt, cancellationToken).ConfigureAwait(false);
                    if (v1Info.IsSuccess)
                    {
                        return v1Info;
                    }

                    lastMessage = v1Info.ErrorMessage ?? lastMessage;
                }
                catch (NeteaseApiException ex)
                {
                    // v1 端点在某些网络/匿名场景不可用：本首歌曲剩余音质不再尝试 v1，直接走经典接口。
                    v1Available = false;
                    lastError = ex;
                }
            }

            try
            {
                var legacyInfo = await RequestPlaybackUrlLegacyAsync(songId, attempt, cancellationToken).ConfigureAwait(false);
                if (legacyInfo.IsSuccess)
                {
                    return legacyInfo;
                }

                lastMessage = legacyInfo.ErrorMessage ?? lastMessage;
            }
            catch (NeteaseApiException ex)
            {
                lastError = ex;
            }
        }

        throw new NeteaseApiException(
            lastMessage ?? lastError?.Message ?? $"歌曲 {songId} 没有可用的播放地址",
            lastError?.Code,
            lastError);
    }

    /// <summary>请求新接口 /weapi/song/enhance/player/url/v1。</summary>
    private async Task<SongPlaybackInfo> RequestPlaybackUrlV1Async(
        long songId,
        AudioQuality quality,
        CancellationToken cancellationToken)
    {
        var payload = new JsonObject
        {
            ["ids"] = "[" + songId.ToString(CultureInfo.InvariantCulture) + "]",
            ["level"] = quality.ToApiLevel(),
            ["encodeType"] = "flac",
        };
        using var document = await PostWeapiAsync(
                "/weapi/song/enhance/player/url/v1",
                payload,
                $"获取播放地址（{quality.ToDisplayName()}）",
                cancellationToken)
            .ConfigureAwait(false);
        return NeteaseMappers.MapPlaybackInfo(document.RootElement, songId);
    }

    /// <summary>请求经典接口 /weapi/song/enhance/player/url。</summary>
    private async Task<SongPlaybackInfo> RequestPlaybackUrlLegacyAsync(
        long songId,
        AudioQuality quality,
        CancellationToken cancellationToken)
    {
        var payload = new JsonObject
        {
            ["ids"] = "[" + songId.ToString(CultureInfo.InvariantCulture) + "]",
            ["br"] = quality.ToBitrate(),
        };
        using var document = await PostWeapiAsync(
                "/weapi/song/enhance/player/url",
                payload,
                $"获取播放地址（{quality.ToDisplayName()}）",
                cancellationToken)
            .ConfigureAwait(false);
        return NeteaseMappers.MapPlaybackInfo(document.RootElement, songId);
    }

    /// <summary>从请求音质开始逐级降档（HiRes → Lossless → ExHigh → Higher → Standard）。</summary>
    internal static AudioQuality[] GetQualityFallbackChain(AudioQuality quality)
    {
        var highest = Math.Clamp((int)quality, (int)AudioQuality.Standard, (int)AudioQuality.HiRes);
        var chain = new AudioQuality[highest + 1];
        for (var index = 0; index <= highest; index++)
        {
            chain[index] = (AudioQuality)(highest - index);
        }

        return chain;
    }
}
