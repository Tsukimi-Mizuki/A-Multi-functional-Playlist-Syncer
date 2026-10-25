using System.Net;
using NeteaseMusicManager.Core.Download;
using NeteaseMusicManager.Core.Library;
using NeteaseMusicManager.Core.Lyrics;
using NeteaseMusicManager.Core.Ncm;
using NeteaseMusicManager.Core.Netease;
using NeteaseMusicManager.Core.Online;
using NeteaseMusicManager.Core.Settings;
using NeteaseMusicManager.Core.Storage;
using NeteaseMusicManager.Core.Sync;
using NeteaseMusicManager.Core.Tags;

namespace NeteaseMusicManager.Desktop.Services;

/// <summary>
/// Desktop 组合根：在这里一次性构造 Core 层的真实实现，ViewModel 只依赖 Core 接口。
/// 网络 HttpClient 由本类持有并统一释放。
/// </summary>
public sealed class AppServices : IDisposable
{
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
        IOnlineMusicSearchService onlineSearch)
    {
        OnlineSearch = onlineSearch;
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
        var downloader = new DownloadService(api, tags, lyrics, ncmDecoder, http);
        var ncmImporter = new NcmImportService(ncmDecoder, tags);
        var sync = new MusicSyncService(api, ncmDecoder, tags, lyrics, ncmImporter, downloader, http);
        var sessionStore = new ProtectedSessionStore();
        var settingsStore = new JsonSettingsStore();
        var covers = new CoverImageService(http);
        var onlineSearch = new OnlineMusicSearchService(api, http);

        return new AppServices(
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
            onlineSearch);
    }

    public void Dispose()
    {
        Http.Dispose();
    }
}
