using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Core.Tags;
using LunaticPrincess.Core.Utilities;

namespace LunaticPrincess.Core.Download;

/// <summary>
/// 批量下载器：取播放地址 → HttpClient 流式下载（带进度）→ NCM 魔数识别并解密 →
/// 拉取歌词/封面 → 通过 <see cref="IAudioTagService"/> 原子写入标签 → 按文件名模板落盘。
/// 单曲失败不会中断整批，返回结果保持输入顺序；支持并发上限与取消。
/// </summary>
public sealed class DownloadService : IDownloadService
{
    private readonly INeteaseApiClient _api;
    private readonly IAudioTagService _tagService;
    private readonly ILyricParser _lyricParser;
    private readonly INcmDecoder _ncmDecoder;
    private readonly HttpClient _httpClient;

    public DownloadService(
        INeteaseApiClient api,
        IAudioTagService tagService,
        ILyricParser lyricParser,
        INcmDecoder ncmDecoder,
        HttpClient httpClient)
    {
        _api = api ?? throw new ArgumentNullException(nameof(api));
        _tagService = tagService ?? throw new ArgumentNullException(nameof(tagService));
        _lyricParser = lyricParser ?? throw new ArgumentNullException(nameof(lyricParser));
        _ncmDecoder = ncmDecoder ?? throw new ArgumentNullException(nameof(ncmDecoder));
        _httpClient = httpClient ?? throw new ArgumentNullException(nameof(httpClient));
    }

    /// <inheritdoc />
    public async Task<DownloadSummary> DownloadAsync(
        IReadOnlyList<NeteaseSong> songs,
        DownloadOptions options,
        IProgress<DownloadProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(songs);
        ArgumentNullException.ThrowIfNull(options);
        if (string.IsNullOrWhiteSpace(options.TargetDirectory))
        {
            throw new ArgumentException("下载目录不能为空。", nameof(options));
        }

        var targetDirectory = Path.GetFullPath(options.TargetDirectory);
        Directory.CreateDirectory(targetDirectory);

        var total = songs.Count;
        if (total == 0)
        {
            return new DownloadSummary();
        }

        var results = new DownloadItemResult[total];
        var completed = 0;
        var maxConcurrency = Math.Max(1, options.MaxConcurrency);
        using var gate = new SemaphoreSlim(maxConcurrency, maxConcurrency);

        var tasks = new Task[total];
        for (var index = 0; index < total; index++)
        {
            tasks[index] = ProcessOneAsync(index);
        }

        await Task.WhenAll(tasks).ConfigureAwait(false);
        return new DownloadSummary { Results = results.Where(r => r is not null).ToArray() };

        async Task ProcessOneAsync(int index)
        {
            await gate.WaitAsync(cancellationToken).ConfigureAwait(false);
            DownloadItemResult? result = null;
            try
            {
                var song = songs[index];
                try
                {
                    result = await DownloadOneAsync(
                            song,
                            targetDirectory,
                            options,
                            total,
                            () => Volatile.Read(ref completed),
                            progress,
                            cancellationToken)
                        .ConfigureAwait(false);
                }
                catch (OperationCanceledException)
                {
                    throw;
                }
                catch (Exception ex)
                {
                    result = new DownloadItemResult(
                        song.Id,
                        DisplayName(song),
                        false,
                        ErrorMessage: ErrorText.DescribeWrite(ex, targetDirectory));
                }

                results[index] = result;
            }
            finally
            {
                gate.Release();
                if (result is not null)
                {
                    var done = Interlocked.Increment(ref completed);
                    progress?.Report(new DownloadProgress(
                        done,
                        total,
                        DisplayName(songs[index]),
                        StageOf(result),
                        result.ErrorMessage,
                        result.Success ? 100 : null));
                }
            }
        }
    }

    private async Task<DownloadItemResult> DownloadOneAsync(
        NeteaseSong song,
        string targetDirectory,
        DownloadOptions options,
        int total,
        Func<int> completedProvider,
        IProgress<DownloadProgress>? progress,
        CancellationToken cancellationToken)
    {
        var songName = DisplayName(song);
        void Report(string stage, string? message = null, double? percent = null) =>
            progress?.Report(new DownloadProgress(completedProvider(), total, songName, stage, message, percent));

        var year = MediaFileHelper.ParseYear(song.ReleaseDate);
        var baseName = MediaFileHelper.BuildBaseName(
            options,
            song.Name,
            song.ArtistText,
            song.Album.Name,
            song.TrackNumber,
            year,
            song.Id);

        // SkipExisting 优先于下载：不发起任何网络请求。
        if (options.SkipExisting && !options.OverwriteExisting)
        {
            var existing = MediaFileHelper.FindExistingAudioFile(targetDirectory, baseName);
            if (existing is not null)
            {
                Report("已存在，跳过", existing, 100);
                return new DownloadItemResult(song.Id, songName, true, existing, ErrorMessage: null, DecryptedFromNcm: false, Skipped: true);
            }
        }

        Report("获取播放地址", "正在获取下载地址…", 0);
        var playback = await _api.GetSongPlaybackInfoAsync(song.Id, options.Quality, cancellationToken)
            .ConfigureAwait(false);

        if (playback is null || !playback.IsSuccess || string.IsNullOrWhiteSpace(playback.Url))
        {
            var reason = MediaFileHelper.NullIfEmpty(playback?.ErrorMessage)
                         ?? "无法获取播放地址（可能受版权限制或需要登录/VIP）。";
            Report("获取播放地址失败", reason);
            return new DownloadItemResult(song.Id, songName, false, ErrorMessage: reason);
        }

        // 试听片段绝不作为完整下载保存。
        if (playback.IsTrial)
        {
            const string trialMessage = "该歌曲仅返回试听片段（需要 VIP 会员），已跳过下载。";
            Report("需要会员", trialMessage);
            return new DownloadItemResult(song.Id, songName, false, ErrorMessage: trialMessage);
        }

        // 高档位音质需要对应会员：服务端不支持时会静默降档，这里如实提示实际拿到的音质。
        if (playback.Level.FromApiLevel() is { } actualQuality && actualQuality != options.Quality)
        {
            Report(
                "获取播放地址",
                $"实际音质：{actualQuality.ToDisplayName()}（{options.Quality.ToDisplayName()} 不可用，已自动降级）",
                0);
        }

        var extension = MediaFileHelper.ResolveExtension(playback.Format, playback.Url);
        var downloadTemp = Path.Combine(targetDirectory, $".nmm-{Guid.NewGuid():N}.part");
        var ncmTemp = Path.ChangeExtension(downloadTemp, ".ncm");
        string? decryptedAudioPath = null;

        try
        {
            Report("下载中", "正在下载音频…", 0);
            var lastPercent = -1;
            var downloadedBytes = await MediaFileHelper.DownloadToFileAsync(
                    _httpClient,
                    playback.Url!,
                    downloadTemp,
                    (written, totalBytes) =>
                    {
                        if (totalBytes is not > 0)
                        {
                            return;
                        }

                        var percent = Math.Clamp(written * 100.0 / totalBytes.Value, 0, 100);
                        var rounded = (int)percent;
                        if (rounded != lastPercent)
                        {
                            lastPercent = rounded;
                            Report("下载中", "正在下载音频…", percent);
                        }
                    },
                    cancellationToken)
                .ConfigureAwait(false);

            if (downloadedBytes <= 0)
            {
                const string emptyMessage = "下载内容为空，已放弃该歌曲。";
                Report("下载失败", emptyMessage);
                return new DownloadItemResult(song.Id, songName, false, ErrorMessage: emptyMessage);
            }

            Report("下载完成", null, 100);

            string audioPath;
            var decryptedFromNcm = false;
            byte[]? ncmCover = null;

            if (await MediaFileHelper.HasNcmMagicAsync(downloadTemp, cancellationToken).ConfigureAwait(false))
            {
                Report("解密 NCM", "检测到 NCM 加密音频，正在解密…");
                File.Move(downloadTemp, ncmTemp, overwrite: true);

                var decryptResult = await _ncmDecoder
                    .DecryptAsync(ncmTemp, targetDirectory, overwrite: true, cancellationToken)
                    .ConfigureAwait(false);

                if (decryptResult is null
                    || !decryptResult.Success
                    || string.IsNullOrWhiteSpace(decryptResult.OutputPath)
                    || !File.Exists(decryptResult.OutputPath))
                {
                    var error = MediaFileHelper.NullIfEmpty(decryptResult?.ErrorMessage) ?? "NCM 解密失败。";
                    Report("解密失败", error);
                    return new DownloadItemResult(song.Id, songName, false, ErrorMessage: error);
                }

                audioPath = decryptResult.OutputPath;
                decryptedAudioPath = audioPath;
                decryptedFromNcm = true;
                ncmCover = decryptResult.CoverData;

                // 解密输出才是真实音频格式，优先于播放接口给出的 Format。
                extension = MediaFileHelper.ResolveContainerExtension(audioPath) ?? extension;
            }
            else
            {
                audioPath = downloadTemp;
            }

            var (plainLyrics, wordLyrics) = await TryBuildLyricsAsync(song.Id, options.LyricsMode, cancellationToken)
                .ConfigureAwait(false);

            byte[]? coverData = null;
            if (options.WriteCover)
            {
                Report("获取封面");
                coverData = ncmCover is { Length: > 0 }
                    ? ncmCover
                    : await MediaFileHelper.TryDownloadBytesAsync(_httpClient, song.CoverUrl, cancellationToken)
                        .ConfigureAwait(false);
            }

            var tags = MediaFileHelper.CreateTagData(song, coverData, plainLyrics, wordLyrics);

            Report("写入标签", "正在写入元数据/封面/歌词…");
            var (tagged, outputPath) = await MediaFileHelper.WriteTagsAtomicallyAsync(
                    _tagService,
                    audioPath,
                    targetDirectory,
                    baseName,
                    extension,
                    song.Id,
                    tags,
                    options.OverwriteExisting,
                    cancellationToken)
                .ConfigureAwait(false);

            decryptedAudioPath = null;

            var warning = tagged ? null : "标签写入失败，已保留音频文件。";
            Report("完成", warning, 100);
            return new DownloadItemResult(song.Id, songName, true, outputPath, warning, decryptedFromNcm);
        }
        finally
        {
            MediaFileHelper.TryDeleteFile(downloadTemp);
            MediaFileHelper.TryDeleteFile(ncmTemp);
            MediaFileHelper.TryDeleteFile(decryptedAudioPath);
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
            // 歌词属于可选增强，接口/解析失败不应导致音频下载失败。
            return (null, null);
        }
    }

    private static string DisplayName(NeteaseSong song) =>
        string.IsNullOrWhiteSpace(song.DisplayName) ? song.Name : song.DisplayName;

    private static string StageOf(DownloadItemResult result)
    {
        if (result.Skipped)
        {
            return "已存在，跳过";
        }

        if (!result.Success)
        {
            return "失败";
        }

        return result.DecryptedFromNcm ? "NCM 解密完成" : "完成";
    }
}
