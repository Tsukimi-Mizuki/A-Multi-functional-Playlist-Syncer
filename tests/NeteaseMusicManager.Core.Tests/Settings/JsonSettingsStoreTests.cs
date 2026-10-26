using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Settings;
using Xunit;

namespace NeteaseMusicManager.Core.Tests.Settings;

public sealed class JsonSettingsStoreTests : IDisposable
{
    private readonly string _directory = Path.Combine(Path.GetTempPath(), "nmm-tests-" + Guid.NewGuid().ToString("N"));
    private string SettingsPath => Path.Combine(_directory, "settings.json");

    public JsonSettingsStoreTests() => Directory.CreateDirectory(_directory);

    [Fact]
    public async Task MissingFile_ReturnsNormalizedDefaults()
    {
        var store = new JsonSettingsStore(SettingsPath);
        var settings = await store.LoadAsync();
        Assert.False(string.IsNullOrWhiteSpace(settings.DownloadDirectory));
        Assert.InRange(settings.MaxConcurrency, 1, 8);
        Assert.Equal("{artist} - {title}", settings.FileNameTemplate);
        Assert.Empty(settings.NcmImportFolders);
        Assert.Equal(string.Empty, settings.NcmImportOutputDirectory);
    }

    [Fact]
    public async Task SaveLoad_NormalizesAndPreservesNcmImportMemory()
    {
        var store = new JsonSettingsStore(SettingsPath);
        var settings = new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            NcmImportFolders = new List<string> { @"  D:\ncm-a  ", @"D:\ncm-b", @"d:\NCM-A", string.Empty, null! },
            NcmImportOutputDirectory = @" D:\ncm-out ",
        };

        await store.SaveAsync(settings);
        var loaded = await store.LoadAsync();

        Assert.Equal(new[] { @"D:\ncm-a", @"D:\ncm-b" }, loaded.NcmImportFolders);
        Assert.Equal(@"D:\ncm-out", loaded.NcmImportOutputDirectory);
    }

    [Fact]
    public async Task SaveLoad_RoundTripsEnumsAndPaths()
    {
        var store = new JsonSettingsStore(SettingsPath);
        var settings = new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            LibraryDirectory = Path.Combine(_directory, "library"),
            Quality = AudioQuality.Lossless,
            LyricsMode = LyricsWriteMode.Word,
            WriteCover = false,
            OverwriteExisting = true,
            MaxConcurrency = 99,
            FileNameTemplate = "{track} {title}",
        };

        await store.SaveAsync(settings);
        var loaded = await store.LoadAsync();

        Assert.Equal(settings.DownloadDirectory, loaded.DownloadDirectory);
        Assert.Equal(settings.LibraryDirectory, loaded.LibraryDirectory);
        Assert.Equal(AudioQuality.Lossless, loaded.Quality);
        Assert.Equal(LyricsWriteMode.Word, loaded.LyricsMode);
        Assert.False(loaded.WriteCover);
        Assert.True(loaded.OverwriteExisting);
        Assert.Equal(8, loaded.MaxConcurrency);
        Assert.Equal("{track} {title}", loaded.FileNameTemplate);
    }

    [Fact]
    public async Task CorruptFile_ReturnsDefaults()
    {
        await File.WriteAllTextAsync(SettingsPath, "{oops");
        var store = new JsonSettingsStore(SettingsPath);
        var settings = await store.LoadAsync();
        Assert.NotNull(settings.DownloadDirectory);
    }

    public void Dispose()
    {
        try { Directory.Delete(_directory, recursive: true); } catch { }
    }
}
