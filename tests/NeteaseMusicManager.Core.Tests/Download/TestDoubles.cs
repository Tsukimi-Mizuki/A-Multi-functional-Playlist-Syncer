using System.Net;
using System.Net.Http;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Ncm;
using NeteaseMusicManager.Core.Netease;
using NeteaseMusicManager.Core.Tags;

namespace NeteaseMusicManager.Core.Tests.Download;

internal sealed class TempWorkspaceForDownload : IDisposable
{
    public TempWorkspaceForDownload()
    {
        Root = Path.Combine(Path.GetTempPath(), "nmm-dl-tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Root);
    }

    public string Root { get; }

    public string WriteFile(string relativePath, byte[] content)
    {
        var fullPath = Path.Combine(Root, relativePath);
        var directory = Path.GetDirectoryName(fullPath);
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }

        File.WriteAllBytes(fullPath, content);
        return fullPath;
    }

    public void Dispose()
    {
        try
        {
            if (Directory.Exists(Root))
            {
                Directory.Delete(Root, recursive: true);
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
        }
    }
}

internal sealed class ImmediateProgressForDownload<T> : IProgress<T>
{
    private readonly object _gate = new();
    private readonly List<T> _reports = new();

    public IReadOnlyList<T> Snapshot()
    {
        lock (_gate)
        {
            return _reports.ToArray();
        }
    }

    public void Report(T value)
    {
        lock (_gate)
        {
            _reports.Add(value);
        }
    }
}

internal sealed class StubHttpMessageHandlerForDownload : HttpMessageHandler
{
    private readonly Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> _handler;
    private readonly object _gate = new();
    private readonly List<string> _requests = new();

    public StubHttpMessageHandlerForDownload(
        Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> handler)
    {
        _handler = handler;
    }

    public IReadOnlyList<string> SnapshotRequests()
    {
        lock (_gate)
        {
            return _requests.ToArray();
        }
    }

    protected override async Task<HttpResponseMessage> SendAsync(
        HttpRequestMessage request,
        CancellationToken cancellationToken)
    {
        lock (_gate)
        {
            _requests.Add(request.RequestUri?.ToString() ?? string.Empty);
        }

        return await _handler(request, cancellationToken).ConfigureAwait(false);
    }

    public static HttpClient CreateClient(
        IReadOnlyDictionary<string, byte[]> responses,
        out StubHttpMessageHandlerForDownload handler)
    {
        handler = new StubHttpMessageHandlerForDownload((request, _) =>
        {
            var url = request.RequestUri?.ToString() ?? string.Empty;
            if (responses.TryGetValue(url, out var bytes))
            {
                return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK)
                {
                    Content = new ByteArrayContent(bytes),
                });
            }

            return Task.FromResult(new HttpResponseMessage(HttpStatusCode.NotFound));
        });
        return new HttpClient(handler);
    }
}

internal sealed class FakeApiForDownload : INeteaseApiClient
{
    public int PlaybackCalls;
    public int LyricsCalls;

    public Func<long, AudioQuality, CancellationToken, Task<SongPlaybackInfo>>? PlaybackHandler { get; set; }
    public Func<long, CancellationToken, Task<RawLyrics>>? LyricsHandler { get; set; }

    public NeteaseSession? Session => null;
    public bool IsLoggedIn => false;

    public void SetSession(NeteaseSession? session) => throw new NotSupportedException();

    public Task<QrLoginStart> CreateQrLoginAsync(CancellationToken cancellationToken = default) =>
        throw new NotSupportedException();

    public Task<QrLoginStatus> PollQrLoginAsync(string key, CancellationToken cancellationToken = default) =>
        throw new NotSupportedException();

    public Task<NeteaseUser?> GetCurrentUserAsync(CancellationToken cancellationToken = default) =>
        throw new NotSupportedException();

    public Task<IReadOnlyList<PlaylistSummary>> GetUserPlaylistsAsync(
        long userId,
        CancellationToken cancellationToken = default) => throw new NotSupportedException();

    public Task<PlaylistDetail> GetPlaylistDetailAsync(long playlistId, CancellationToken cancellationToken = default) =>
        throw new NotSupportedException();

    public Task<IReadOnlyList<NeteaseSong>> GetSongDetailsAsync(
        IReadOnlyCollection<long> songIds,
        CancellationToken cancellationToken = default) => throw new NotSupportedException();

    public Task<SongPlaybackInfo> GetSongPlaybackInfoAsync(
        long songId,
        AudioQuality quality,
        CancellationToken cancellationToken = default)
    {
        Interlocked.Increment(ref PlaybackCalls);
        return PlaybackHandler?.Invoke(songId, quality, cancellationToken)
               ?? throw new NotSupportedException("本测试未配置播放地址。");
    }

    public Task<RawLyrics> GetLyricsAsync(long songId, CancellationToken cancellationToken = default)
    {
        Interlocked.Increment(ref LyricsCalls);
        return LyricsHandler?.Invoke(songId, cancellationToken)
               ?? Task.FromResult(new RawLyrics(null));
    }

    public Task<AlbumInfo?> GetAlbumAsync(long albumId, CancellationToken cancellationToken = default) =>
        throw new NotSupportedException();
}

internal sealed class RecordingTagServiceForDownload : IAudioTagService
{
    private readonly object _gate = new();

    public List<(string Path, AudioTagData Tags)> Writes { get; } = new();
    public List<bool> FileExistedAtWrite { get; } = new();
    public bool ThrowOnWrite { get; set; }
    public string ThrowMessage { get; set; } = "标签写入失败";

    public Task<AudioTagInfo?> ReadAsync(string audioPath, CancellationToken cancellationToken = default) =>
        Task.FromResult<AudioTagInfo?>(null);

    public Task WriteAsync(string audioPath, AudioTagData tags, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            Writes.Add((audioPath, tags));
            FileExistedAtWrite.Add(File.Exists(audioPath));
        }

        if (ThrowOnWrite)
        {
            return Task.FromException(new IOException(ThrowMessage));
        }

        return Task.CompletedTask;
    }
}

internal sealed class FakeLyricParserForDownload : NeteaseMusicManager.Core.Lyrics.ILyricParser
{
    public int ParseCalls;
    public int PlainCalls;
    public int EnhancedCalls;
    public string PlainText { get; set; } = "[00:00.00]line1";
    public string EnhancedText { get; set; } = "[00:00.00]<00:00.00>line1";

    public NeteaseMusicManager.Core.Lyrics.LyricDocument Parse(RawLyrics rawLyrics)
    {
        Interlocked.Increment(ref ParseCalls);
        return new NeteaseMusicManager.Core.Lyrics.LyricDocument
        {
            Lines = new[]
            {
                new NeteaseMusicManager.Core.Lyrics.LyricLine(
                    0,
                    1000,
                    "line1",
                    new[] { new NeteaseMusicManager.Core.Lyrics.LyricWord(0, 500, "line1") }),
            },
        };
    }

    public string ToPlainLrc(NeteaseMusicManager.Core.Lyrics.LyricDocument document)
    {
        Interlocked.Increment(ref PlainCalls);
        return PlainText;
    }

    public string ToEnhancedLrc(NeteaseMusicManager.Core.Lyrics.LyricDocument document)
    {
        Interlocked.Increment(ref EnhancedCalls);
        return EnhancedText;
    }
}

internal sealed class FakeNcmDecoderForDownload : INcmDecoder
{
    public NcmMetadata Metadata { get; set; } = new(
        "NCM 内嵌歌名",
        "NCM 内嵌歌手",
        "NCM 内嵌专辑",
        "mp3",
        128_000,
        200_000,
        555);

    public byte[]? CoverData { get; set; }
    public string OutputFormat { get; set; } = "flac";
    public bool IsNcmFileResult { get; set; } = true;
    public List<string> DecryptCalls { get; } = new();
    public List<bool> DecryptOverwriteFlags { get; } = new();

    public Func<string, string, bool, CancellationToken, Task<NcmDecryptResult>>? DecryptHandler { get; set; }

    public bool IsNcmFile(string path) => IsNcmFileResult;

    public Task<NcmFileInfo> ReadInfoAsync(string ncmPath, CancellationToken cancellationToken = default) =>
        Task.FromResult(new NcmFileInfo(ncmPath, Metadata));

    public async Task<NcmDecryptResult> DecryptAsync(
        string ncmPath,
        string outputDirectory,
        bool overwrite = false,
        CancellationToken cancellationToken = default)
    {
        lock (DecryptCalls)
        {
            DecryptCalls.Add(ncmPath);
            DecryptOverwriteFlags.Add(overwrite);
        }

        if (DecryptHandler is not null)
        {
            return await DecryptHandler(ncmPath, outputDirectory, overwrite, cancellationToken).ConfigureAwait(false);
        }

        var outputPath = Path.Combine(
            outputDirectory,
            Path.GetFileNameWithoutExtension(ncmPath) + "." + OutputFormat);
        await File.WriteAllBytesAsync(outputPath, new byte[] { 0x66, 0x4C, 0x61, 0x43 }, cancellationToken)
            .ConfigureAwait(false);
        return new NcmDecryptResult(ncmPath, outputPath, true, OutputFormat, Metadata, CoverData);
    }
}
