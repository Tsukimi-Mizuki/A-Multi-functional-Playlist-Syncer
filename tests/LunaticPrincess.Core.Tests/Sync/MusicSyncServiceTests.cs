using LunaticPrincess.Core.Download;
using LunaticPrincess.Core.Library;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Core.Sync;
using Xunit;

namespace LunaticPrincess.Core.Tests.Sync;

public sealed class MusicSyncServiceTests
{
    private static readonly byte[] NcmCover =
        { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x01 };

    private static byte[] PngBytes(byte marker) =>
        new byte[] { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, marker };

    private static NeteaseSong CreateSong(
        long id,
        string name = "晴天",
        string artist = "周杰伦",
        string album = "叶惠美",
        string? coverUrl = "https://img.example/sync-cover.jpg",
        string releaseDate = "2003-07-31",
        int trackNumber = 2) =>
        new()
        {
            Id = id,
            Name = name,
            Artists = new[] { new ArtistRef(1, artist) },
            Album = new AlbumRef(2, album, coverUrl),
            DurationMs = 269_000,
            ReleaseDate = releaseDate,
            TrackNumber = trackNumber,
        };

    private static LocalTrack CreateLocal(string path, bool isNcm) => new()
    {
        Path = path,
        Title = "本地标题",
        Artist = "本地歌手",
        DurationMs = 240_000,
        Format = isNcm ? "flac" : "mp3",
        IsNcm = isNcm,
    };

    private static DownloadOptions CreateDownloadOptions(
        string directory,
        LyricsWriteMode lyricsMode = LyricsWriteMode.Line,
        bool writeCover = true) => new()
    {
        TargetDirectory = directory,
        LyricsMode = lyricsMode,
        WriteCover = writeCover,
    };

    private sealed class Harness : IDisposable
    {
        public FakeApiForSync Api { get; } = new();
        public FakeNcmDecoderForSync Decoder { get; } = new();
        public RecordingTagServiceForSync Tags { get; } = new();
        public FakeLyricParserForSync Parser { get; } = new();
        public FakeImporterForSync Importer { get; } = new();
        public FakeDownloaderForSync Downloader { get; } = new();
        public FakeBatchLyricsForSync Batch { get; } = new();
        public Dictionary<string, byte[]> HttpResponses { get; } = new();
        public HttpClient Client { get; private set; } = null!;
        public StubHttpMessageHandlerForSync HttpHandler { get; private set; } = null!;

        public Harness()
        {
            RebuildHttpClient();
        }

        public void RebuildHttpClient()
        {
            Client = StubHttpMessageHandlerForSync.CreateClient(HttpResponses, out var handler);
            HttpHandler = handler;
        }

        public MusicSyncService CreateService() =>
            new(Api, Decoder, Tags, Parser, Importer, Downloader, Client);

        public MusicSyncService CreateEnrichedService() =>
            new(Api, Decoder, Tags, Parser, Importer, Downloader, Client, Batch);

        public void Dispose() => Client.Dispose();
    }

    private static void AssertNoTempFiles(string directory)
    {
        foreach (var file in Directory.EnumerateFiles(directory, "*", SearchOption.AllDirectories))
        {
            var name = Path.GetFileName(file);
            Assert.DoesNotContain(".tmp", name, StringComparison.OrdinalIgnoreCase);
            Assert.False(name.StartsWith(".", StringComparison.Ordinal), $"遗留临时文件: {file}");
        }
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_DecryptsAndWritesApiMetadataAndLyrics()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var ncmPath = workspace.WriteFile("Lucky.ncm", new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D, 1 });
        harness.Importer.CoverData = NcmCover;
        harness.Api.SongDetailsHandler = (_, _) =>
            Task.FromResult<IReadOnlyList<NeteaseSong>>(new[] { CreateSong(7) });
        harness.Api.LyricsHandler = (_, _) => Task.FromResult(new RawLyrics("[00:00.00]x"));
        var progress = new ImmediateProgressForSync<SyncProgress>();
        var request = new SyncRequest
        {
            Song = CreateSong(7, name: "请求歌名", artist: "请求歌手", album: "请求专辑"),
            LocalMatch = CreateLocal(ncmPath, isNcm: true),
        };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) },
            progress);

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Equal(SyncSourceKind.DecryptedNcm, result.Source);
        Assert.Equal(1, summary.DecryptedCount);
        Assert.Equal(0, summary.DownloadedCount);
        Assert.Equal(Path.Combine(workspace.Root, "周杰伦 - 晴天.flac"), result.OutputPath);
        Assert.True(File.Exists(result.OutputPath));
        Assert.Empty(harness.Downloader.Calls);
        Assert.Empty(harness.Decoder.DecryptCalls);
        var importCall = Assert.Single(harness.Importer.Calls);
        Assert.True(importCall.Overwrite);

        var write = Assert.Single(harness.Tags.Writes);
        Assert.Equal("晴天", write.Tags.Title);
        Assert.Equal(new[] { "周杰伦" }, write.Tags.Artists);
        Assert.Equal(NcmCover, write.Tags.CoverData);
        Assert.Equal("image/png", write.Tags.CoverMimeType);
        Assert.Equal(harness.Parser.PlainText, write.Tags.PlainLyrics);
        Assert.Null(write.Tags.WordLyrics);
        Assert.Equal(7L, write.Tags.NeteaseId);
        Assert.Empty(harness.HttpHandler.SnapshotRequests());
        AssertNoTempFiles(workspace.Root);

        var reports = progress.Snapshot();
        Assert.Equal(1, reports[^1].Completed);
        Assert.Equal(1, reports[^1].Total);
        Assert.Contains(reports, r => r.Stage == "解密 NCM");
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_AutoMatchesQqWordLyrics()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var ncmPath = workspace.WriteFile("Lucky.ncm", new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D });
        harness.Api.SongDetailsHandler = (_, _) =>
            Task.FromResult<IReadOnlyList<NeteaseSong>>(new[] { CreateSong(7) });
        harness.Api.LyricsHandler = (_, _) => Task.FromResult(new RawLyrics("[00:00.00]x"));
        var request = new SyncRequest { Song = CreateSong(7), LocalMatch = CreateLocal(ncmPath, isNcm: true) };

        var summary = await harness.CreateEnrichedService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Equal(SyncSourceKind.DecryptedNcm, result.Source);
        var fetched = Assert.Single(harness.Batch.FetchCalls);
        Assert.Equal(result.OutputPath, fetched);
        var options = Assert.Single(harness.Batch.FetchOptions);
        Assert.Equal(OnlineSourcePriority.QQMusicFirst, options.TagPriority);
        Assert.Equal(OnlineSourcePriority.QQMusicFirst, options.LyricsPriority);
        Assert.True(options.PreferWordLyrics);
        Assert.Contains("已写入标签", result.Message);
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_AutoMatchDisabled_KeepsOnlineLyricsOnly()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var ncmPath = workspace.WriteFile("Lucky.ncm", new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D });
        harness.Api.SongDetailsHandler = (_, _) =>
            Task.FromResult<IReadOnlyList<NeteaseSong>>(new[] { CreateSong(7) });
        harness.Api.LyricsHandler = (_, _) => Task.FromResult(new RawLyrics("[00:00.00]x"));
        var request = new SyncRequest { Song = CreateSong(7), LocalMatch = CreateLocal(ncmPath, isNcm: true) };
        var downloadOptions = CreateDownloadOptions(workspace.Root);

        var summary = await harness.CreateEnrichedService().SyncAsync(
            new[] { request },
            new SyncOptions
            {
                Download = new DownloadOptions
                {
                    TargetDirectory = downloadOptions.TargetDirectory,
                    LyricsMode = downloadOptions.LyricsMode,
                    WriteCover = downloadOptions.WriteCover,
                    AutoMatchOnline = false,
                },
            });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Empty(harness.Batch.FetchCalls);
        Assert.Null(result.Message);
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_WordLyricsMode_WritesEnhancedLyrics()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var ncmPath = workspace.WriteFile("Lucky.ncm", new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D });
        harness.Api.SongDetailsHandler = (_, _) =>
            Task.FromResult<IReadOnlyList<NeteaseSong>>(new[] { CreateSong(7) });
        harness.Api.LyricsHandler = (_, _) => Task.FromResult(new RawLyrics("[00:00.00]x"));
        var request = new SyncRequest { Song = CreateSong(7), LocalMatch = CreateLocal(ncmPath, isNcm: true) };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions
            {
                Download = CreateDownloadOptions(workspace.Root, LyricsWriteMode.Word),
            });

        Assert.Equal(1, summary.DecryptedCount);
        var write = Assert.Single(harness.Tags.Writes);
        Assert.Equal(harness.Parser.PlainText, write.Tags.PlainLyrics);
        Assert.Equal(harness.Parser.EnhancedText, write.Tags.WordLyrics);
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_ImportFailure_ReturnsFailed()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var ncmPath = workspace.WriteFile("Lucky.ncm", new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D });
        harness.Importer.Handler = (paths, _, _, _) => Task.FromResult<IReadOnlyList<NcmDecryptResult>>(
            new[] { new NcmDecryptResult(paths[0], null, false, ErrorMessage: "NCM 解密失败") });
        var request = new SyncRequest { Song = CreateSong(7), LocalMatch = CreateLocal(ncmPath, isNcm: true) };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.False(result.Success);
        Assert.Equal(SyncSourceKind.Failed, result.Source);
        Assert.Contains("解密失败", result.Message);
        Assert.Empty(harness.Downloader.Calls);
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_MissingFile_ReturnsFailed_WithoutDownloader()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var request = new SyncRequest
        {
            Song = CreateSong(7),
            LocalMatch = CreateLocal(Path.Combine(workspace.Root, "gone.ncm"), isNcm: true),
        };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.False(result.Success);
        Assert.Equal(SyncSourceKind.Failed, result.Source);
        Assert.Empty(harness.Importer.Calls);
        Assert.Empty(harness.Downloader.Calls);
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_FallsBackToRequestMetadata_WhenApiAndHeadersFail()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var ncmPath = workspace.WriteFile("Lucky.ncm", new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D });
        harness.Decoder.ReadInfoFailure = new IOException("损坏的头");
        harness.Api.SongDetailsHandler = (_, _) =>
            Task.FromException<IReadOnlyList<NeteaseSong>>(new NeteaseApiException("网络不可用"));
        var request = new SyncRequest { Song = CreateSong(7), LocalMatch = CreateLocal(ncmPath, isNcm: true) };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Equal(SyncSourceKind.DecryptedNcm, result.Source);
        Assert.Equal(Path.Combine(workspace.Root, "周杰伦 - 晴天.flac"), result.OutputPath);
        var write = Assert.Single(harness.Tags.Writes);
        Assert.Equal("晴天", write.Tags.Title);
        Assert.Equal(new[] { "周杰伦" }, write.Tags.Artists);
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_FallsBackToHttpCover_WhenImportHasNoCover()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var ncmPath = workspace.WriteFile("Lucky.ncm", new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D });
        harness.Importer.CoverData = null;
        var httpCover = PngBytes(4);
        harness.HttpResponses["https://img.example/sync-cover.jpg"] = httpCover;
        harness.RebuildHttpClient();
        harness.Api.SongDetailsHandler = (_, _) =>
            Task.FromResult<IReadOnlyList<NeteaseSong>>(new[] { CreateSong(7) });
        var request = new SyncRequest { Song = CreateSong(7), LocalMatch = CreateLocal(ncmPath, isNcm: true) };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        Assert.Equal(1, summary.DecryptedCount);
        var write = Assert.Single(harness.Tags.Writes);
        Assert.Equal(httpCover, write.Tags.CoverData);
        Assert.Contains("https://img.example/sync-cover.jpg", harness.HttpHandler.SnapshotRequests());
    }

    [Fact]
    public async Task SyncAsync_ExistingRegularFile_IsSkipped_WhenNotRedownloading()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var existing = workspace.WriteFile("existing.mp3", new byte[] { 1, 2, 3 });
        var request = new SyncRequest
        {
            Song = CreateSong(1),
            LocalMatch = CreateLocal(existing, isNcm: false),
        };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Equal(SyncSourceKind.Skipped, result.Source);
        Assert.Equal(existing, result.OutputPath);
        Assert.Equal(1, summary.SkippedCount);
        Assert.Empty(harness.Downloader.Calls);
    }

    [Fact]
    public async Task SyncAsync_ExistingRegularFile_Redownloads_WhenRequested()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var existing = workspace.WriteFile("existing.mp3", new byte[] { 1, 2, 3 });
        var request = new SyncRequest
        {
            Song = CreateSong(1),
            LocalMatch = CreateLocal(existing, isNcm: false),
        };
        var options = new SyncOptions
        {
            Download = CreateDownloadOptions(workspace.Root),
            RedownloadExisting = true,
        };

        var summary = await harness.CreateService().SyncAsync(new[] { request }, options);

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Equal(SyncSourceKind.Downloaded, result.Source);
        var call = Assert.Single(harness.Downloader.Calls);
        Assert.Same(request.Song, call.Songs[0]);
        Assert.Same(options.Download, call.Options);
    }

    [Fact]
    public async Task SyncAsync_MissingLocal_DownloadsViaDownloader()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var request = new SyncRequest { Song = CreateSong(1) };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Equal(SyncSourceKind.Downloaded, result.Source);
        Assert.Equal(Path.Combine(workspace.Root, "晴天.mp3"), result.OutputPath);
        Assert.Equal(1, summary.DownloadedCount);
    }

    [Fact]
    public async Task SyncAsync_DownloaderFailure_ReturnsFailed()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        harness.Downloader.Handler = (songs, _, _) => Task.FromResult(new DownloadSummary
        {
            Results = new[]
            {
                new DownloadItemResult(songs[0].Id, songs[0].Name, false, ErrorMessage: "无版权"),
            },
        });
        var request = new SyncRequest { Song = CreateSong(1) };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.False(result.Success);
        Assert.Equal(SyncSourceKind.Failed, result.Source);
        Assert.Equal("无版权", result.Message);
        Assert.Equal(1, summary.FailedCount);
    }

    [Fact]
    public async Task SyncAsync_DownloaderSkip_MapsToSkippedSource()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        harness.Downloader.Handler = (songs, options, _) => Task.FromResult(new DownloadSummary
        {
            Results = new[]
            {
                new DownloadItemResult(
                    songs[0].Id,
                    songs[0].Name,
                    true,
                    Path.Combine(options.TargetDirectory, "skip.mp3"),
                    Skipped: true),
            },
        });
        var request = new SyncRequest { Song = CreateSong(1) };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Equal(SyncSourceKind.Skipped, result.Source);
    }

    [Fact]
    public async Task SyncAsync_DuplicateRequests_ProduceOneResultPerRequest_InOrder()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var song = CreateSong(1);
        var requests = new[]
        {
            new SyncRequest { Song = song },
            new SyncRequest { Song = song },
        };

        var summary = await harness.CreateService().SyncAsync(
            requests,
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        Assert.Equal(2, summary.Results.Count);
        Assert.All(summary.Results, r =>
        {
            Assert.Equal(song.Id, r.SongId);
            Assert.True(r.Success);
            Assert.Equal(SyncSourceKind.Downloaded, r.Source);
        });
        Assert.Equal(2, harness.Downloader.Calls.Count);
        Assert.Equal(2, summary.SuccessCount);
    }

    [Fact]
    public async Task SyncAsync_AggregatesProgress_AcrossBranches()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var existing = workspace.WriteFile("existing.mp3", new byte[] { 1 });
        var requests = new[]
        {
            new SyncRequest { Song = CreateSong(1), LocalMatch = CreateLocal(existing, isNcm: false) },
            new SyncRequest { Song = CreateSong(2) },
        };
        var progress = new ImmediateProgressForSync<SyncProgress>();

        var summary = await harness.CreateService().SyncAsync(
            requests,
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) },
            progress);

        Assert.Equal(2, summary.SuccessCount);
        Assert.Equal(1, summary.SkippedCount);
        Assert.Equal(1, summary.DownloadedCount);
        var reports = progress.Snapshot();
        Assert.Equal(2, reports[^1].Completed);
        Assert.Equal(2, reports[^1].Total);
        Assert.Contains(reports, r => r.Stage == "已跳过");
        Assert.Contains(reports, r => r.Stage == "下载完成");
    }

    [Fact]
    public async Task SyncAsync_LocalNcm_TagWriteFailure_StillReturnsDecryptedSuccess()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        var ncmPath = workspace.WriteFile("Lucky.ncm", new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D });
        harness.Tags.ThrowOnWrite = true;
        var request = new SyncRequest { Song = CreateSong(7), LocalMatch = CreateLocal(ncmPath, isNcm: true) };

        var summary = await harness.CreateService().SyncAsync(
            new[] { request },
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Equal(SyncSourceKind.DecryptedNcm, result.Source);
        Assert.Contains("标签写入失败", result.Message);
        Assert.True(File.Exists(result.OutputPath));
        AssertNoTempFiles(workspace.Root);
    }

    [Fact]
    public async Task SyncAsync_HonoursCancellation()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => harness.CreateService().SyncAsync(
                new[] { new SyncRequest { Song = CreateSong(1) } },
                new SyncOptions { Download = CreateDownloadOptions(workspace.Root) },
                cancellationToken: cts.Token));
    }

    [Fact]
    public async Task SyncAsync_EmptyRequests_ReturnsEmptySummary()
    {
        using var workspace = new TempWorkspaceForSync();
        using var harness = new Harness();

        var summary = await harness.CreateService().SyncAsync(
            Array.Empty<SyncRequest>(),
            new SyncOptions { Download = CreateDownloadOptions(workspace.Root) });

        Assert.Empty(summary.Results);
    }
}
