using LunaticPrincess.Core.Download;
using LunaticPrincess.Core.Library;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Core.Tags;
using LunaticPrincess.Core.Utilities;

namespace LunaticPrincess.Core.Sync;

/// <summary>
/// 一键同步：
/// 本地已有 NCM → 解密并重新写入在线元数据/封面/歌词（<see cref="SyncSourceKind.DecryptedNcm"/>）；
/// 本地已有普通音频且未要求重新下载 → 跳过；
/// 否则调用 <see cref="IDownloadService"/> 下载单曲。
/// 每个请求恰好产出一个结果，顺序与输入一致；支持取消与聚合进度。
/// </summary>
public sealed class MusicSyncService : ISyncService
{
    private readonly INeteaseApiClient _api;
    private readonly INcmDecoder _ncmDecoder;
    private readonly IAudioTagService _tagService;
    private readonly ILyricParser _lyricParser;
    private readonly INcmImportService _importer;
    private readonly IDownloadService _downloader;
    private readonly HttpClient _httpClient;
    private readonly IBatchLyricsService? _batchLyrics;

    public MusicSyncService(
        INeteaseApiClient api,
        INcmDecoder ncmDecoder,
        IAudioTagService tagService,
        ILyricParser lyricParser,
        INcmImportService importer,
        IDownloadService downloader,
        HttpClient httpClient,
        IBatchLyricsService? batchLyrics = null)
    {
        _api = api ?? throw new ArgumentNullException(nameof(api));
        _ncmDecoder = ncmDecoder ?? throw new ArgumentNullException(nameof(ncmDecoder));
        _tagService = tagService ?? throw new ArgumentNullException(nameof(tagService));
        _lyricParser = lyricParser ?? throw new ArgumentNullException(nameof(lyricParser));
        _importer = importer ?? throw new ArgumentNullException(nameof(importer));
        _downloader = downloader ?? throw new ArgumentNullException(nameof(downloader));
        _httpClient = httpClient ?? throw new ArgumentNullException(nameof(httpClient));
        _batchLyrics = batchLyrics;
    }

    /// <inheritdoc />
    public async Task<SyncSummary> SyncAsync(
        IReadOnlyList<SyncRequest> requests,
        SyncOptions options,
        IProgress<SyncProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(requests);
        ArgumentNullException.ThrowIfNull(options);
        ArgumentNullException.ThrowIfNull(options.Download);
        if (string.IsNullOrWhiteSpace(options.Download.TargetDirectory))
        {
            throw new ArgumentException("同步目标目录不能为空。", nameof(options));
        }

        Directory.CreateDirectory(Path.GetFullPath(options.Download.TargetDirectory));

        var total = requests.Count;
        if (total == 0)
        {
            return new SyncSummary();
        }

        var results = new SyncItemResult[total];
        for (var index = 0; index < total; index++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var request = requests[index];
            if (request?.Song is null)
            {
                results[index] = new SyncItemResult(0, "未知歌曲", false, SyncSourceKind.Failed, null, "同步请求为空。");
                progress?.Report(new SyncProgress(index + 1, total, "未知歌曲", "失败", "同步请求为空。"));
                continue;
            }

            var songName = DisplayName(request.Song);
            progress?.Report(new SyncProgress(index, total, songName, "处理中", "正在同步…"));

            try
            {
                results[index] = await SyncOneAsync(request, options, progress, index, total, cancellationToken)
                    .ConfigureAwait(false);
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch (Exception ex)
            {
                results[index] = new SyncItemResult(
                    request.Song.Id,
                    songName,
                    false,
                    SyncSourceKind.Failed,
                    null,
                    ErrorText.DescribeWrite(ex, options.Download.TargetDirectory));
            }

            var result = results[index];
            progress?.Report(new SyncProgress(
                index + 1,
                total,
                songName,
                StageOf(result),
                result.Message,
                result.Success ? 100 : null));
        }

        return new SyncSummary { Results = results.Select(r => r!).ToArray() };
    }

    private async Task<SyncItemResult> SyncOneAsync(
        SyncRequest request,
        SyncOptions options,
        IProgress<SyncProgress>? progress,
        int completed,
        int total,
        CancellationToken cancellationToken)
    {
        var song = request.Song;
        var songName = DisplayName(song);
        var local = request.LocalMatch;

        if (local is not null && local.IsNcm)
        {
            return await SyncFromNcmAsync(request, options, progress, completed, total, cancellationToken)
                .ConfigureAwait(false);
        }

        if (local is not null && !options.RedownloadExisting)
        {
            return new SyncItemResult(song.Id, songName, true, SyncSourceKind.Skipped, local.Path, "本地已存在，跳过。");
        }

        return await SyncViaDownloaderAsync(request, options, progress, completed, total, cancellationToken)
            .ConfigureAwait(false);
    }

    private async Task<SyncItemResult> SyncFromNcmAsync(
        SyncRequest request,
        SyncOptions options,
        IProgress<SyncProgress>? progress,
        int completed,
        int total,
        CancellationToken cancellationToken)
    {
        var song = request.Song;
        var songName = DisplayName(song);
        var local = request.LocalMatch!;
        var ncmPath = local.Path;

        void Report(string stage, string? message = null, double? percent = null) =>
            progress?.Report(new SyncProgress(completed, total, songName, stage, message, percent));

        if (string.IsNullOrWhiteSpace(ncmPath) || !File.Exists(ncmPath))
        {
            return Failed(song, songName, $"本地 NCM 文件不存在：{ncmPath}");
        }

        var targetDirectory = Path.GetFullPath(options.Download.TargetDirectory);
        Report("解密 NCM", "正在解密本地 NCM…");

        // 头部元数据只作为在线数据缺失时的回退，读取失败不影响解密。
        NcmMetadata? ncmMetadata = null;
        try
        {
            var info = await _ncmDecoder.ReadInfoAsync(ncmPath, cancellationToken).ConfigureAwait(false);
            ncmMetadata = info?.Metadata;
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception)
        {
            ncmMetadata = null;
        }

        NcmDecryptResult? imported;
        try
        {
            var importResults = await _importer.ImportAsync(
                    new[] { ncmPath },
                    targetDirectory,
                    overwrite: true,
                    new DelegateProgress<NcmImportProgress>(p => Report(p.Message ?? "正在解密 NCM…")),
                    cancellationToken)
                .ConfigureAwait(false);
            imported = importResults?.FirstOrDefault();
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            return Failed(song, songName, $"NCM 解密失败：{ErrorText.DescribeWrite(ex, targetDirectory)}");
        }

        if (imported is null)
        {
            return Failed(song, songName, "NCM 解密失败：导入器未返回结果。");
        }

        if (!imported.Success)
        {
            return Failed(song, songName, imported.ErrorMessage ?? "NCM 解密失败。");
        }

        if (string.IsNullOrWhiteSpace(imported.OutputPath) || !File.Exists(imported.OutputPath))
        {
            return Failed(song, songName, "NCM 解密输出文件不存在。");
        }

        var decryptedPath = imported.OutputPath!;
        try
        {
            Report("获取歌曲信息", "正在获取在线元数据/歌词/封面…");
            var enriched = await TryFetchSongAsync(song, cancellationToken).ConfigureAwait(false) ?? song;

            var (plainLyrics, wordLyrics) = await TryBuildLyricsAsync(
                    song.Id,
                    options.Download.LyricsMode,
                    cancellationToken)
                .ConfigureAwait(false);

            byte[]? coverData = null;
            if (options.Download.WriteCover)
            {
                coverData = imported.CoverData is { Length: > 0 }
                    ? imported.CoverData
                    : await MediaFileHelper.TryDownloadBytesAsync(
                            _httpClient,
                            enriched.CoverUrl ?? song.CoverUrl,
                            cancellationToken)
                        .ConfigureAwait(false);
            }

            var tags = MediaFileHelper.CreateTagData(enriched, coverData, plainLyrics, wordLyrics);

            // 在线字段缺失时回退到 NCM 内嵌元数据/本地扫描结果，保证文件名可用。
            var title = MediaFileHelper.FirstNonEmpty(enriched.Name, ncmMetadata?.Title, local.Title)
                        ?? MediaFileHelper.FirstNonEmpty(song.Name)
                        ?? "未知歌曲";
            var artist = MediaFileHelper.FirstNonEmpty(enriched.ArtistText, ncmMetadata?.Artists, local.Artist)
                         ?? MediaFileHelper.FirstNonEmpty(song.ArtistText);
            var album = MediaFileHelper.FirstNonEmpty(enriched.Album.Name, ncmMetadata?.Album);
            var trackNumber = enriched.TrackNumber > 0 ? enriched.TrackNumber : 0;
            var year = MediaFileHelper.ParseYear(enriched.ReleaseDate);
            var extension = MediaFileHelper.ResolveContainerExtension(decryptedPath) ?? ".mp3";
            var baseName = MediaFileHelper.BuildBaseName(
                options.Download,
                title,
                artist,
                album,
                trackNumber,
                year,
                song.Id);

            Report("写入标签", "正在写入元数据/封面/歌词…");
            var (tagged, outputPath) = await MediaFileHelper.WriteTagsAtomicallyAsync(
                    _tagService,
                    decryptedPath,
                    targetDirectory,
                    baseName,
                    extension,
                    song.Id,
                    tags,
                    options.Download.OverwriteExisting,
                    cancellationToken)
                .ConfigureAwait(false);

            var warning = tagged ? null : "标签写入失败，已保留解密音频。";
            var enrichment = await TryEnrichAsync(
                    outputPath,
                    options,
                    stage => Report("匹配在线歌词", stage),
                    cancellationToken)
                .ConfigureAwait(false);
            if (enrichment is not null)
            {
                warning = warning is null ? enrichment : warning + "；" + enrichment;
            }

            Report("完成", warning, 100);
            return new SyncItemResult(song.Id, songName, true, SyncSourceKind.DecryptedNcm, outputPath, warning);
        }
        catch (OperationCanceledException)
        {
            MediaFileHelper.TryDeleteFile(decryptedPath);
            throw;
        }
        catch (Exception ex)
        {
            MediaFileHelper.TryDeleteFile(decryptedPath);
            return Failed(song, songName, $"NCM 解密后处理失败：{ErrorText.DescribeWrite(ex, targetDirectory)}");
        }
    }

    private async Task<SyncItemResult> SyncViaDownloaderAsync(
        SyncRequest request,
        SyncOptions options,
        IProgress<SyncProgress>? progress,
        int completed,
        int total,
        CancellationToken cancellationToken)
    {
        var song = request.Song;
        var songName = DisplayName(song);
        var downloadProgress = new DelegateProgress<DownloadProgress>(p =>
            progress?.Report(new SyncProgress(completed, total, p.SongName, p.Stage, p.Message, p.Percent)));

        var summary = await _downloader
            .DownloadAsync(new[] { song }, options.Download, downloadProgress, cancellationToken)
            .ConfigureAwait(false);

        var item = summary?.Results.FirstOrDefault(r => r.SongId == song.Id)
                   ?? summary?.Results.FirstOrDefault();
        if (item is null)
        {
            return Failed(song, songName, "下载器未返回结果。");
        }

        if (item.Skipped)
        {
            return new SyncItemResult(
                song.Id,
                songName,
                true,
                SyncSourceKind.Skipped,
                item.OutputPath,
                item.ErrorMessage ?? "已存在，跳过。");
        }

        if (!item.Success)
        {
            return Failed(song, songName, item.ErrorMessage ?? "下载失败。");
        }

        return new SyncItemResult(song.Id, songName, true, SyncSourceKind.Downloaded, item.OutputPath, item.ErrorMessage);
    }

    /// <summary>
    /// 解密并写入基础标签后，按「QQ 音乐逐字歌词优先、网易云音乐兜底」在线补全标签、封面与歌词。
    /// 补全失败只作为提示返回，不影响同步成功。
    /// </summary>
    private async Task<string?> TryEnrichAsync(
        string? filePath,
        SyncOptions options,
        Action<string> report,
        CancellationToken cancellationToken)
    {
        if (!options.Download.AutoMatchOnline
            || _batchLyrics is null
            || string.IsNullOrWhiteSpace(filePath)
            || !File.Exists(filePath))
        {
            return null;
        }

        try
        {
            report("正在按 QQ 优先次序补全标签、封面与歌词…");
            var matchOptions = options.Download.MatchOptions
                               ?? BatchLyricsOptions.AutoEnrich with
                               {
                                   FetchLyrics = options.Download.LyricsMode != LyricsWriteMode.None,
                               };
            var result = await _batchLyrics
                .FetchForFileAsync(filePath!, matchOptions, cancellationToken)
                .ConfigureAwait(false);

            if (result is null)
            {
                return null;
            }

            if (result.Success)
            {
                return DescribeMatch(result);
            }

            return result.Skipped ? null : $"在线匹配未完成：{result.Message}";
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            return $"在线匹配未完成：{ex.Message}";
        }
    }

    /// <summary>补全结果文案附带来源（如「（QQ 音乐）」），方便确认歌词来源。</summary>
    private static string? DescribeMatch(BatchLyricsResult result)
    {
        var message = MediaFileHelper.NullIfEmpty(result.Message);
        if (string.IsNullOrWhiteSpace(result.SourceLabel))
        {
            return message;
        }

        return string.IsNullOrWhiteSpace(message)
            ? $"已在线补全（{result.SourceLabel}）"
            : $"{message}（{result.SourceLabel}）";
    }

    private async Task<NeteaseSong?> TryFetchSongAsync(NeteaseSong song, CancellationToken cancellationToken)
    {
        try
        {
            var details = await _api.GetSongDetailsAsync(new[] { song.Id }, cancellationToken).ConfigureAwait(false);
            if (details is null || details.Count == 0)
            {
                return null;
            }

            return details.FirstOrDefault(d => d.Id == song.Id) ?? details[0];
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception)
        {
            return null;
        }
    }

    private async Task<(string? Plain, string? Word)> TryBuildLyricsAsync(
        long songId,
        LyricsWriteMode mode,
        CancellationToken cancellationToken)
    {
        if (mode == LyricsWriteMode.None)
        {
            return (null, null);
        }

        try
        {
            var raw = await _api.GetLyricsAsync(songId, cancellationToken).ConfigureAwait(false);
            if (raw is null)
            {
                return (null, null);
            }

            var document = _lyricParser.Parse(raw);
            if (document is null || document.IsEmpty)
            {
                return (null, null);
            }

            var plain = MediaFileHelper.NullIfEmpty(_lyricParser.ToPlainLrc(document));
            var word = mode == LyricsWriteMode.Word
                ? MediaFileHelper.NullIfEmpty(_lyricParser.ToEnhancedLrc(document))
                : null;
            return (plain, word);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception)
        {
            // 歌词失败不影响同步主体。
            return (null, null);
        }
    }

    private static SyncItemResult Failed(NeteaseSong song, string songName, string message) =>
        new(song.Id, songName, false, SyncSourceKind.Failed, null, message);

    private static string DisplayName(NeteaseSong song) =>
        string.IsNullOrWhiteSpace(song.DisplayName) ? song.Name : song.DisplayName;

    private static string StageOf(SyncItemResult result)
    {
        if (!result.Success)
        {
            return "失败";
        }

        return result.Source switch
        {
            SyncSourceKind.Downloaded => "下载完成",
            SyncSourceKind.DecryptedNcm => "NCM 解密完成",
            SyncSourceKind.Skipped => "已跳过",
            _ => "完成",
        };
    }
}
