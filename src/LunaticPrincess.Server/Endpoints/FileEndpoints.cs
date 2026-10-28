using LunaticPrincess.Core.Settings;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Infrastructure;

namespace LunaticPrincess.Server.Endpoints;

public static class FileEndpoints
{
    private const long MaxCoverBytes = 20L * 1024 * 1024;
    private static readonly TimeSpan CoverTimeout = TimeSpan.FromSeconds(15);
    private static readonly HashSet<string> AudioExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aac", ".wma", ".ape", ".wv", ".dsf",
    };

    public static void Map(IEndpointRouteBuilder app, ServerServices services)
    {
        app.MapGet("/api/fs/browse", async (string? path, CancellationToken cancellationToken) =>
        {
            var target = path;
            if (string.IsNullOrWhiteSpace(target))
            {
                var settings = await services.SettingsStore.LoadAsync(cancellationToken).ConfigureAwait(false);
                target = FirstExistingDirectory(settings) ?? DefaultRootDirectory();
            }

            target = Path.GetFullPath(target!.Trim());
            if (!Directory.Exists(target))
            {
                throw new ApiException(StatusCodes.Status404NotFound, "目录不存在：" + target, "not_found");
            }

            var directories = new List<FsEntryDto>();
            try
            {
                foreach (var directory in Directory.GetDirectories(target))
                {
                    directories.Add(new FsEntryDto(Path.GetFileName(directory), directory));
                }
            }
            catch (Exception ex) when (ex is UnauthorizedAccessException or IOException)
            {
                services.Log.Warn("读取目录失败：" + target + "：" + ex.Message);
            }

            var audioFiles = new List<FsFileDto>();
            try
            {
                foreach (var file in Directory.GetFiles(target))
                {
                    if (!AudioExtensions.Contains(Path.GetExtension(file)))
                    {
                        continue;
                    }

                    long size;
                    try
                    {
                        size = new FileInfo(file).Length;
                    }
                    catch (Exception ex) when (ex is UnauthorizedAccessException or IOException)
                    {
                        size = 0;
                    }

                    audioFiles.Add(new FsFileDto(Path.GetFileName(file), file, size));
                }
            }
            catch (Exception ex) when (ex is UnauthorizedAccessException or IOException)
            {
                services.Log.Warn("读取目录失败：" + target + "：" + ex.Message);
            }

            var dto = new FsBrowseDto(
                target,
                Directory.GetParent(target)?.FullName,
                directories.OrderBy(d => d.Name, StringComparer.OrdinalIgnoreCase).ToArray(),
                audioFiles.OrderBy(f => f.Name, StringComparer.OrdinalIgnoreCase).ToArray());
            return Results.Json(dto, JsonDefaults.Options);
        });

        app.MapGet("/api/cover", async (string? url, CancellationToken cancellationToken) =>
        {
            var raw = EndpointHelpers.RequirePath(url);
            if (!Uri.TryCreate(raw, UriKind.Absolute, out var uri)
                || (uri.Scheme != Uri.UriSchemeHttp && uri.Scheme != Uri.UriSchemeHttps))
            {
                throw new ApiException(StatusCodes.Status400BadRequest, "url 必须是 http(s) 地址", "invalid_request");
            }

            using var request = new HttpRequestMessage(HttpMethod.Get, uri);
            request.Headers.TryAddWithoutValidation("User-Agent", EndpointHelpers.CoverUserAgent);
            request.Headers.TryAddWithoutValidation("Referer", "https://music.163.com/");

            byte[] bytes;
            string mediaType;
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            timeout.CancelAfter(CoverTimeout);
            try
            {
                using var response = await services.Http
                    .SendAsync(request, HttpCompletionOption.ResponseHeadersRead, timeout.Token)
                    .ConfigureAwait(false);
                if (!response.IsSuccessStatusCode)
                {
                    throw new ApiException(
                        StatusCodes.Status502BadGateway,
                        "封面下载失败：HTTP " + (int)response.StatusCode,
                        "cover_error");
                }

                mediaType = response.Content.Headers.ContentType?.MediaType ?? "image/jpeg";
                if (!mediaType.StartsWith("image/", StringComparison.OrdinalIgnoreCase))
                {
                    throw new ApiException(StatusCodes.Status502BadGateway, "封面地址返回的不是图片", "cover_error");
                }

                bytes = await response.Content.ReadAsByteArrayAsync(timeout.Token).ConfigureAwait(false);
            }
            catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
            {
                throw new ApiException(StatusCodes.Status504GatewayTimeout, "封面下载超时", "cover_timeout");
            }
            catch (HttpRequestException ex)
            {
                throw new ApiException(StatusCodes.Status502BadGateway, "封面下载失败：" + ex.Message, "cover_error");
            }

            if (bytes.Length == 0)
            {
                throw new ApiException(StatusCodes.Status502BadGateway, "封面内容为空", "cover_error");
            }

            if (bytes.Length > MaxCoverBytes)
            {
                throw new ApiException(StatusCodes.Status502BadGateway, "封面超过 20MB", "cover_error");
            }

            return Results.File(bytes, mediaType);
        });
    }

    private static string? FirstExistingDirectory(AppSettings settings)
    {
        var candidates = new[]
        {
            settings.LibraryDirectory,
            settings.DownloadDirectory,
            settings.NcmImportOutputDirectory,
        };
        foreach (var candidate in candidates)
        {
            if (!string.IsNullOrWhiteSpace(candidate) && Directory.Exists(candidate))
            {
                return candidate;
            }
        }

        return null;
    }

    private static string DefaultRootDirectory()
    {
        var profile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (!string.IsNullOrWhiteSpace(profile) && Directory.Exists(profile))
        {
            return profile;
        }

        return AppContext.BaseDirectory;
    }
}
