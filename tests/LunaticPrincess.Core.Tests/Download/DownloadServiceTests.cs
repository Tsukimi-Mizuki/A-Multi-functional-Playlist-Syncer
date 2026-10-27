using System.Net;
using System.Net.Http;
using System.Text;
using LunaticPrincess.Core.Download;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Download;

public sealed class DownloadServiceTests
{
    private static NeteaseSong CreateSong(
        long id,
        string name = "晴天",
        string artist = "周杰伦",
        string album = "叶惠美",
        string? coverUrl = "https://img.example/cover.jpg",
        int durationMs = 269_000,
        string releaseDate = "2003-07-31",
        int trackNumber = 3) =>
        new()
        {
            Id = id,
            Name = name,
            Artists = new[] { new ArtistRef(1, artist) },
            Album = new AlbumRef(2, album, coverUrl),
            DurationMs = durationMs,
            ReleaseDate = releaseDate,
            TrackNumber = trackNumber,
        };

    private static byte[] Mp3Bytes() => new byte[] { 0x49, 0x44, 0x33, 0x04, 0x00, 0x00, 0xFF, 0xFB, 0x01 };

    private static byte[] FlacBytes() => new byte[] { 0x66, 0x4C, 0x61, 0x43, 0x00, 0x00, 0x01 };

    private static byte[] PngBytes(byte marker) =>
        new byte[] { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, marker };

    private static byte[] NcmBytes() => Encoding.ASCII.GetBytes("CTENFDAM").Concat(new byte[] { 1, 2, 3, 4 }).ToArray();

    private static DownloadService CreateService(
        FakeApiForDownload api,
        RecordingTagServiceForDownload tags,
        FakeLyricParserForDownload parser,
        FakeNcmDecoderForDownload decoder,
        HttpClient client) => new(api, tags, parser, decoder, client);

    private static void AssertNoTempFiles(string directory)
    {
        foreach (var file in Directory.EnumerateFiles(directory, "*", SearchOption.AllDirectories))
        {
            var name = Path.GetFileName(file);
            Assert.False(name.EndsWith(".part", StringComparison.OrdinalIgnoreCase), $"遗留临时文件: {file}");
            Assert.DoesNotContain(".tmp", name, StringComparison.OrdinalIgnoreCase);
            Assert.DoesNotContain(".nmm-", name, StringComparison.OrdinalIgnoreCase);
            Assert.False(name.StartsWith(".", StringComparison.Ordinal), $"遗留临时文件: {file}");
        }
    }

    [Fact]
    public async Task DownloadAsync_DownloadsAndWritesMetadataCoverAndLineLyrics()
    {
        using var workspace = new TempWorkspaceForDownload();
        var media = Mp3Bytes();
        var cover = PngBytes(7);
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
                Bitrate = 320_000,
            }),
            LyricsHandler = (_, _) => Task.FromResult(new RawLyrics("[00:01.00]line")),
        };
        var tags = new RecordingTagServiceForDownload();
        var parser = new FakeLyricParserForDownload();
        var decoder = new FakeNcmDecoderForDownload();
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]>
            {
                ["https://cdn.example/1.mp3"] = media,
                ["https://img.example/cover.jpg"] = cover,
            },
            out var handler);
        var progress = new ImmediateProgressForDownload<DownloadProgress>();
        var service = CreateService(api, tags, parser, decoder, client);

        var summary = await service.DownloadAsync(
            new[] { CreateSong(1) },
            new DownloadOptions { TargetDirectory = workspace.Root },
            progress);

        Assert.Equal(1, summary.SuccessCount);
        Assert.Equal(0, summary.FailedCount);
        Assert.Equal(0, summary.SkippedCount);
        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.False(result.Skipped);
        Assert.False(result.DecryptedFromNcm);
        Assert.Equal(Path.Combine(workspace.Root, "周杰伦 - 晴天.mp3"), result.OutputPath);
        Assert.True(File.Exists(result.OutputPath));
        Assert.Equal(media, await File.ReadAllBytesAsync(result.OutputPath!));
        Assert.Equal(2, handler.SnapshotRequests().Count);

        var write = Assert.Single(tags.Writes);
        Assert.True(tags.FileExistedAtWrite[0]);
        Assert.Equal("晴天", write.Tags.Title);
        Assert.Equal(new[] { "周杰伦" }, write.Tags.Artists);
        Assert.Equal("叶惠美", write.Tags.Album);
        Assert.Equal("周杰伦", write.Tags.AlbumArtist);
        Assert.Equal(3u, write.Tags.TrackNumber);
        Assert.Equal(2003u, write.Tags.Year);
        Assert.Equal(1L, write.Tags.NeteaseId);
        Assert.Equal(cover, write.Tags.CoverData);
        Assert.Equal("image/png", write.Tags.CoverMimeType);
        Assert.Equal(parser.PlainText, write.Tags.PlainLyrics);
        Assert.Null(write.Tags.WordLyrics);
        Assert.Equal(1, parser.ParseCalls);
        Assert.Equal(0, parser.EnhancedCalls);

        var reports = progress.Snapshot();
        Assert.Contains(reports, r => r.Stage == "下载中");
        Assert.Contains(reports, r => r.Stage == "写入标签");
        Assert.Equal(1, reports[^1].Completed);
        Assert.Equal(1, reports[^1].Total);
        AssertNoTempFiles(workspace.Root);
    }

    [Fact]
    public async Task DownloadAsync_WordLyricsMode_WritesEnhancedLyrics()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
            LyricsHandler = (_, _) => Task.FromResult(new RawLyrics("[00:01.00]line")),
        };
        var tags = new RecordingTagServiceForDownload();
        var parser = new FakeLyricParserForDownload();
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]> { ["https://cdn.example/2.mp3"] = Mp3Bytes() },
            out _);

        var summary = await CreateService(api, tags, parser, new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                new[] { CreateSong(2, coverUrl: null) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    LyricsMode = LyricsWriteMode.Word,
                    WriteCover = false,
                });

        Assert.Equal(1, summary.SuccessCount);
        var write = Assert.Single(tags.Writes);
        Assert.Equal(parser.PlainText, write.Tags.PlainLyrics);
        Assert.Equal(parser.EnhancedText, write.Tags.WordLyrics);
        Assert.Equal(1, parser.EnhancedCalls);
    }

    [Fact]
    public async Task DownloadAsync_SkipsExistingFile_WithoutNetworkRequests()
    {
        using var workspace = new TempWorkspaceForDownload();
        var existing = workspace.WriteFile("周杰伦 - 晴天.mp3", new byte[] { 9, 9, 9 });
        var api = new FakeApiForDownload();
        var tags = new RecordingTagServiceForDownload();
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]>(), out var handler);

        var summary = await CreateService(api, tags, new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(new[] { CreateSong(1) }, new DownloadOptions { TargetDirectory = workspace.Root });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.True(result.Skipped);
        Assert.Equal(existing, result.OutputPath);
        Assert.Equal(1, summary.SkippedCount);
        Assert.Equal(0, summary.SuccessCount);
        Assert.Equal(0, api.PlaybackCalls);
        Assert.Empty(handler.SnapshotRequests());
        Assert.Empty(tags.Writes);
    }

    [Fact]
    public async Task DownloadAsync_OverwriteExisting_ReplacesFile()
    {
        using var workspace = new TempWorkspaceForDownload();
        var existing = workspace.WriteFile("周杰伦 - 晴天.mp3", new byte[] { 9, 9, 9 });
        var media = Mp3Bytes();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
        };
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]> { ["https://cdn.example/1.mp3"] = media },
            out _);

        var summary = await CreateService(api, new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                new[] { CreateSong(1, coverUrl: null) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    OverwriteExisting = true,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.None,
                });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.False(result.Skipped);
        Assert.Equal(existing, result.OutputPath);
        Assert.Equal(media, await File.ReadAllBytesAsync(existing));
        Assert.Equal(1, api.PlaybackCalls);
    }

    [Fact]
    public async Task DownloadAsync_PlaybackFailure_ReturnsFailedItem_AndContinuesBatch()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(id == 1
                ? new SongPlaybackInfo { SongId = id, Url = null, ErrorMessage = "版权受限" }
                : new SongPlaybackInfo
                {
                    SongId = id,
                    Url = $"https://cdn.example/{id}.mp3",
                    Format = "mp3",
                }),
        };
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]> { ["https://cdn.example/2.mp3"] = Mp3Bytes() },
            out _);
        var songs = new[] { CreateSong(1, name: "受限歌曲", coverUrl: null), CreateSong(2, name: "可下载歌曲", coverUrl: null) };

        var summary = await CreateService(api, new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                songs,
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    MaxConcurrency = 1,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.None,
                });

        Assert.Equal(2, summary.Results.Count);
        Assert.False(summary.Results[0].Success);
        Assert.Equal("版权受限", summary.Results[0].ErrorMessage);
        Assert.True(summary.Results[1].Success);
        Assert.Equal(1, summary.FailedCount);
        Assert.Equal(1, summary.SuccessCount);
        Assert.True(File.Exists(summary.Results[1].OutputPath));
        AssertNoTempFiles(workspace.Root);
    }

    [Fact]
    public async Task DownloadAsync_TrialPlayback_IsRejected_NeverSavedAsFullSong()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = "https://cdn.example/trial.mp3",
                Format = "mp3",
                IsTrial = true,
                TrialStartSeconds = 0,
                TrialEndSeconds = 45,
            }),
        };
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]>(), out var handler);

        var summary = await CreateService(api, new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(new[] { CreateSong(3) }, new DownloadOptions { TargetDirectory = workspace.Root });

        var result = Assert.Single(summary.Results);
        Assert.False(result.Success);
        Assert.Contains("试听", result.ErrorMessage);
        Assert.Empty(Directory.GetFiles(workspace.Root));
        Assert.Empty(handler.SnapshotRequests());
    }

    [Fact]
    public async Task DownloadAsync_NcmPayload_Decrypts_AndPrefersNcmCover()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
        };
        var decoder = new FakeNcmDecoderForDownload
        {
            CoverData = PngBytes(3),
            OutputFormat = "flac",
        };
        var httpCover = PngBytes(9);
        var tags = new RecordingTagServiceForDownload();
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]>
            {
                ["https://cdn.example/4.mp3"] = NcmBytes(),
                ["https://img.example/cover.jpg"] = httpCover,
            },
            out _);

        var summary = await CreateService(api, tags, new FakeLyricParserForDownload(), decoder, client)
            .DownloadAsync(
                new[] { CreateSong(4) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    WriteCover = true,
                    LyricsMode = LyricsWriteMode.None,
                });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.True(result.DecryptedFromNcm);
        Assert.EndsWith(".flac", result.OutputPath, StringComparison.OrdinalIgnoreCase);
        Assert.True(File.Exists(result.OutputPath));
        var decryptCall = Assert.Single(decoder.DecryptCalls);
        Assert.EndsWith(".ncm", decryptCall, StringComparison.OrdinalIgnoreCase);
        Assert.True(decoder.DecryptOverwriteFlags[0]);
        var write = Assert.Single(tags.Writes);
        Assert.Equal(PngBytes(3), write.Tags.CoverData);
        Assert.Equal("image/png", write.Tags.CoverMimeType);
        Assert.Equal("晴天", write.Tags.Title);
        AssertNoTempFiles(workspace.Root);
    }

    [Fact]
    public async Task DownloadAsync_LyricsFailure_DoesNotFailDownload()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
            LyricsHandler = (_, _) => Task.FromException<RawLyrics>(new NeteaseApiException("歌词接口失败")),
        };
        var tags = new RecordingTagServiceForDownload();
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]> { ["https://cdn.example/5.mp3"] = Mp3Bytes() },
            out _);

        var summary = await CreateService(api, tags, new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                new[] { CreateSong(5, coverUrl: null) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.Line,
                });

        Assert.Equal(1, summary.SuccessCount);
        var write = Assert.Single(tags.Writes);
        Assert.Null(write.Tags.PlainLyrics);
        Assert.Null(write.Tags.WordLyrics);
    }

    [Fact]
    public async Task DownloadAsync_FilenameCollisions_GetSongIdSuffix()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
        };
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]>
            {
                ["https://cdn.example/10.mp3"] = Mp3Bytes(),
                ["https://cdn.example/11.mp3"] = Mp3Bytes(),
            },
            out _);

        var summary = await CreateService(api, new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                new[] { CreateSong(10), CreateSong(11) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    MaxConcurrency = 1,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.None,
                    SkipExisting = false,
                });

        Assert.Equal(2, summary.SuccessCount);
        var fileNames = Directory.GetFiles(workspace.Root).Select(Path.GetFileName).OrderBy(n => n, StringComparer.Ordinal).ToArray();
        Assert.Equal(new[] { "周杰伦 - 晴天 (11).mp3", "周杰伦 - 晴天.mp3" }, fileNames);
    }

    [Fact]
    public async Task DownloadAsync_RespectsMaxConcurrency()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
        };
        var media = Mp3Bytes();
        var gate = new object();
        var inFlight = 0;
        var peak = 0;
        var bothStarted = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        using var client = new HttpClient(new StubHttpMessageHandlerForDownload(async (request, cancellationToken) =>
        {
            var count = Interlocked.Increment(ref inFlight);
            lock (gate)
            {
                peak = Math.Max(peak, count);
            }

            if (count >= 2)
            {
                bothStarted.TrySetResult();
            }

            try
            {
                await bothStarted.Task.WaitAsync(TimeSpan.FromSeconds(10), cancellationToken);
            }
            finally
            {
                Interlocked.Decrement(ref inFlight);
            }

            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent(media) };
        }));

        var songs = Enumerable.Range(1, 4)
            .Select(i => CreateSong(i, name: $"歌曲{i}", coverUrl: null))
            .ToArray();
        var summary = await CreateService(api, new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                songs,
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    MaxConcurrency = 2,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.None,
                });

        Assert.Equal(4, summary.SuccessCount);
        Assert.Equal(2, peak);
    }

    [Fact]
    public async Task DownloadAsync_TagWriteFailure_StillKeepsAudioFile()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
        };
        var tags = new RecordingTagServiceForDownload { ThrowOnWrite = true };
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]> { ["https://cdn.example/6.mp3"] = Mp3Bytes() },
            out _);

        var summary = await CreateService(api, tags, new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                new[] { CreateSong(6, coverUrl: null) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.None,
                });

        var result = Assert.Single(summary.Results);
        Assert.True(result.Success);
        Assert.Contains("标签写入失败", result.ErrorMessage);
        Assert.True(File.Exists(result.OutputPath));
        AssertNoTempFiles(workspace.Root);
    }

    [Fact]
    public async Task DownloadAsync_HttpFailure_ReturnsFailedItem_WithoutTempFiles()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
        };
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]>(), out _);

        var summary = await CreateService(api, new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                new[] { CreateSong(7, coverUrl: null) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.None,
                });

        var result = Assert.Single(summary.Results);
        Assert.False(result.Success);
        Assert.Contains("404", result.ErrorMessage);
        Assert.Empty(Directory.GetFiles(workspace.Root));
    }

    [Fact]
    public async Task DownloadAsync_Cancellation_LeavesNoTempFiles()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = $"https://cdn.example/{id}.mp3",
                Format = "mp3",
            }),
        };
        var requestStarted = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        using var client = new HttpClient(new StubHttpMessageHandlerForDownload(async (request, cancellationToken) =>
        {
            requestStarted.TrySetResult();
            await Task.Delay(TimeSpan.FromSeconds(30), cancellationToken);
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent(Mp3Bytes()) };
        }));
        using var cts = new CancellationTokenSource();

        var run = CreateService(api, new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                new[] { CreateSong(8, coverUrl: null) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.None,
                },
                cancellationToken: cts.Token);

        await requestStarted.Task.WaitAsync(TimeSpan.FromSeconds(10));
        cts.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => run);
        AssertNoTempFiles(workspace.Root);
    }

    [Fact]
    public async Task DownloadAsync_ResolvesExtensionFromUrl_WhenFormatMissing()
    {
        using var workspace = new TempWorkspaceForDownload();
        var api = new FakeApiForDownload
        {
            PlaybackHandler = (id, _, _) => Task.FromResult(new SongPlaybackInfo
            {
                SongId = id,
                Url = "https://cdn.example/file.flac?token=abc",
                Format = null,
            }),
        };
        using var client = StubHttpMessageHandlerForDownload.CreateClient(
            new Dictionary<string, byte[]> { ["https://cdn.example/file.flac?token=abc"] = FlacBytes() },
            out _);

        var summary = await CreateService(api, new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(
                new[] { CreateSong(9, coverUrl: null) },
                new DownloadOptions
                {
                    TargetDirectory = workspace.Root,
                    WriteCover = false,
                    LyricsMode = LyricsWriteMode.None,
                });

        var result = Assert.Single(summary.Results);
        Assert.EndsWith(".flac", result.OutputPath, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public async Task DownloadAsync_EmptySongList_ReturnsEmptySummary()
    {
        using var workspace = new TempWorkspaceForDownload();
        using var client = StubHttpMessageHandlerForDownload.CreateClient(new Dictionary<string, byte[]>(), out _);

        var summary = await CreateService(new FakeApiForDownload(), new RecordingTagServiceForDownload(), new FakeLyricParserForDownload(), new FakeNcmDecoderForDownload(), client)
            .DownloadAsync(Array.Empty<NeteaseSong>(), new DownloadOptions { TargetDirectory = workspace.Root });

        Assert.Empty(summary.Results);
    }
}
