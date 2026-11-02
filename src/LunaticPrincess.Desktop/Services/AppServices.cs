using System.Net;
using LunaticPrincess.Core.Download;
using LunaticPrincess.Core.Library;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Settings;
using LunaticPrincess.Core.Storage;
using LunaticPrincess.Core.Sync;
using LunaticPrincess.Core.Tags;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// Desktop 组合根：在这里一次性构造 Core 层的真实实现，ViewModel 只依赖 Core 接口。
/// 网络 HttpClient 由本类持有并统一释放。
/// </summary>
public sealed class AppServices : IDisposable
{
    private readonly BatchLyricsService _batchLyrics;
    private SessionAutoSaver? _sessionAutoSaver;

    private AppServices(
        HttpClient http,
        INeteaseApiClient api,
        INcmDecoder ncmDecoder,
        IAudioTagService tags,
        ILyricParser lyrics,
        ILocalLibraryService library,
        ISongMatcher matcher,
        IDownloadService download,
        INcmImportService ncmImport,
        ISyncService sync,
        ISessionStore sessionStore,
        ISettingsStore settingsStore,
        CoverImageService covers,
        IOnlineMusicSearchService onlineSearch,
        BatchLyricsService batchLyrics,
        IReadOnlyList<ILyricsSource> fallbackLyricsSources)
    {
        _batchLyrics = batchLyrics;
        OnlineSearch = onlineSearch;
        BatchLyrics = batchLyrics;
        FallbackLyricsSources = fallbackLyricsSources;
        Http = http;
        Api = api;
        NcmDecoder = ncmDecoder;
        Tags = tags;
        Lyrics = lyrics;
        Library = library;
        Matcher = matcher;
        Download = download;
        NcmImport = ncmImport;
        Sync = sync;
        SessionStore = sessionStore;
        SettingsStore = settingsStore;
        Covers = covers;
    }

    /// <summary>服务端续期/轮换登录 Cookie 后自动落盘失败时触发（可能在后台线程）。</summary>
    public event Action<Exception>? SessionPersistFailed;

    public HttpClient Http { get; }
    public INeteaseApiClient Api { get; }
    public INcmDecoder NcmDecoder { get; }
    public IAudioTagService Tags { get; }
    public ILyricParser Lyrics { get; }
    public ILocalLibraryService Library { get; }
    public ISongMatcher Matcher { get; }
    public IDownloadService Download { get; }
    public INcmImportService NcmImport { get; }
    public ISyncService Sync { get; }
    public ISessionStore SessionStore { get; }
    public ISettingsStore SettingsStore { get; }
    public CoverImageService Covers { get; }
    public IOnlineMusicSearchService OnlineSearch { get; }
    public IBatchLyricsService BatchLyrics { get; }

    /// <summary>备用词源实例（酷狗音乐、LRCLIB），顺序即兜底尝试顺序。</summary>
    public IReadOnlyList<ILyricsSource> FallbackLyricsSources { get; }

    /// <summary>把设置里的备用词源开关应用到批量匹配服务（关掉的源在兜底时跳过）。</summary>
    public void ApplyFallbackLyricsSettings(AppSettings settings)
    {
        ArgumentNullException.ThrowIfNull(settings);
        _batchLyrics.FallbackSourceFilter = source => source.Label switch
        {
            KugouLyricsSource.LyricsSourceLabel => settings.FallbackKugouLyrics,
            LrclibLyricsSource.LyricsSourceLabel => settings.FallbackLrclibLyrics,
            _ => true,
        };
    }

    /// <summary>构造全部真实依赖。所有平台相关路径（设置、登录态）使用 Core 的 AppPaths 默认位置。</summary>
    public static AppServices Create()
    {
        // 网易云客户端自行管理 Cookie（退出登录必须能立刻丢弃 MUSIC_U），
        // 因此关闭 HttpClientHandler 的自动 Cookie 容器。
        var handler = new HttpClientHandler
        {
            UseCookies = false,
            AutomaticDecompression = DecompressionMethods.All,
            AllowAutoRedirect = true,
        };
        var http = new HttpClient(handler)
        {
            // 该 HttpClient 同时用于 API 与音频下载。API 请求由 NeteaseApiClient 自身
            // 限制为单次 20 秒；这里必须给大文件下载留足时间（无损单曲可达数十 MB）。
            Timeout = TimeSpan.FromMinutes(30),
        };

        var api = new NeteaseApiClient(http);
        var ncmDecoder = new NcmDecoder();
        var tags = new AudioTagService();
        var lyrics = new LyricParser();
        var library = new LocalLibraryService(ncmDecoder, tags);
        var matcher = new SongMatcher();
        var onlineSearch = new OnlineMusicSearchService(api, http, lyrics);
        // 网易云 / QQ 都没有歌词时，按顺序用酷狗 KRC、LRCLIB 兜底补歌词。
        ILyricsSource[] fallbackLyrics =
        [
            new KugouLyricsSource(http, lyrics),
            new LrclibLyricsSource(http, lyrics),
        ];
        var batchLyrics = new BatchLyricsService(tags, onlineSearch, fallbackLyrics);
        // 下载、同步与 NCM 解锁成功后都用同一套在线匹配补全标签/封面/歌词（QQ 逐字歌词优先）。
        var downloader = new DownloadService(api, tags, lyrics, ncmDecoder, http, batchLyrics);
        var ncmImporter = new NcmImportService(ncmDecoder, tags, batchLyrics);
        var sync = new MusicSyncService(api, ncmDecoder, tags, lyrics, ncmImporter, downloader, http, batchLyrics);
        var sessionStore = new ProtectedSessionStore();
        var settingsStore = new JsonSettingsStore();
        var covers = new CoverImageService(http);

        var services = new AppServices(
            http,
            api,
            ncmDecoder,
            tags,
            lyrics,
            library,
            matcher,
            downloader,
            ncmImporter,
            sync,
            sessionStore,
            settingsStore,
            covers,
            onlineSearch,
            batchLyrics,
            fallbackLyrics);
        // 网易云会在后续请求里轮换 MUSIC_U；每次合并出新 Cookie 都要落盘，否则下次启动会拿着旧 Cookie 掉登录。
        services._sessionAutoSaver = new SessionAutoSaver(
            api,
            sessionStore,
            ex => services.SessionPersistFailed?.Invoke(ex));
        return services;
    }

    /// <summary>
    /// 等待已排队的登录态自动落盘完成。退出登录时须先 <c>SetSession(null)</c> 再等待它、最后清除 session.dat，
    /// 否则在途的落盘可能在删除之后把登录态写回磁盘。
    /// </summary>
    public Task FlushSessionSavesAsync() => _sessionAutoSaver?.FlushAsync() ?? Task.CompletedTask;

    public void Dispose()
    {
        _sessionAutoSaver?.Dispose();
        Http.Dispose();
    }
}
