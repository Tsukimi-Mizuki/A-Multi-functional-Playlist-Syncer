using System.Net;
using LunaticPrincess.Core;
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

namespace LunaticPrincess.Server.Infrastructure;

/// <summary>
/// Server 组合根：与桌面端 AppServices 保持完全一致的 Core 服务装配，只额外持有一个 ServerLog。
/// 网络 HttpClient 由本类持有并统一释放。
/// </summary>
public sealed class ServerServices : IDisposable
{
    private ServerServices(
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
        IOnlineMusicSearchService onlineSearch,
        IBatchLyricsService batchLyrics,
        ServerLog log)
    {
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
        OnlineSearch = onlineSearch;
        BatchLyrics = batchLyrics;
        Log = log;
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

    public IOnlineMusicSearchService OnlineSearch { get; }

    public IBatchLyricsService BatchLyrics { get; }

    public ServerLog Log { get; }

    public static ServerServices Create(ServerLog log)
    {
        // 网易云客户端自行管理 Cookie（退出登录必须能立刻丢弃 MUSIC_U），因此关闭自动 Cookie 容器。
        var handler = new HttpClientHandler
        {
            UseCookies = false,
            AutomaticDecompression = DecompressionMethods.All,
            AllowAutoRedirect = true,
        };
        var http = new HttpClient(handler)
        {
            // 同时用于 API 与音频下载；API 单次超时由 NeteaseApiClient 内部限制。
            Timeout = TimeSpan.FromMinutes(30),
        };

        var api = new NeteaseApiClient(http);
        var ncmDecoder = new NcmDecoder();
        var tags = new AudioTagService();
        var lyrics = new LyricParser();
        var library = new LocalLibraryService(ncmDecoder, tags);
        var matcher = new SongMatcher();
        var downloader = new DownloadService(api, tags, lyrics, ncmDecoder, http);
        var onlineSearch = new OnlineMusicSearchService(api, http, lyrics);
        var batchLyrics = new BatchLyricsService(tags, onlineSearch);
        var ncmImporter = new NcmImportService(ncmDecoder, tags, batchLyrics);
        var sync = new MusicSyncService(api, ncmDecoder, tags, lyrics, ncmImporter, downloader, http);
        var sessionStore = new ProtectedSessionStore();
        var settingsStore = new JsonSettingsStore();

        return new ServerServices(
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
            onlineSearch,
            batchLyrics,
            log);
    }

    public void Dispose() => Http.Dispose();
}
