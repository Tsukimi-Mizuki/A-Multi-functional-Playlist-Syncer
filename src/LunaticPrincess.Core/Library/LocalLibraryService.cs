using System.Text;
using System.Text.RegularExpressions;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Tags;

namespace LunaticPrincess.Core.Library;

/// <summary>
/// 本地曲库扫描器：递归遍历目录、按绝对路径去重（不跟随链接/junction 以防死循环），
/// 对 .ncm 读取解密头信息，对普通音频读取标签并在缺失时用「歌手 - 歌曲名」文件名回退。
/// 单个文件失败不会中断整批扫描。
/// </summary>
public sealed class LocalLibraryService : ILocalLibraryService
{
    /// <summary>扫描支持的扩展名。</summary>
    public static readonly IReadOnlyCollection<string> SupportedExtensions = Array.AsReadOnly(new[]
    {
        ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".ncm",
    });

    private static readonly HashSet<string> SupportedExtensionSet =
        new(SupportedExtensions, StringComparer.OrdinalIgnoreCase);

    private static readonly EnumerationOptions FileEnumerationOptions = new()
    {
        IgnoreInaccessible = true,
        RecurseSubdirectories = false,
        AttributesToSkip = FileAttributes.System,
    };

    private static readonly EnumerationOptions DirectoryEnumerationOptions = new()
    {
        IgnoreInaccessible = true,
        RecurseSubdirectories = false,
        AttributesToSkip = FileAttributes.System | FileAttributes.ReparsePoint,
    };

    // 可选的前导音轨号：「01.」「02 - 」「3)」等。
    private static readonly Regex TrackNumberPrefix = new(
        @"^\s*\d{1,3}\s*[\.\-_、\)\]]\s*",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    // 「01 Song」这类只有空格分隔的两位音轨号。
    private static readonly Regex TwoDigitTrackPrefix = new(
        @"^\s*\d{1,2}\s+",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    private static readonly Regex ArtistTitleSeparator = new(
        @"\s+[-–—－]\s+",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    private readonly INcmDecoder _ncmDecoder;
    private readonly IAudioTagService _tagService;

    public LocalLibraryService(INcmDecoder ncmDecoder, IAudioTagService tagService)
    {
        _ncmDecoder = ncmDecoder ?? throw new ArgumentNullException(nameof(ncmDecoder));
        _tagService = tagService ?? throw new ArgumentNullException(nameof(tagService));
    }

    /// <inheritdoc />
    public async Task<IReadOnlyList<LocalTrack>> ScanAsync(
        IEnumerable<string> directories,
        IProgress<LibraryScanProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(directories);

        var roots = ResolveRoots(directories);
        var files = CollectFiles(roots, cancellationToken);

        var total = files.Count;
        progress?.Report(new LibraryScanProgress(0, total, total == 0 ? string.Empty : files[0]));

        var tracks = new List<LocalTrack>(total);
        for (var i = 0; i < total; i++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var file = files[i];
            try
            {
                tracks.Add(await ReadTrackAsync(file, cancellationToken).ConfigureAwait(false));
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch (Exception ex) when (IsRecoverable(ex))
            {
                // 标签/NCM 头损坏时仍以文件名回退展示，避免整个扫描失败。
                tracks.Add(BuildFallbackTrack(file));
            }

            progress?.Report(new LibraryScanProgress(i + 1, total, file));
        }

        return tracks;
    }

    private static List<string> ResolveRoots(IEnumerable<string> directories)
    {
        var roots = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var directory in directories)
        {
            if (string.IsNullOrWhiteSpace(directory))
            {
                continue;
            }

            string fullPath;
            try
            {
                fullPath = Path.GetFullPath(directory);
            }
            catch (Exception ex) when (IsRecoverable(ex))
            {
                continue;
            }

            if (!Directory.Exists(fullPath))
            {
                continue;
            }

            var key = Path.TrimEndingDirectorySeparator(fullPath);
            if (seen.Add(key))
            {
                roots.Add(fullPath);
            }
        }

        return roots;
    }

    private static List<string> CollectFiles(IReadOnlyList<string> roots, CancellationToken cancellationToken)
    {
        var files = new List<string>();
        var seenFiles = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var visitedDirectories = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        foreach (var root in roots)
        {
            var pending = new Stack<string>();
            pending.Push(root);

            while (pending.Count > 0)
            {
                cancellationToken.ThrowIfCancellationRequested();
                var directory = pending.Pop();

                string canonical;
                try
                {
                    canonical = Path.TrimEndingDirectorySeparator(Path.GetFullPath(directory));
                }
                catch (Exception ex) when (IsRecoverable(ex))
                {
                    continue;
                }

                if (!visitedDirectories.Add(canonical))
                {
                    continue;
                }

                try
                {
                    foreach (var file in Directory.EnumerateFiles(directory, "*", FileEnumerationOptions))
                    {
                        if (!SupportedExtensionSet.Contains(Path.GetExtension(file)))
                        {
                            continue;
                        }

                        var fullPath = Path.GetFullPath(file);
                        if (seenFiles.Add(fullPath))
                        {
                            files.Add(fullPath);
                        }
                    }
                }
                catch (Exception ex) when (IsRecoverable(ex))
                {
                    // 权限/IO 错误：跳过该目录，继续其它目录。
                }

                try
                {
                    foreach (var child in Directory.EnumerateDirectories(directory, "*", DirectoryEnumerationOptions))
                    {
                        pending.Push(child);
                    }
                }
                catch (Exception ex) when (IsRecoverable(ex))
                {
                    // 子目录无法枚举时忽略。
                }
            }
        }

        files.Sort(StringComparer.OrdinalIgnoreCase);
        return files;
    }

    private async Task<LocalTrack> ReadTrackAsync(string file, CancellationToken cancellationToken)
    {
        var extension = Path.GetExtension(file).ToLowerInvariant();
        if (extension == ".ncm")
        {
            return await ReadNcmTrackAsync(file, cancellationToken).ConfigureAwait(false);
        }

        return await ReadAudioTrackAsync(file, extension, cancellationToken).ConfigureAwait(false);
    }

    private async Task<LocalTrack> ReadNcmTrackAsync(string file, CancellationToken cancellationToken)
    {
        var extension = Path.GetExtension(file).ToLowerInvariant();
        NcmMetadata? metadata = null;
        try
        {
            if (_ncmDecoder.IsNcmFile(file))
            {
                var info = await _ncmDecoder.ReadInfoAsync(file, cancellationToken).ConfigureAwait(false);
                metadata = info?.Metadata;
            }
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex) when (IsRecoverable(ex))
        {
            metadata = null;
        }

        var fallback = ParseFileName(file);
        return new LocalTrack
        {
            Path = file,
            Title = FirstNonEmpty(metadata?.Title, fallback.Title) ?? fallback.Title ?? "未知",
            Artist = FirstNonEmpty(metadata?.Artists, fallback.Artist),
            Album = NullIfEmpty(metadata?.Album),
            DurationMs = metadata?.DurationMs > 0 ? metadata.DurationMs : 0,
            Format = FirstNonEmpty(metadata?.Format, extension.TrimStart('.')),
            Bitrate = metadata?.Bitrate > 0 ? metadata.Bitrate : null,
            IsNcm = true,
            NcmMetadata = metadata,
            NeteaseId = metadata?.MusicId is > 0 ? metadata.MusicId : null,
            // NCM 封面在加密音频段内，仅读头信息不进行解密，因此扫描阶段未知。
            HasCover = false,
        };
    }

    private async Task<LocalTrack> ReadAudioTrackAsync(string file, string extension, CancellationToken cancellationToken)
    {
        AudioTagInfo? info = null;
        try
        {
            info = await _tagService.ReadAsync(file, cancellationToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex) when (IsRecoverable(ex))
        {
            info = null;
        }

        var fallback = ParseFileName(file);
        return new LocalTrack
        {
            Path = file,
            Title = FirstNonEmpty(info?.Title, fallback.Title) ?? fallback.Title ?? "未知",
            Artist = FirstNonEmpty(info?.Artist, fallback.Artist),
            Album = NullIfEmpty(info?.Album),
            DurationMs = info?.DurationMs > 0 ? info.DurationMs : 0,
            Format = extension.TrimStart('.'),
            Bitrate = info?.Bitrate > 0 ? info.Bitrate : null,
            IsNcm = false,
            NcmMetadata = null,
            NeteaseId = info?.NeteaseId is > 0 ? info.NeteaseId : null,
            HasCover = info?.HasCover ?? false,
        };
    }

    private LocalTrack BuildFallbackTrack(string file)
    {
        var extension = Path.GetExtension(file).ToLowerInvariant();
        var isNcm = extension == ".ncm";
        var fallback = ParseFileName(file);
        return new LocalTrack
        {
            Path = file,
            Title = fallback.Title ?? "未知",
            Artist = fallback.Artist,
            Format = extension.TrimStart('.'),
            IsNcm = isNcm,
            HasCover = false,
        };
    }

    /// <summary>
    /// 从文件名推断「歌手 - 歌曲名」；支持中文/全角连字符，并去掉前导音轨号。
    /// 无法拆分时只返回歌曲名。
    /// </summary>
    internal static (string? Title, string? Artist) ParseFileName(string path)
    {
        var stem = Path.GetFileNameWithoutExtension(path).Trim();
        if (stem.Length == 0)
        {
            return (null, null);
        }

        stem = TrackNumberPrefix.Replace(stem, string.Empty, 1).Trim();
        stem = TwoDigitTrackPrefix.Replace(stem, string.Empty, 1).Trim();
        if (stem.Length == 0)
        {
            return (null, null);
        }

        var parts = ArtistTitleSeparator.Split(stem, 2);
        if (parts.Length == 2 && !string.IsNullOrWhiteSpace(parts[0]) && !string.IsNullOrWhiteSpace(parts[1]))
        {
            return (parts[1].Trim(), parts[0].Trim());
        }

        return (stem, null);
    }

    private static string? FirstNonEmpty(params string?[] values)
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

    private static string? NullIfEmpty(string? value) =>
        string.IsNullOrWhiteSpace(value) ? null : value.Trim();

    /// <summary>
    /// 单文件/单目录失败可恢复：TagLib 的 CorruptFileException、IO、权限、路径异常等都不应中断整批扫描；
    /// 只把取消与真正致命的异常继续抛出。
    /// </summary>
    private static bool IsRecoverable(Exception exception) =>
        exception is not OperationCanceledException
            and not OutOfMemoryException
            and not StackOverflowException
            and not AccessViolationException;
}
