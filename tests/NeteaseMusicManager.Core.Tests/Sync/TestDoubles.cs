using System.Net;
using System.Net.Http;
using NeteaseMusicManager.Core.Download;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Ncm;
using NeteaseMusicManager.Core.Netease;
using NeteaseMusicManager.Core.Tags;

namespace NeteaseMusicManager.Core.Tests.Sync;

internal sealed class TempWorkspaceForSync : IDisposable
{
    public TempWorkspaceForSync()
    {
        Root = Path.Combine(Path.GetTempPath(), "nmm-sync-tests", Guid.NewGuid().ToString("N"));
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

internal sealed class ImmediateProgressForSync<T> : IProgress<T>
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

internal sealed class StubHttpMessageHandlerForSync : HttpMessageHandler
{
    private readonly Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> _handler;
    private readonly object _gate = new();
    private readonly List<string> _requests = new();

    public StubHttpMessageHandlerForSync(
        Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> handler) => _handler = handler;

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
        out StubHttpMessageHandlerForSync handler)
    {
        handler = new StubHttpMessageHandlerForSync((request, _) =>
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

internal sealed class FakeApiForSync : INeteaseApiClient
{
    public Func<IReadOnlyCollection<long>, CancellationToken, Task<IReadOnlyList<NeteaseSong>>>? SongDetailsHandler { get; set; }
    public Func<long, CancellationToken, Task<RawLyrics>>? LyricsHandler { get; set; }
    public int SongDetailsCalls;
    public int LyricsCalls;

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

    public Task<IReadOnlyList<NeteaseSong>> SearchSongsAsync(
        string keyword,
        int limit = 20,
        CancellationToken cancellationToken = default) => throw new NotSupportedException();

    public Task<IReadOnlyList<NeteaseSong>> GetSongDetailsAsync(
        IReadOnlyCollection<long> songIds,
        CancellationToken cancellationToken = default)
    {
        Interlocked.Increment(ref SongDetailsCalls);
        return SongDetailsHandler?.Invoke(songIds, cancellationToken)
               ?? Task.FromResult<IReadOnlyList<NeteaseSong>>(Array.Empty<NeteaseSong>());
    }

    public Task<SongPlaybackInfo> GetSongPlaybackInfoAsync(
        long songId,
        AudioQuality quality,
        CancellationToken cancellationToken = default) => throw new NotSupportedException();

    public Task<RawLyrics> GetLyricsAsync(long songId, CancellationToken cancellationToken = default)
    {
        Interlocked.Increment(ref LyricsCalls);
        return LyricsHandler?.Invoke(songId, cancellationToken) ?? Task.FromResult(new RawLyrics(null));
    }

    public Task<AlbumInfo?> GetAlbumAsync(long albumId, CancellationToken cancellationToken = default) =>
        throw new NotSupportedException();
}

internal sealed class RecordingTagServiceForSync : IAudioTagService
{
    private readonly object _gate = new();

    public List<(string Path, AudioTagData Tags)> Writes { get; } = new();
    public bool ThrowOnWrite { get; set; }

    public Task<AudioTagInfo?> ReadAsync(string audioPath, CancellationToken cancellationToken = default) =>
        Task.FromResult<AudioTagInfo?>(null);

    public Task WriteAsync(string audioPath, AudioTagData tags, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate)
        {
            Writes.Add((audioPath, tags));
        }

        return ThrowOnWrite
            ? Task.FromException(new IOException("标签写入失败"))
            : Task.CompletedTask;
    }
}

internal sealed class FakeLyricParserForSync : NeteaseMusicManager.Core.Lyrics.ILyricParser
{
    public string PlainText { get; set; } = "[00:00.00]line1";
    public string EnhancedText { get; set; } = "[00:00.00]<00:00.00>line1";

    public NeteaseMusicManager.Core.Lyrics.LyricDocument Parse(RawLyrics rawLyrics) =>
        new()
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

    public string ToPlainLrc(NeteaseMusicManager.Core.Lyrics.LyricDocument document) => PlainText;

    public string ToEnhancedLrc(NeteaseMusicManager.Core.Lyrics.LyricDocument document) => EnhancedText;
}

internal sealed class FakeNcmDecoderForSync : INcmDecoder
{
    public NcmMetadata Metadata { get; set; } = new("NCM 歌名", "NCM 歌手", "NCM 专辑", "flac", 999_000, 240_000, 7);
    public Exception? ReadInfoFailure { get; set; }
    public List<string> ReadInfoCalls { get; } = new();
    public List<string> DecryptCalls { get; } = new();

    public bool IsNcmFile(string path) => true;

    public Task<NcmFileInfo> ReadInfoAsync(string ncmPath, CancellationToken cancellationToken = default)
    {
        lock (ReadInfoCalls)
        {
            ReadInfoCalls.Add(ncmPath);
        }

        if (ReadInfoFailure is not null)
        {
            return Task.FromException<NcmFileInfo>(ReadInfoFailure);
        }

        return Task.FromResult(new NcmFileInfo(ncmPath, Metadata));
    }

    public Task<NcmDecryptResult> DecryptAsync(
        string ncmPath,
        string outputDirectory,
        bool overwrite = false,
        CancellationToken cancellationToken = default)
    {
        lock (DecryptCalls)
        {
            DecryptCalls.Add(ncmPath);
        }

        return Task.FromException<NcmDecryptResult>(
            new InvalidOperationException("同步的 NCM 分支应通过 INcmImportService 解密。"));
    }
}

internal sealed class FakeImporterForSync : INcmImportService
{
    public List<(string[] Paths, string OutputDirectory, bool Overwrite)> Calls { get; } = new();
    public NcmMetadata Metadata { get; set; } = new("NCM 歌名", "NCM 歌手", "NCM 专辑", "flac", 999_000, 240_000, 7);
    public byte[]? CoverData { get; set; } = new byte[] { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x01 };
    public string OutputFormat { get; set; } = "flac";

    public Func<IReadOnlyList<string>, string, bool, CancellationToken, Task<IReadOnlyList<NcmDecryptResult>>>? Handler { get; set; }

    public async Task<IReadOnlyList<NcmDecryptResult>> ImportAsync(
        IReadOnlyList<string> ncmPaths,
        string outputDirectory,
        bool overwrite = false,
        IProgress<NcmImportProgress>? progress = null,
        CancellationToken cancellationToken = default,
        NcmImportOptions? options = null)
    {
        lock (Calls)
        {
            Calls.Add((ncmPaths.ToArray(), outputDirectory, overwrite));
        }

        if (Handler is not null)
        {
            return await Handler(ncmPaths, outputDirectory, overwrite, cancellationToken).ConfigureAwait(false);
        }

        var results = new List<NcmDecryptResult>(ncmPaths.Count);
        foreach (var path in ncmPaths)
        {
            var outputPath = Path.Combine(
                outputDirectory,
                Path.GetFileNameWithoutExtension(path) + "." + OutputFormat);
            await File.WriteAllBytesAsync(outputPath, new byte[] { 0x66, 0x4C, 0x61, 0x43 }, cancellationToken)
                .ConfigureAwait(false);
            results.Add(new NcmDecryptResult(path, outputPath, true, OutputFormat, Metadata, CoverData));
        }

        return results;
    }
}

internal sealed class FakeDownloaderForSync : IDownloadService
{
    public List<(NeteaseSong[] Songs, DownloadOptions Options)> Calls { get; } = new();

    public Func<IReadOnlyList<NeteaseSong>, DownloadOptions, CancellationToken, Task<DownloadSummary>>? Handler { get; set; }

    public Task<DownloadSummary> DownloadAsync(
        IReadOnlyList<NeteaseSong> songs,
        DownloadOptions options,
        IProgress<DownloadProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        lock (Calls)
        {
            Calls.Add((songs.ToArray(), options));
        }

        if (Handler is not null)
        {
            return Handler(songs, options, cancellationToken);
        }

        var results = songs
            .Select(song => new DownloadItemResult(
                song.Id,
                song.Name,
                true,
                Path.Combine(options.TargetDirectory, song.Name + ".mp3")))
            .ToArray();
        return Task.FromResult(new DownloadSummary { Results = results });
    }
}
