using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Settings;
using Xunit;

namespace LunaticPrincess.Core.Tests.Settings;

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
    public async Task SaveLoad_PreservesLyricsEditorOptionMemory()
    {
        var store = new JsonSettingsStore(SettingsPath);
        var settings = new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            LyricsEditorSearchSource = LyricsSourceKind.QQMusic,
            LyricsEditorPreferWordLyrics = true,
        };

        await store.SaveAsync(settings);
        var loaded = await store.LoadAsync();

        Assert.Equal(LyricsSourceKind.QQMusic, loaded.LyricsEditorSearchSource);
        Assert.True(loaded.LyricsEditorPreferWordLyrics);
    }


    [Fact]
    public async Task SaveLoad_PreservesFallbackLyricsSwitches()
    {
        var store = new JsonSettingsStore(SettingsPath);
        var settings = new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            FallbackKugouLyrics = false,
            FallbackLrclibLyrics = true,
            LyricsEditorSearchSource = LyricsSourceKind.Lrclib,
        };

        await store.SaveAsync(settings);
        var loaded = await store.LoadAsync();

        Assert.False(loaded.FallbackKugouLyrics);
        Assert.True(loaded.FallbackLrclibLyrics);
        Assert.Equal(LyricsSourceKind.Lrclib, loaded.LyricsEditorSearchSource);
    }

    [Fact]
    public async Task Load_LegacyLyricsEditorSourceName_StillParses()
    {
        Directory.CreateDirectory(_directory);
        // 旧版本把来源存成 OnlineMusicSource 的名字，改名成 LyricsSourceKind 后仍应能读回。
        await File.WriteAllTextAsync(
            SettingsPath,
            "{\"downloadDirectory\":\"D:\\\\music\",\"lyricsEditorSearchSource\":\"QQMusic\"}");

        var loaded = await new JsonSettingsStore(SettingsPath).LoadAsync();

        Assert.Equal(LyricsSourceKind.QQMusic, loaded.LyricsEditorSearchSource);
        Assert.True(loaded.FallbackKugouLyrics);
        Assert.True(loaded.FallbackLrclibLyrics);
    }
    [Fact]
    public async Task SaveLoad_ClampsAndPersistsMatchThreshold()
    {
        var store = new JsonSettingsStore(SettingsPath);
        await store.SaveAsync(new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            MatchThreshold = 0.78,
        });
        Assert.Equal(0.78, (await store.LoadAsync()).MatchThreshold, 3);

        await store.SaveAsync(new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            MatchThreshold = 5,
        });
        Assert.Equal(LyricsMatchScorer.MaxThreshold, (await store.LoadAsync()).MatchThreshold, 3);

        await store.SaveAsync(new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            MatchThreshold = double.NaN,
        });
        Assert.Equal(LyricsMatchScorer.DefaultMinScore, (await store.LoadAsync()).MatchThreshold, 3);
    }

    [Fact]
    public async Task SaveLoad_PersistsThemeWithDarkDefault()
    {
        var store = new JsonSettingsStore(SettingsPath);
        Assert.Equal(ThemeMode.Dark, (await store.LoadAsync()).Theme);

        await store.SaveAsync(new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            Theme = ThemeMode.Light,
        });
        Assert.Equal(ThemeMode.Light, (await store.LoadAsync()).Theme);
    }

    [Fact]
    public async Task Load_UnknownTheme_FallsBackToDark()
    {
        await File.WriteAllTextAsync(
            SettingsPath,
            "{\"downloadDirectory\":\"D:\\\\music\",\"theme\":99}");

        var loaded = await new JsonSettingsStore(SettingsPath).LoadAsync();

        Assert.Equal(ThemeMode.Dark, loaded.Theme);
    }

    [Fact]
    public async Task Load_UnknownLyricsEditorSource_FallsBackToNetease()
    {
        Directory.CreateDirectory(_directory);
        await File.WriteAllTextAsync(
            SettingsPath,
            "{\"downloadDirectory\":\"D:\\\\music\",\"lyricsEditorSearchSource\":42}");

        var store = new JsonSettingsStore(SettingsPath);
        var loaded = await store.LoadAsync();

        Assert.Equal(LyricsSourceKind.Netease, loaded.LyricsEditorSearchSource);
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

    [Fact]
    public async Task LoadAsync_LegacyHigherQuality_MigratesToExHighAndKeepsOtherSettings()
    {
        await File.WriteAllTextAsync(
            SettingsPath,
            "{\"downloadDirectory\":\"D:\\\\music\",\"quality\":\"Higher\",\"maxConcurrency\":5}");

        var store = new JsonSettingsStore(SettingsPath);
        var loaded = await store.LoadAsync();

        Assert.Equal(AudioQuality.ExHigh, loaded.Quality);
        Assert.Equal(@"D:\music", loaded.DownloadDirectory);
        Assert.Equal(5, loaded.MaxConcurrency);
    }

    [Fact]
    public async Task LoadAsync_UnknownQuality_FallsBackToDefaultButKeepsOtherSettings()
    {
        await File.WriteAllTextAsync(
            SettingsPath,
            "{\"downloadDirectory\":\"D:\\\\music\",\"quality\":\"Ultra\",\"maxConcurrency\":2}");

        var store = new JsonSettingsStore(SettingsPath);
        var loaded = await store.LoadAsync();

        Assert.Equal(AudioQuality.ExHigh, loaded.Quality);
        Assert.Equal(@"D:\music", loaded.DownloadDirectory);
        Assert.Equal(2, loaded.MaxConcurrency);
    }

    [Fact]
    public async Task SaveLoad_HighQualityTier_RoundTrips()
    {
        var store = new JsonSettingsStore(SettingsPath);
        await store.SaveAsync(new AppSettings
        {
            DownloadDirectory = Path.Combine(_directory, "music"),
            Quality = AudioQuality.DolbyAtmos,
        });

        var loaded = await store.LoadAsync();

        Assert.Equal(AudioQuality.DolbyAtmos, loaded.Quality);
    }

    public void Dispose()
    {
        try { Directory.Delete(_directory, recursive: true); } catch { }
    }
}
