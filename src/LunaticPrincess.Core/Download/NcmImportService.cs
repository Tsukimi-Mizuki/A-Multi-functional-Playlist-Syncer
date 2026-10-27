using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Tags;

namespace LunaticPrincess.Core.Download;

/// <summary>
/// 批量导入本地 .ncm：解密为 mp3/flac，并把 NCM 内嵌元数据（标题/歌手/专辑）、封面与网易云 ID
/// 自动写入音频标签；可选在解锁成功后自动在线匹配标签、封面与歌词。单个文件失败不会中断整批。
/// </summary>
public sealed class NcmImportService : INcmImportService
{
    private readonly INcmDecoder _ncmDecoder;
    private readonly IAudioTagService _tagService;
    private readonly IBatchLyricsService? _batchLyrics;

    public NcmImportService(INcmDecoder ncmDecoder, IAudioTagService tagService, IBatchLyricsService? batchLyrics = null)
    {
        _ncmDecoder = ncmDecoder ?? throw new ArgumentNullException(nameof(ncmDecoder));
        _tagService = tagService ?? throw new ArgumentNullException(nameof(tagService));
        _batchLyrics = batchLyrics;
    }

    /// <inheritdoc />
    public async Task<IReadOnlyList<NcmDecryptResult>> ImportAsync(
        IReadOnlyList<string> ncmPaths,
        string outputDirectory,
        bool overwrite = false,
        IProgress<NcmImportProgress>? progress = null,
        CancellationToken cancellationToken = default,
        NcmImportOptions? options = null)
    {
        options ??= new NcmImportOptions();
        var matchOptions = options.MatchOptions ?? new BatchLyricsOptions(OverwriteExisting: true);
        ArgumentNullException.ThrowIfNull(ncmPaths);
        if (string.IsNullOrWhiteSpace(outputDirectory))
        {
            throw new ArgumentException("输出目录不能为空。", nameof(outputDirectory));
        }

        var targetDirectory = Path.GetFullPath(outputDirectory);
        Directory.CreateDirectory(targetDirectory);

        // 去重但保持输入顺序，避免同一文件被重复解密/写标签。
        var paths = new List<string>(ncmPaths.Count);
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var path in ncmPaths)
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                continue;
            }

            string fullPath;
            try
            {
                fullPath = Path.GetFullPath(path);
            }
            catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
            {
                paths.Add(path);
                continue;
            }

            if (seen.Add(fullPath))
            {
                paths.Add(fullPath);
            }
        }

        var total = paths.Count;
        var results = new List<NcmDecryptResult>(total);
        for (var index = 0; index < total; index++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var path = paths[index];
            var displayPath = Path.GetFileName(path);

            progress?.Report(new NcmImportProgress(index, total, displayPath, "正在解密 NCM…"));

            NcmDecryptResult result;
            try
            {
                result = await ImportOneAsync(path, targetDirectory, overwrite, cancellationToken)
                    .ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch (Exception ex)
            {
                result = new NcmDecryptResult(path, null, false, ErrorMessage: $"导入失败：{ex.Message}");
            }

            if (result.Success &&
                options.AutoMatchOnline &&
                _batchLyrics is not null &&
                result.OutputPath is { } decryptedAudio &&
                File.Exists(decryptedAudio))
            {
                progress?.Report(new NcmImportProgress(index, total, displayPath, "正在匹配在线标签、封面与歌词…"));
                try
                {
                    var match = await _batchLyrics
                        .FetchForFileAsync(decryptedAudio, matchOptions, cancellationToken)
                        .ConfigureAwait(false);
                    result = result with { EnrichmentMessage = DescribeMatch(match) };
                }
                catch (OperationCanceledException)
                {
                    throw;
                }
                catch (Exception ex)
                {
                    result = result with { EnrichmentMessage = $"在线匹配失败：{ex.Message}" };
                }
            }

            results.Add(result);
            progress?.Report(new NcmImportProgress(
                index + 1,
                total,
                displayPath,
                result.Success
                    ? result.EnrichmentMessage is null ? "完成" : $"完成 · {result.EnrichmentMessage}"
                    : result.ErrorMessage ?? "失败"));
        }

        return results;
    }

    private static string DescribeMatch(BatchLyricsResult match)
    {
        var source = string.IsNullOrWhiteSpace(match.SourceLabel) ? string.Empty : $"（{match.SourceLabel}）";
        var song = string.IsNullOrWhiteSpace(match.MatchedTitle)
            ? string.Empty
            : $"匹配「{match.MatchedTitle}{(string.IsNullOrWhiteSpace(match.MatchedArtist) ? string.Empty : $" - {match.MatchedArtist}")}」：";
        return match.Success || match.Skipped
            ? $"{song}{match.Message}{source}"
            : $"{song}在线匹配失败：{match.Message}{source}";
    }

    private async Task<NcmDecryptResult> ImportOneAsync(
        string ncmPath,
        string targetDirectory,
        bool overwrite,
        CancellationToken cancellationToken)
    {
        if (!File.Exists(ncmPath))
        {
            return new NcmDecryptResult(ncmPath, null, false, ErrorMessage: "文件不存在。");
        }

        bool isNcm;
        try
        {
            isNcm = _ncmDecoder.IsNcmFile(ncmPath);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            return new NcmDecryptResult(ncmPath, null, false, ErrorMessage: $"无法读取 NCM 文件头：{ex.Message}");
        }

        if (!isNcm)
        {
            return new NcmDecryptResult(ncmPath, null, false, ErrorMessage: "不是有效的 NCM 文件。");
        }

        var decrypted = await _ncmDecoder
            .DecryptAsync(ncmPath, targetDirectory, overwrite, cancellationToken)
            .ConfigureAwait(false);

        if (decrypted is null)
        {
            return new NcmDecryptResult(ncmPath, null, false, ErrorMessage: "解密器未返回结果。");
        }

        if (!decrypted.Success)
        {
            return decrypted;
        }

        if (string.IsNullOrWhiteSpace(decrypted.OutputPath) || !File.Exists(decrypted.OutputPath))
        {
            return decrypted with { Success = false, ErrorMessage = "解密输出文件不存在。" };
        }

        var tags = MediaFileHelper.CreateNcmTagData(decrypted.Metadata ?? NcmMetadata.Empty, decrypted.CoverData);
        var tagError = await TryWriteTagsAsync(decrypted.OutputPath!, tags, cancellationToken).ConfigureAwait(false);
        if (tagError is not null)
        {
            // 音频已解密成功但标签失败：保留输出文件并明确报告。
            return decrypted with { Success = false, ErrorMessage = $"标签写入失败（音频已解密保留）：{tagError}" };
        }

        return decrypted;
    }

    /// <summary>
    /// 先复制到同目录临时文件写标签，成功后再原子替换原文件；失败返回错误文本（原文件保持完整）。
    /// </summary>
    private async Task<string?> TryWriteTagsAsync(
        string audioPath,
        AudioTagData tags,
        CancellationToken cancellationToken)
    {
        var directory = Path.GetDirectoryName(audioPath) ?? string.Empty;
        var extension = Path.GetExtension(audioPath);
        var tempPath = Path.Combine(
            directory,
            $".{Path.GetFileNameWithoutExtension(audioPath)}.{Guid.NewGuid():N}.tagtmp{extension}");

        try
        {
            File.Copy(audioPath, tempPath, overwrite: true);
            await _tagService.WriteAsync(tempPath, tags, cancellationToken).ConfigureAwait(false);
            File.Move(tempPath, audioPath, overwrite: true);
            return null;
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            MediaFileHelper.TryDeleteFile(tempPath);
            return ex.Message;
        }
        finally
        {
            MediaFileHelper.TryDeleteFile(tempPath);
        }
    }
}
