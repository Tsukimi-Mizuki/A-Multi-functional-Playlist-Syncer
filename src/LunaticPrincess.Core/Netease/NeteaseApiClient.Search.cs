using System.Globalization;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Netease;

public sealed partial class NeteaseApiClient
{
    /// <inheritdoc />
    public async Task<IReadOnlyList<NeteaseSong>> SearchSongsAsync(
        string keyword,
        int limit = 20,
        CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(keyword))
        {
            return Array.Empty<NeteaseSong>();
        }

        var safeLimit = Math.Clamp(limit, 1, 50);
        var uri = new Uri(
            MusicBaseUri,
            $"/api/search/get/web?s={Uri.EscapeDataString(keyword.Trim())}&type=1&limit={safeLimit.ToString(CultureInfo.InvariantCulture)}&offset=0");
        using var document = await GetJsonAsync(uri, "搜索网易云歌曲", cancellationToken).ConfigureAwait(false);

        var resultElement = document.RootElement.GetPropertyOrNull("result");
        var songsElement = resultElement?.GetPropertyOrNull("songs");
        if (songsElement is not { ValueKind: System.Text.Json.JsonValueKind.Array })
        {
            return Array.Empty<NeteaseSong>();
        }

        return songsElement.Value
            .EnumerateArray()
            .Select(song => NeteaseMappers.MapSong(song))
            .Where(song => song.Id > 0 && !string.IsNullOrWhiteSpace(song.Name))
            .ToList();
    }
}
