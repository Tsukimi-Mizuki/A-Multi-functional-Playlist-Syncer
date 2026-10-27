using LunaticPrincess.Core.Library;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Tags;
using Xunit;

namespace LunaticPrincess.Core.Tests.Library;

public sealed class LocalLibraryServiceTests
{
    private readonly FakeAudioTagServiceForLibrary _tags = new();
    private readonly FakeNcmDecoderForLibrary _ncm = new();

    private LocalLibraryService CreateService() => new(_ncm, _tags);

    [Fact]
    public async Task ScanAsync_FindsSupportedFilesRecursively_DedupesOverlappingRoots()
    {
        using var workspace = new TempWorkspaceForLibrary();
        workspace.CreateFile("a.mp3");
        workspace.CreateFile("album\\b.flac");
        workspace.CreateFile("album\\disc\\c.m4a");
        workspace.CreateFile("d.mp4");
        workspace.CreateFile("e.ogg");
        workspace.CreateFile("f.opus");
        workspace.CreateFile("g.wav");
        workspace.CreateFile("h.ncm");
        workspace.CreateFile("ignore.txt");
        var album = Path.Combine(workspace.Root, "album");

        var tracks = await CreateService().ScanAsync(new[] { workspace.Root, album, workspace.Root + Path.DirectorySeparatorChar });

        Assert.Equal(8, tracks.Count);
        Assert.Equal(tracks.Count, tracks.Select(t => t.Path).Distinct(StringComparer.OrdinalIgnoreCase).Count());
        Assert.All(tracks, t => Assert.True(File.Exists(t.Path)));
        Assert.DoesNotContain(tracks, t => t.Path.EndsWith(".txt", StringComparison.OrdinalIgnoreCase));
        Assert.Equal(new[] { "flac", "m4a", "mp3", "mp4", "ncm", "ogg", "opus", "wav" }, tracks.Select(t => t.Format!).OrderBy(f => f, StringComparer.Ordinal).ToArray());
        Assert.Single(tracks, t => t.IsNcm);
    }

    [Fact]
    public async Task ScanAsync_UsesTagMetadata_ForAudioFiles()
    {
        using var workspace = new TempWorkspaceForLibrary();
        var path = workspace.CreateFile("周杰伦 - 晴天.mp3");
        _tags.Entries[path] = new AudioTagInfo(
            Title: "晴天",
            Artist: "周杰伦",
            Album: "叶惠美",
            AlbumArtist: "周杰伦",
            DurationMs: 269_000,
            Bitrate: 320_000,
            Lyrics: "[00:01.00]故事的小黄花",
            HasCover: true,
            NeteaseId: 186016);

        var tracks = await CreateService().ScanAsync(new[] { workspace.Root });

        var track = Assert.Single(tracks);
        Assert.Equal(path, track.Path);
        Assert.Equal("晴天", track.Title);
        Assert.Equal("周杰伦", track.Artist);
        Assert.Equal("叶惠美", track.Album);
        Assert.Equal(269_000, track.DurationMs);
        Assert.Equal("mp3", track.Format);
        Assert.Equal(320_000, track.Bitrate);
        Assert.True(track.HasCover);
        Assert.False(track.IsNcm);
        Assert.Equal(186016, track.NeteaseId);
    }

    [Fact]
    public async Task ScanAsync_FallsBackToFileName_WhenTagsMissingOrUnreadable()
    {
        using var workspace = new TempWorkspaceForLibrary();
        workspace.CreateFile("周杰伦 - 晴天.flac");
        workspace.CreateFile("01. 普通歌曲.ogg");
        var broken = workspace.CreateFile("损坏文件.mp3");
        _tags.ReadFailures[broken] = new InvalidDataException("bad tag");

        var tracks = await CreateService().ScanAsync(new[] { workspace.Root });

        Assert.Equal(3, tracks.Count);
        var sunny = Assert.Single(tracks, t => t.Path.EndsWith("晴天.flac", StringComparison.Ordinal));
        Assert.Equal("晴天", sunny.Title);
        Assert.Equal("周杰伦", sunny.Artist);
        var plain = Assert.Single(tracks, t => t.Path.EndsWith("普通歌曲.ogg", StringComparison.Ordinal));
        Assert.Equal("普通歌曲", plain.Title);
        Assert.Null(plain.Artist);
        var brokenTrack = Assert.Single(tracks, t => t.Path == broken);
        Assert.Equal("损坏文件", brokenTrack.Title);
        Assert.Equal("mp3", brokenTrack.Format);
    }

    [Fact]
    public async Task ScanAsync_ReadsNcmHeaderMetadata_AndSurvivesBrokenNcm()
    {
        using var workspace = new TempWorkspaceForLibrary();
        var ncmPath = workspace.CreateFile("downloads\\Lucky.ncm");
        var brokenPath = workspace.CreateFile("downloads\\broken.ncm");
        _ncm.Infos[ncmPath] = new NcmFileInfo(ncmPath, new NcmMetadata(
            Title: "Lucky",
            Artists: "Jason Mraz/Colbie Caillat",
            Album: "We Sing. We Dance.",
            Format: "flac",
            Bitrate: 999_000,
            DurationMs: 245_000,
            MusicId: 123456));
        _ncm.ReadFailures[brokenPath] = new IOException("truncated header");

        var tracks = await CreateService().ScanAsync(new[] { workspace.Root });

        Assert.Equal(2, tracks.Count);
        var lucky = Assert.Single(tracks, t => t.Path == ncmPath);
        Assert.True(lucky.IsNcm);
        Assert.Equal("Lucky", lucky.Title);
        Assert.Equal("Jason Mraz/Colbie Caillat", lucky.Artist);
        Assert.Equal("We Sing. We Dance.", lucky.Album);
        Assert.Equal("flac", lucky.Format);
        Assert.Equal(999_000, lucky.Bitrate);
        Assert.Equal(245_000, lucky.DurationMs);
        Assert.Equal(123456, lucky.NeteaseId);
        Assert.NotNull(lucky.NcmMetadata);
        Assert.False(lucky.HasCover);

        var brokenTrack = Assert.Single(tracks, t => t.Path == brokenPath);
        Assert.True(brokenTrack.IsNcm);
        Assert.Equal("broken", brokenTrack.Title);
        Assert.Null(brokenTrack.NcmMetadata);
        Assert.Null(brokenTrack.NeteaseId);
    }

    [Fact]
    public async Task ScanAsync_IgnoresMissingDirectories_AndUnsupportedFiles()
    {
        using var workspace = new TempWorkspaceForLibrary();
        workspace.CreateFile("notes.txt");
        var missing = Path.Combine(workspace.Root, "not-there");

        var tracks = await CreateService().ScanAsync(new[] { missing, workspace.Root, " " });

        Assert.Empty(tracks);
    }

    [Fact]
    public async Task ScanAsync_ReportsMonotonicProgress()
    {
        using var workspace = new TempWorkspaceForLibrary();
        workspace.CreateFile("1.mp3");
        workspace.CreateFile("2.mp3");
        workspace.CreateFile("3.flac");
        var progress = new ImmediateProgressForLibrary<LibraryScanProgress>();

        var tracks = await CreateService().ScanAsync(new[] { workspace.Root }, progress);

        Assert.Equal(3, tracks.Count);
        var reports = progress.Snapshot();
        Assert.Equal(tracks.Count + 1, reports.Count);
        Assert.All(reports, r => Assert.Equal(3, r.TotalFiles));
        Assert.Equal(tracks.Count, reports[^1].ScannedFiles);
        Assert.NotEmpty(reports[^1].CurrentFile);
        for (var i = 1; i < reports.Count; i++)
        {
            Assert.True(reports[i].ScannedFiles >= reports[i - 1].ScannedFiles);
        }
    }

    [Fact]
    public async Task ScanAsync_HonoursCancellation()
    {
        using var workspace = new TempWorkspaceForLibrary();
        workspace.CreateFile("a.mp3");
        using var cts = new CancellationTokenSource();
        cts.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => CreateService().ScanAsync(new[] { workspace.Root }, cancellationToken: cts.Token));
    }

    [Fact]
    public async Task ScanAsync_SkipsDirectorySymlinks_ToAvoidLoops()
    {
        using var workspace = new TempWorkspaceForLibrary();
        var music = workspace.CreateDirectory("music");
        workspace.CreateFile("music\\a.mp3");
        var linkPath = Path.Combine(music, "loop");
        try
        {
            Directory.CreateSymbolicLink(linkPath, music);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or PlatformNotSupportedException)
        {
            // CI/无符号链接权限环境：跳过该用例即可，遍历逻辑本身另有 visited 集合兜底。
            return;
        }

        var tracks = await CreateService().ScanAsync(new[] { music });

        Assert.Single(tracks);
    }
}
