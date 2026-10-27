using LunaticPrincess.Core.Tags;
using Xunit;
using TagLibFile = TagLib.File;

namespace LunaticPrincess.Core.Tests.Tags;

public sealed class AudioTagServiceTests : IDisposable
{
    private readonly TempDirectory _temp = new("audio-tags");
    private readonly AudioTagService _service = new();

    public void Dispose() => _temp.Dispose();

    [Fact]
    public async Task WriteAsync_RoundTripsAllFields()
    {
        var path = CreateMp3("歌曲 - 测试歌手.mp3");
        var cover = Mp3Fixture.PngCover();
        const string plainLyrics = "[00:01.00]普通歌词";
        const string wordLyrics = "[00:01.00]<00:01.00>逐字<00:01.50>歌词";

        var tags = new AudioTagData
        {
            Title = "晴天",
            Artists = new[] { "周杰伦", "测试歌手" },
            Album = "叶惠美",
            AlbumArtist = "周杰伦",
            TrackNumber = 3,
            DiscNumber = 1,
            Year = 2003,
            CoverData = cover,
            CoverMimeType = "image/png",
            PlainLyrics = plainLyrics,
            WordLyrics = wordLyrics,
            NeteaseId = 186016,
            Comment = "原始备注",
        };

        await _service.WriteAsync(path, tags);

        var info = await _service.ReadAsync(path);
        Assert.NotNull(info);
        Assert.Equal("晴天", info!.Title);
        Assert.Equal("周杰伦/测试歌手", info.Artist);
        Assert.Equal("叶惠美", info.Album);
        Assert.Equal("周杰伦", info.AlbumArtist);
        Assert.Equal(wordLyrics, info.Lyrics);
        Assert.True(info.HasCover);
        Assert.Equal(186016L, info.NeteaseId);
        Assert.True(info.DurationMs > 0);
        Assert.True(info.Bitrate > 0);

        using var file = TagLibFile.Create(path);
        var id3 = Assert.IsType<TagLib.Id3v2.Tag>(file.GetTag(TagLib.TagTypes.Id3v2, true));
        Assert.Equal(3u, id3.Track);
        Assert.Equal(1u, id3.Disc);
        Assert.Equal(2003u, id3.Year);
        Assert.Equal("晴天", id3.Title);
        Assert.Single(id3.Pictures);
        Assert.Equal("image/png", id3.Pictures[0].MimeType);
        Assert.Equal(TagLib.PictureType.FrontCover, id3.Pictures[0].Type);
        Assert.Equal(cover, id3.Pictures[0].Data.Data);
        Assert.Contains("LunaticPrincess:186016", id3.Comment);
        Assert.Contains("原始备注", id3.Comment);
        Assert.True((file.TagTypesOnDisk & TagLib.TagTypes.Id3v2) != 0, "应写入 ID3v2 标签。");
    }

    [Fact]
    public async Task WriteAsync_PrefersWordLyricsAndFallsBackToPlainLyrics()
    {
        var path = CreateMp3("lyrics.mp3");

        await _service.WriteAsync(path, new AudioTagData { WordLyrics = "word-a", PlainLyrics = "plain-a" });
        Assert.Equal("word-a", (await _service.ReadAsync(path))!.Lyrics);

        await _service.WriteAsync(path, new AudioTagData { WordLyrics = "  ", PlainLyrics = "plain-b" });
        Assert.Equal("plain-b", (await _service.ReadAsync(path))!.Lyrics);

        await _service.WriteAsync(path, new AudioTagData { PlainLyrics = "plain-c" });
        Assert.Equal("plain-c", (await _service.ReadAsync(path))!.Lyrics);
    }

    [Fact]
    public async Task WriteAsync_PreservesExistingCommentAndUsesMarkerWithoutDuplicates()
    {
        var path = CreateMp3("comment.mp3");

        await _service.WriteAsync(path, new AudioTagData { Comment = "原始备注", NeteaseId = 42 });
        var first = await _service.ReadAsync(path);
        Assert.Equal(42L, first!.NeteaseId);

        await _service.WriteAsync(path, new AudioTagData { Title = "新标题", NeteaseId = 43 });
        var second = await _service.ReadAsync(path);
        Assert.Equal("新标题", second!.Title);
        Assert.Equal(43L, second.NeteaseId);

        using (var file = TagLibFile.Create(path))
        {
            var comment = file.Tag.Comment ?? string.Empty;
            Assert.Contains("原始备注", comment);
            Assert.Contains("LunaticPrincess:43", comment);
            Assert.DoesNotContain("LunaticPrincess:42", comment);
        }

        // 下载管线会用 "LunaticPrincess" 作为 Comment 基底，不应产生重复标记词。
        await _service.WriteAsync(path, new AudioTagData { Comment = "LunaticPrincess", NeteaseId = 44 });
        using var file2 = TagLibFile.Create(path);
        Assert.Equal("LunaticPrincess:44", file2.Tag.Comment);
        Assert.Equal(44L, (await _service.ReadAsync(path))!.NeteaseId);
    }

    [Fact]
    public async Task WriteAsync_KeepsExistingCoverLyricsAndNeteaseIdWhenNotProvided()
    {
        var path = CreateMp3("preserve.mp3");
        var cover = Mp3Fixture.PngCover();

        await _service.WriteAsync(path, new AudioTagData
        {
            Title = "第一版",
            CoverData = cover,
            CoverMimeType = "image/png",
            WordLyrics = "word-lyrics",
            NeteaseId = 1001,
        });

        await _service.WriteAsync(path, new AudioTagData { Title = "第二版" });

        var info = await _service.ReadAsync(path);
        Assert.NotNull(info);
        Assert.Equal("第二版", info!.Title);
        Assert.Equal("word-lyrics", info.Lyrics);
        Assert.True(info.HasCover);
        Assert.Equal(1001L, info.NeteaseId);
    }

    [Fact]
    public async Task WriteAsync_ReplacesCoverWhenNewCoverProvided()
    {
        var path = CreateMp3("cover.mp3");
        var first = Mp3Fixture.PngCover();
        var second = Mp3Fixture.PngCover().Concat(new byte[] { 0x42, 0x42, 0x42 }).ToArray();

        await _service.WriteAsync(path, new AudioTagData { CoverData = first, CoverMimeType = "image/png" });
        await _service.WriteAsync(path, new AudioTagData { CoverData = second, CoverMimeType = "image/png" });

        using var file = TagLibFile.Create(path);
        var pictures = file.Tag.Pictures;
        Assert.Single(pictures);
        Assert.Equal(second, pictures[0].Data.Data);
    }

    [Fact]
    public async Task WriteAsync_NullTags_Throws()
    {
        var path = CreateMp3("null.mp3");

        await Assert.ThrowsAsync<ArgumentNullException>(
            () => _service.WriteAsync(path, null!));
        await Assert.ThrowsAsync<ArgumentException>(
            () => _service.WriteAsync(" ", new AudioTagData()));
        await Assert.ThrowsAsync<ArgumentException>(
            () => _service.ReadAsync(" "));
    }

    [Fact]
    public async Task WriteAsync_MissingOrUnsupportedFile_ThrowsMeaningfulException()
    {
        await Assert.ThrowsAsync<FileNotFoundException>(
            () => _service.WriteAsync(_temp.File("missing.mp3"), new AudioTagData()));

        var unsupported = _temp.File("notes.txt");
        await File.WriteAllTextAsync(unsupported, "not audio");
        var exception = await Assert.ThrowsAsync<InvalidOperationException>(
            () => _service.WriteAsync(unsupported, new AudioTagData { Title = "x" }));
        Assert.False(string.IsNullOrWhiteSpace(exception.Message));
    }

    [Fact]
    public async Task ReadAsync_ReturnsNullForMissingCorruptAndUnsupportedFiles()
    {
        Assert.Null(await _service.ReadAsync(_temp.File("missing.mp3")));

        var corrupt = _temp.File("corrupt.mp3");
        await File.WriteAllBytesAsync(corrupt, new byte[] { 0x00, 0x01, 0x02 });
        Assert.Null(await _service.ReadAsync(corrupt));

        var unsupported = _temp.File("lyrics.txt");
        await File.WriteAllTextAsync(unsupported, "just text");
        Assert.Null(await _service.ReadAsync(unsupported));
    }

    [Fact]
    public async Task ReadAsync_UntaggedMp3_ReturnsPropertiesWithoutTags()
    {
        var path = CreateMp3("plain.mp3");

        var info = await _service.ReadAsync(path);

        Assert.NotNull(info);
        Assert.Null(info!.Title);
        Assert.Null(info.Artist);
        Assert.Null(info.Album);
        Assert.Null(info.NeteaseId);
        Assert.False(info.HasCover);
        Assert.True(info.DurationMs > 0);
        Assert.True(info.Bitrate > 0);
    }

    [Fact]
    public async Task WriteAsync_RoundTripsExtendedTagsAndReplaceAllCanClearFields()
    {
        var path = CreateMp3("extended-tags.mp3");

        await _service.WriteAsync(path, new AudioTagData
        {
            ReplaceAll = true,
            Title = "T",
            Artists = new[] { "A1", "A2" },
            Album = "AL",
            AlbumArtist = "AA",
            Year = 2024,
            TrackNumber = 2,
            DiscNumber = 1,
            Genre = "Pop/Rock",
            Composer = "C1/C2",
            Comment = "note",
            PlainLyrics = "[00:01.00]line",
        });

        var info = await _service.ReadAsync(path);
        Assert.NotNull(info);
        Assert.Equal("T", info!.Title);
        Assert.Equal("A1/A2", info.Artist);
        Assert.Equal("AL", info.Album);
        Assert.Equal("AA", info.AlbumArtist);
        Assert.Equal(2024u, info.Year);
        Assert.Equal(2u, info.TrackNumber);
        Assert.Equal(1u, info.DiscNumber);
        Assert.Equal("Pop/Rock", info.Genre);
        Assert.Equal("C1/C2", info.Composer);
        Assert.Equal("note", info.Comment);
        Assert.Equal("[00:01.00]line", info.Lyrics);
        Assert.True(info.FileSize > 0);
        Assert.Equal("MP3", info.FileFormat);

        await _service.WriteAsync(path, new AudioTagData
        {
            ReplaceAll = true,
            Title = "Only title",
            PlainLyrics = "new lyrics",
        });

        var cleared = await _service.ReadAsync(path);
        Assert.NotNull(cleared);
        Assert.Equal("Only title", cleared!.Title);
        Assert.Null(cleared.Artist);
        Assert.Null(cleared.Album);
        Assert.Null(cleared.AlbumArtist);
        Assert.Null(cleared.Year);
        Assert.Null(cleared.TrackNumber);
        Assert.Null(cleared.DiscNumber);
        Assert.Null(cleared.Genre);
        Assert.Null(cleared.Composer);
        Assert.Null(cleared.Comment);
        Assert.Equal("new lyrics", cleared.Lyrics);
    }

    private string CreateMp3(string fileName)
    {
        var path = _temp.File(fileName);
        File.WriteAllBytes(path, Mp3Fixture.BuildMinimalMp3());
        return path;
    }
}
