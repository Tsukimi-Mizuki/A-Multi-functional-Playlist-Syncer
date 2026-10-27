using System.Globalization;
using System.Text.Json.Nodes;
using LunaticPrincess.Core.Models;

namespace LunaticPrincess.Core.Netease;

public sealed partial class NeteaseApiClient
{
    /// <inheritdoc />
    public async Task<AlbumInfo?> GetAlbumAsync(long albumId, CancellationToken cancellationToken = default)
    {
        if (albumId <= 0)
        {
            return null;
        }

        // 1) weapi 端点优先（公开 GET /api/album/{id} 对部分专辑会返回 -462）。
        NeteaseApiException? weapiError = null;
        try
        {
            using var document = await PostWeapiAsync(
                    $"/weapi/v1/album/{albumId.ToString(CultureInfo.InvariantCulture)}",
                    new JsonObject(),
                    "获取专辑信息",
                    cancellationToken)
                .ConfigureAwait(false);
            if (document.RootElement.GetPropertyOrNull("album") is { ValueKind: System.Text.Json.JsonValueKind.Object } album)
            {
                return NeteaseMappers.MapAlbum(album);
            }
        }
        catch (NeteaseApiException ex)
        {
            weapiError = ex;
        }

        // 2) 公开 GET /api/album/{id} 兜底（可能返回 -462，作为已知失败容忍）。
        try
        {
            var uri = new Uri(MusicBaseUri, $"/api/album/{albumId.ToString(CultureInfo.InvariantCulture)}");
            using var document = await GetJsonAsync(uri, "获取专辑信息", cancellationToken).ConfigureAwait(false);
            if (document.RootElement.GetPropertyOrNull("album") is { ValueKind: System.Text.Json.JsonValueKind.Object } album)
            {
                return NeteaseMappers.MapAlbum(album);
            }

            return null;
        }
        catch (NeteaseApiException fallbackError)
        {
            throw new NeteaseApiException(
                $"获取专辑 {albumId} 失败：{weapiError?.Message ?? "weapi 不可用"}；备用接口：{fallbackError.Message}",
                weapiError?.Code ?? fallbackError.Code,
                fallbackError);
        }
    }
}
