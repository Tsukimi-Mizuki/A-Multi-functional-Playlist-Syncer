using System.Buffers;
using System.Net;
using System.Net.Http;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Tags;
using LunaticPrincess.Core.Utilities;

namespace LunaticPrincess.Core.Download;

/// <summary>
/// 下载/导入/同步共用的媒体文件工具：扩展名推断、NCM 魔数识别、封面 MIME、唯一文件名、
/// HTTP 下载、标签原子写入与临时文件清理。
/// </summary>
internal static class MediaFileHelper
{
    /// <summary>可播放音频扩展名（不含 .ncm）。</summary>
    internal static readonly IReadOnlySet<string> AudioExtensions = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
    {
        ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav",
    };

    private static readonly HashSet<string> KnownContainerExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aac",
    };

    private static readonly byte[] NcmMagic = { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D }; // "CTENFDAM"

    private static readonly byte[] PngMagic = { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A };
    private static readonly byte[] GifMagic = { 0x47, 0x49, 0x46, 0x38 };

    /// <summary>是否为网易云 .ncm 文件头（CTENFDAM）。</summary>
    internal static bool IsNcmMagic(ReadOnlySpan<byte> header)
    {
        if (header.Length < NcmMagic.Length)
        {
            return false;
        }

        for (var i = 0; i < NcmMagic.Length; i++)
        {
            if (header[i] != NcmMagic[i])
            {
                return false;
            }
        }

        return true;
    }

    /// <summary>读取文件前 8 字节判断是否为 NCM。文件过短/不可读时返回 false。</summary>
    internal static async Task<bool> HasNcmMagicAsync(string path, CancellationToken cancellationToken)
    {
        try
        {
            var buffer = new byte[NcmMagic.Length];
            await using var stream = new FileStream(
                path,
                FileMode.Open,
                FileAccess.Read,
                FileShare.Read,
                buffer.Length,
                useAsync: true);

            var read = 0;
            while (read < buffer.Length)
            {
                var count = await stream.ReadAsync(buffer.AsMemory(read, buffer.Length - read), cancellationToken)
                    .ConfigureAwait(false);
                if (count == 0)
                {
                    break;
                }

                read += count;
            }

            return IsNcmMagic(buffer.AsSpan(0, read));
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }

    /// <summary>
    /// 由接口返回的 Format 与下载 URL 推断最终扩展名；两者都不可用时回退 .mp3。
    /// 注意：请求无损但服务端可能返回 mp3，必须使用实际返回的 Format。
    /// </summary>
    internal static string ResolveExtension(string? format, string? url)
    {
        var fromFormat = NormalizeExtension(format);
        if (fromFormat is not null)
        {
            return fromFormat;
        }

        var fromUrl = NormalizeExtension(GetUrlExtension(url));
        return fromUrl ?? ".mp3";
    }

    /// <summary>取文件路径/URL 的扩展名（含点、小写），未知时返回 null。</summary>
    internal static string? ResolveContainerExtension(string? pathOrUrl)
    {
        if (string.IsNullOrWhiteSpace(pathOrUrl))
        {
            return null;
        }

        try
        {
            return NormalizeExtension(Path.GetExtension(pathOrUrl));
        }
        catch (ArgumentException)
        {
            return null;
        }
    }

    private static string? GetUrlExtension(string? url)
    {
        if (string.IsNullOrWhiteSpace(url))
        {
            return null;
        }

        if (Uri.TryCreate(url, UriKind.Absolute, out var uri))
        {
            return Path.GetExtension(uri.AbsolutePath);
        }

        return Path.GetExtension(url.Split('?', '#')[0]);
    }

    private static string? NormalizeExtension(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        var trimmed = value.Trim().Trim('.').ToLowerInvariant();
        if (trimmed.Length is < 2 or > 5)
        {
            return null;
        }

        foreach (var c in trimmed)
        {
            if (!char.IsLetterOrDigit(c))
            {
                return null;
            }
        }

        var extension = "." + trimmed;
        return KnownContainerExtensions.Contains(extension) ? extension : null;
    }

    /// <summary>依据图片文件头识别封面 MIME；未知按 JPEG 处理。</summary>
    internal static string SniffImageMime(ReadOnlySpan<byte> data)
    {
        if (data.Length >= PngMagic.Length && data[..PngMagic.Length].SequenceEqual(PngMagic))
        {
            return "image/png";
        }

        if (data.Length >= GifMagic.Length && data[..GifMagic.Length].SequenceEqual(GifMagic))
        {
            return "image/gif";
        }

        return "image/jpeg";
    }

    /// <summary>从 releaseDate（yyyy-MM-dd 等）解析年份。</summary>
    internal static int ParseYear(string? releaseDate)
    {
        if (string.IsNullOrWhiteSpace(releaseDate) || releaseDate.Length < 4)
        {
            return 0;
        }

        var yearText = releaseDate[..4];
        foreach (var c in yearText)
        {
            if (!char.IsDigit(c))
            {
                return 0;
            }
        }

        return int.TryParse(yearText, out var year) && year is > 1900 and < 2200 ? year : 0;
    }

    /// <summary>按 / &amp; 、 ; 等拆分 NCM 元数据里的歌手串。</summary>
    internal static IReadOnlyList<string> SplitArtists(string? artists)
    {
        if (string.IsNullOrWhiteSpace(artists))
        {
            return Array.Empty<string>();
        }

        var result = new List<string>();
        foreach (var token in artists.Split(
                     new[] { '/', '&', '、', ';', '；', ',', '，' },
                     StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            if (!string.IsNullOrWhiteSpace(token))
            {
                result.Add(token.Trim());
            }
        }

        return result;
    }

    /// <summary>
    /// 按文件名模板生成不含扩展名的安全文件名。
    /// 先带 .mp3 生成再由 <see cref="Path.GetFileNameWithoutExtension"/> 去掉扩展名，
    /// 以保证与 <see cref="FileNameSanitizer.BuildFileName"/> 的清理/截断规则一致。
    /// </summary>
    internal static string BuildBaseName(
        DownloadOptions options,
        string? title,
        string? artist,
        string? album,
        int trackNumber,
        int year,
        long songId)
    {
        var fileName = FileNameSanitizer.BuildFileName(
            options.FileNameTemplate,
            title,
            artist,
            album,
            trackNumber,
            year,
            songId,
            "mp3");
        var baseName = Path.GetFileNameWithoutExtension(fileName);
        return string.IsNullOrWhiteSpace(baseName) ? "未知歌曲" : baseName;
    }

    /// <summary>按「模板基名 + 扩展名」直接得到目标文件名（不做唯一化）。</summary>
    internal static string BuildFinalFileName(string baseName, string extension) => baseName + extension;

    /// <summary>在目录中查找同基名且为音频扩展名的已有文件（用于 SkipExisting 判断）。</summary>
    internal static string? FindExistingAudioFile(string directory, string baseName)
    {
        if (!Directory.Exists(directory))
        {
            return null;
        }

        try
        {
            foreach (var file in Directory.EnumerateFiles(directory, "*", SearchOption.TopDirectoryOnly))
            {
                if (!AudioExtensions.Contains(Path.GetExtension(file)))
                {
                    continue;
                }

                if (string.Equals(Path.GetFileNameWithoutExtension(file), baseName, StringComparison.OrdinalIgnoreCase))
                {
                    return file;
                }
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return null;
        }

        return null;
    }

    /// <summary>
    /// 把已下载/已解密的音频原子写入目标目录：先移动到临时文件写标签，再移动到最终文件名；
    /// 标签写入失败时保留未打标签的音频。返回是否成功写标签与最终路径。
    /// </summary>
    internal static async Task<(bool Tagged, string OutputPath)> WriteTagsAtomicallyAsync(
        IAudioTagService tagService,
        string sourceAudioPath,
        string targetDirectory,
        string baseName,
        string extension,
        long? songId,
        AudioTagData tags,
        bool overwriteExisting,
        CancellationToken cancellationToken)
    {
        var tempStem = SafeTempStem(baseName);
        var tempPath = Path.Combine(targetDirectory, $".{tempStem}.{Guid.NewGuid():N}.tmp{extension}");
        File.Move(sourceAudioPath, tempPath);

        try
        {
            var tagged = false;
            try
            {
                await tagService.WriteAsync(tempPath, tags, cancellationToken).ConfigureAwait(false);
                tagged = true;
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch (Exception)
            {
                // 标签写入失败不应丢弃音频文件；交给调用方在进度里提示。
                tagged = false;
            }

            string outputPath;
            if (overwriteExisting)
            {
                outputPath = Path.Combine(targetDirectory, BuildFinalFileName(baseName, extension));
                File.Move(tempPath, outputPath, overwrite: true);
            }
            else
            {
                outputPath = MoveToUniquePath(tempPath, targetDirectory, baseName, extension, songId);
            }

            return (tagged, outputPath);
        }
        finally
        {
            TryDeleteFile(tempPath);
        }
    }

    /// <summary>移动到目录内唯一的文件名；冲突时使用 (歌曲ID) 与 (2)/(3)… 后缀。</summary>
    internal static string MoveToUniquePath(
        string sourcePath,
        string targetDirectory,
        string baseName,
        string extension,
        long? songId)
    {
        for (var attempt = 0; attempt < 512; attempt++)
        {
            var candidate = BuildCandidatePath(targetDirectory, baseName, extension, songId, attempt);
            try
            {
                File.Move(sourcePath, candidate, overwrite: false);
                return candidate;
            }
            catch (IOException) when (File.Exists(candidate))
            {
                // 并发下载/目录中已有同名文件，尝试下一个候选名。
            }
        }

        throw new IOException($"无法为「{baseName}」生成唯一的文件名。");
    }

    private static string BuildCandidatePath(
        string directory,
        string baseName,
        string extension,
        long? songId,
        int attempt)
    {
        if (attempt == 0)
        {
            return Path.Combine(directory, BuildFinalFileName(baseName, extension));
        }

        if (songId is long id)
        {
            var prefixed = $"{baseName} ({id})";
            var name = attempt == 1 ? prefixed : $"{prefixed} ({attempt})";
            return Path.Combine(directory, BuildFinalFileName(name, extension));
        }

        return Path.Combine(directory, $"{baseName} ({attempt + 1}){extension}");
    }

    /// <summary>HTTP 下载媒体到本地文件，回调 (已下载字节, 总字节?)。失败抛 HttpRequestException/IOException。</summary>
    internal static async Task<long> DownloadToFileAsync(
        HttpClient httpClient,
        string url,
        string destinationPath,
        Action<long, long?>? progress,
        CancellationToken cancellationToken)
    {
        using var response = await httpClient
            .GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cancellationToken)
            .ConfigureAwait(false);

        if (!response.IsSuccessStatusCode)
        {
            throw new HttpRequestException(
                $"媒体下载失败：HTTP {(int)response.StatusCode} {response.ReasonPhrase}",
                inner: null,
                statusCode: response.StatusCode);
        }

        var totalBytes = response.Content.Headers.ContentLength;
        if (totalBytes is > 0)
        {
            progress?.Invoke(0, totalBytes);
        }

        await using var source = await response.Content.ReadAsStreamAsync(cancellationToken).ConfigureAwait(false);
        await using var destination = new FileStream(
            destinationPath,
            FileMode.Create,
            FileAccess.Write,
            FileShare.None,
            bufferSize: 81920,
            useAsync: true);

        var buffer = ArrayPool<byte>.Shared.Rent(81920);
        try
        {
            long written = 0;
            int read;
            while ((read = await source.ReadAsync(buffer.AsMemory(0, buffer.Length), cancellationToken).ConfigureAwait(false)) > 0)
            {
                await destination.WriteAsync(buffer.AsMemory(0, read), cancellationToken).ConfigureAwait(false);
                written += read;
                progress?.Invoke(written, totalBytes);
            }

            return written;
        }
        finally
        {
            ArrayPool<byte>.Shared.Return(buffer);
        }
    }

    /// <summary>下载封面等小文件；任何失败（含超时）返回 null，不打断主流程；调用方取消时抛出。</summary>
    internal static async Task<byte[]?> TryDownloadBytesAsync(
        HttpClient httpClient,
        string? url,
        CancellationToken cancellationToken,
        long maxBytes = 10 * 1024 * 1024)
    {
        if (string.IsNullOrWhiteSpace(url))
        {
            return null;
        }

        try
        {
            using var response = await httpClient
                .GetAsync(url, HttpCompletionOption.ResponseHeadersRead, cancellationToken)
                .ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                return null;
            }

            var length = response.Content.Headers.ContentLength;
            if (length is > 0 && length > maxBytes)
            {
                return null;
            }

            var bytes = await response.Content.ReadAsByteArrayAsync(cancellationToken).ConfigureAwait(false);
            return bytes.Length is > 0 && bytes.Length <= maxBytes ? bytes : null;
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return null;
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex) when (ex is HttpRequestException or IOException or InvalidOperationException)
        {
            return null;
        }
    }

    /// <summary>构造下载/同步写标签用的元数据。</summary>
    internal static AudioTagData CreateTagData(
        NeteaseSong song,
        byte[]? coverData,
        string? plainLyrics,
        string? wordLyrics)
    {
        var artists = song.Artists
            .Select(a => a.Name?.Trim())
            .Where(name => !string.IsNullOrWhiteSpace(name))
            .Select(name => name!)
            .ToArray();

        return new AudioTagData
        {
            Title = NullIfEmpty(song.Name),
            Artists = artists,
            Album = NullIfEmpty(song.Album.Name),
            AlbumArtist = artists.Length > 0 ? artists[0] : null,
            TrackNumber = song.TrackNumber > 0 ? (uint)song.TrackNumber : null,
            DiscNumber = song.DiscNumber > 0 ? (uint)song.DiscNumber : null,
            Year = ParseYear(song.ReleaseDate) is int year and > 0 ? (uint)year : null,
            CoverData = coverData is { Length: > 0 } ? coverData : null,
            CoverMimeType = coverData is { Length: > 0 } cover ? SniffImageMime(cover) : null,
            PlainLyrics = NullIfEmpty(plainLyrics),
            WordLyrics = NullIfEmpty(wordLyrics),
            NeteaseId = song.Id > 0 ? song.Id : null,
            Comment = "LunaticPrincess",
        };
    }

    /// <summary>构造 NCM 导入用的标签数据（NCM 内嵌元数据 + 封面）。</summary>
    internal static AudioTagData CreateNcmTagData(NcmMetadata metadata, byte[]? coverData)
    {
        var artists = SplitArtists(metadata.Artists);
        return new AudioTagData
        {
            Title = NullIfEmpty(metadata.Title),
            Artists = artists,
            Album = NullIfEmpty(metadata.Album),
            AlbumArtist = artists.Count > 0 ? artists[0] : null,
            CoverData = coverData is { Length: > 0 } ? coverData : null,
            CoverMimeType = coverData is { Length: > 0 } cover ? SniffImageMime(cover) : null,
            NeteaseId = metadata.MusicId is long id and > 0 ? id : null,
            Comment = "LunaticPrincess",
        };
    }

    internal static string? NullIfEmpty(string? value) =>
        string.IsNullOrWhiteSpace(value) ? null : value.Trim();

    internal static string? FirstNonEmpty(params string?[] values)
    {
        foreach (var value in values)
        {
            if (!string.IsNullOrWhiteSpace(value))
            {
                return value.Trim();
            }
        }

        return null;
    }

    internal static void TryDeleteFile(string? path)
    {
        if (string.IsNullOrEmpty(path))
        {
            return;
        }

        try
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // 清理临时文件失败不影响主流程。
        }
    }

    private static string SafeTempStem(string baseName)
    {
        var stem = FileNameSanitizer.Sanitize(baseName, 80);
        return string.IsNullOrWhiteSpace(stem) ? "track" : stem;
    }
}

/// <summary>把回调包装成 <see cref="IProgress{T}"/>，用于同步转接进度。</summary>
internal sealed class DelegateProgress<T> : IProgress<T>
{
    private readonly Action<T> _handler;

    public DelegateProgress(Action<T> handler)
    {
        _handler = handler ?? throw new ArgumentNullException(nameof(handler));
    }

    public void Report(T value) => _handler(value);
}
