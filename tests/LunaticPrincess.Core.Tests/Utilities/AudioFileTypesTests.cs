using LunaticPrincess.Core.Utilities;
using Xunit;

namespace LunaticPrincess.Core.Tests.Utilities;

public sealed class AudioFileTypesTests : IDisposable
{
    private readonly TempDirectory _temp = new("audio-file-types");

    public void Dispose() => _temp.Dispose();

    [Theory]
    [InlineData("song.mp3", true)]
    [InlineData("song.FLAC", true)]
    [InlineData("song.m4a", true)]
    [InlineData("song.opus", true)]
    [InlineData("song.wma", true)]
    [InlineData("song.ncm", false)]
    [InlineData("cover.jpg", false)]
    [InlineData("song", false)]
    [InlineData("", false)]
    [InlineData(null, false)]
    public void IsSupported_OnlyAcceptsEditableAudioExtensions(string? path, bool expected) =>
        Assert.Equal(expected, AudioFileTypes.IsSupported(path));

    [Fact]
    public void TryResolve_ReturnsAudioFileWhenDroppedDirectly()
    {
        var path = _temp.File("song.flac");
        File.WriteAllBytes(path, [0x66, 0x4C, 0x61, 0x43]);

        Assert.True(AudioFileTypes.TryResolve(path, out var resolved, out var error));
        Assert.Equal(path, resolved);
        Assert.Null(error);
    }

    [Fact]
    public void TryResolve_PicksFirstSupportedFileFromFolder()
    {
        var folder = _temp.Dir("album");
        File.WriteAllText(Path.Combine(folder, "cover.jpg"), "x");
        File.WriteAllText(Path.Combine(folder, "b - 第二首.mp3"), "x");
        File.WriteAllText(Path.Combine(folder, "a - 第一首.flac"), "x");

        Assert.True(AudioFileTypes.TryResolve(folder, out var resolved, out var error));
        Assert.Equal(Path.Combine(folder, "a - 第一首.flac"), resolved);
        Assert.Null(error);
    }

    [Fact]
    public void TryResolve_RejectsUnsupportedExtensionWithHint()
    {
        var path = _temp.File("song.ncm");
        File.WriteAllBytes(path, [0x43, 0x54, 0x45, 0x4E]);

        Assert.False(AudioFileTypes.TryResolve(path, out var resolved, out var error));
        Assert.Null(resolved);
        Assert.NotNull(error);
        Assert.Contains(".ncm", error);
        Assert.Contains("MP3", error);
    }

    [Fact]
    public void TryResolve_ReportsEmptyFolderAndMissingPath()
    {
        var empty = _temp.Dir("empty");
        Assert.False(AudioFileTypes.TryResolve(empty, out _, out var emptyError));
        Assert.Equal(AudioFileTypes.NoAudioInFolderMessage, emptyError);

        Assert.False(AudioFileTypes.TryResolve(_temp.File("missing.mp3"), out _, out var missingError));
        Assert.Contains("找不到", missingError);

        Assert.False(AudioFileTypes.TryResolve("  ", out _, out var blankError));
        Assert.Equal(AudioFileTypes.EmptyTargetMessage, blankError);
    }

    [Fact]
    public void ListSupportedFiles_SortsByNameAndSkipsOtherFormats()
    {
        var folder = _temp.Dir("mixed");
        File.WriteAllText(Path.Combine(folder, "z.mp3"), "x");
        File.WriteAllText(Path.Combine(folder, "a.wav"), "x");
        File.WriteAllText(Path.Combine(folder, "note.txt"), "x");

        var files = AudioFileTypes.ListSupportedFiles(folder);

        Assert.Equal(2, files.Count);
        Assert.EndsWith("a.wav", files[0], StringComparison.OrdinalIgnoreCase);
        Assert.EndsWith("z.mp3", files[1], StringComparison.OrdinalIgnoreCase);
        Assert.Empty(AudioFileTypes.ListSupportedFiles(null));
        Assert.Empty(AudioFileTypes.ListSupportedFiles(Path.Combine(folder, "nope")));
    }
}
