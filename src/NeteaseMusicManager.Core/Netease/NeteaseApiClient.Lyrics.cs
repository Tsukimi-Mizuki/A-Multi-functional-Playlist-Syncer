using System.Globalization;
using System.Text.Json.Nodes;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Netease;

public sealed partial class NeteaseApiClient
{
    /// <inheritdoc />
    public async Task<RawLyrics> GetLyricsAsync(long songId, CancellationToken cancellationToken = default)
    {
        if (songId <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(songId), "歌曲 ID 必须大于 0。");
        }

        // 1) 公开接口：匿名可用，且 yv=1 时能拿到干净的 YRC 逐字歌词（实测）。
        try
        {
            var uri = new Uri(
                MusicBaseUri,
                $"/api/song/lyric?id={songId.ToString(CultureInfo.InvariantCulture)}&lv=-1&kv=-1&tv=-1&rv=-1&yv=1");
            using var document = await GetJsonAsync(uri, "获取歌词", cancellationToken).ConfigureAwait(false);
            return NeteaseMappers.MapLyrics(document.RootElement);
        }
        catch (NeteaseApiException)
        {
            // 继续尝试 weapi 路径。
        }

        // 2) weapi /weapi/song/lyric（v0，YRC 为纯文本行）。
        try
        {
            var payload = new JsonObject
            {
                ["id"] = songId,
                ["lv"] = -1,
                ["kv"] = 0,
                ["tv"] = -1,
                ["rv"] = -1,
                ["yv"] = 1,
            };
            using var document = await PostWeapiAsync(
                    "/weapi/song/lyric",
                    payload,
                    "获取歌词",
                    cancellationToken)
                .ConfigureAwait(false);
            return NeteaseMappers.MapLyrics(document.RootElement);
        }
        catch (NeteaseApiException)
        {
            // 继续尝试 weapi v1。
        }

        // 3) weapi /weapi/song/lyric/v1（新版 JSON-flavor，仅兜底；LRC/YRC 可能为 JSON 行）。
        try
        {
            var payload = new JsonObject
            {
                ["id"] = songId,
                ["cp"] = false,
                ["tv"] = 0,
                ["lv"] = 0,
                ["rv"] = 0,
                ["kv"] = 0,
                ["yv"] = 0,
                ["ytv"] = 0,
                ["yrv"] = 0,
            };
            using var document = await PostWeapiAsync(
                    "/weapi/song/lyric/v1",
                    payload,
                    "获取歌词",
                    cancellationToken)
                .ConfigureAwait(false);
            return NeteaseMappers.MapLyrics(document.RootElement);
        }
        catch (NeteaseApiException ex)
        {
            throw new NeteaseApiException($"获取歌曲 {songId} 歌词失败：{ex.Message}", ex.Code, ex);
        }
    }
}
