using System.Collections.ObjectModel;
using Avalonia.Media.Imaging;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using NeteaseMusicManager.Core;
using NeteaseMusicManager.Core.Download;
using NeteaseMusicManager.Core.Library;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Netease;
using NeteaseMusicManager.Core.Settings;
using NeteaseMusicManager.Core.Sync;
using NeteaseMusicManager.Desktop.Services;

namespace NeteaseMusicManager.Desktop.ViewModels;

/// <summary>
/// 主窗口 ViewModel：登录态、歌单、歌曲表格、本地对比、同步/下载、NCM 导入、设置、进度与日志。
/// 所有耗时操作都在 AsyncRelayCommand 中执行并支持取消，UI 线程不做阻塞等待。
/// </summary>
public sealed partial class MainWindowViewModel : ObservableObject, IDisposable
{
    private const int MaxLogEntries = 400;

    private readonly AppServices _services;
    private readonly IDesktopUiService _ui;
    private readonly CancellationTokenSource _lifetimeCts = new();
    private readonly List<LocalTrack> _localTracks = [];

    private AppSettings _settings = new AppSettings().Normalize();
    private PlaylistDetail? _currentPlaylist;
    private CancellationTokenSource? _operationCts;
    private int _playlistLoadVersion;
    private bool _suppressPlaylistSelection;
    private bool _bulkSelectionChange;
    private bool _disposed;
    private int _busyDepth;
    private int _presentCount;
    private int _ncmCount;
    private int _missingCount;

    public MainWindowViewModel(AppServices services, IDesktopUiService ui)
    {
        _services = services;
        _ui = ui;
        RefreshEmptyStates();
        RefreshCommandStates();
    }

    // ===== 集合 =====

    public ObservableCollection<PlaylistItemViewModel> Playlists { get; } = [];

    public ObservableCollection<SongRowViewModel> Songs { get; } = [];

    public ObservableCollection<LogEntryViewModel> LogEntries { get; } = [];

    // ===== 登录态 =====

    [ObservableProperty]
    private bool _isLoggedIn;

    public bool IsNotLoggedIn => !IsLoggedIn;

    [ObservableProperty]
    private string _accountName = "未登录";

    [ObservableProperty]
    private string _accountSubtitle = "扫码登录网易云音乐";

    [ObservableProperty]
    private Bitmap? _avatar;

    // ===== 状态 =====

    [ObservableProperty]
    private bool _isSidebarExpanded = true;

    public string SidebarToggleToolTip => IsSidebarExpanded ? "收起歌单列表 (Ctrl+B)" : "展开歌单列表 (Ctrl+B)";
    public bool IsSidebarCollapsed => !IsSidebarExpanded;
    public double SidebarWidth => IsSidebarExpanded ? 272 : 88;
    public double SidebarExpandedOpacity => IsSidebarExpanded ? 1 : 0;
    public double SidebarCollapsedOpacity => IsSidebarExpanded ? 0 : 1;

    partial void OnIsSidebarExpandedChanged(bool value)
    {
        OnPropertyChanged(nameof(SidebarToggleToolTip));
        OnPropertyChanged(nameof(IsSidebarCollapsed));
        OnPropertyChanged(nameof(SidebarWidth));
        OnPropertyChanged(nameof(SidebarExpandedOpacity));
        OnPropertyChanged(nameof(SidebarCollapsedOpacity));
    }

    [RelayCommand]
    private void ToggleSidebar() => IsSidebarExpanded = !IsSidebarExpanded;

    [ObservableProperty]
    private bool _isBusy;

    public bool IsNotBusy => !IsBusy;

    [ObservableProperty]
    private string _busyText = "就绪";

    [ObservableProperty]
    private string _statusText = "就绪";

    [ObservableProperty]
    private double _progressValue;

    [ObservableProperty]
    private bool _isProgressIndeterminate;

    [ObservableProperty]
    private string? _errorMessage;

    public bool HasError => !string.IsNullOrWhiteSpace(ErrorMessage);

    // ===== 歌单/歌曲信息 =====

    [ObservableProperty]
    private PlaylistItemViewModel? _selectedPlaylist;

    [ObservableProperty]
    private SongRowViewModel? _selectedSong;

    [ObservableProperty]
    private string _currentPlaylistTitle = "未选择歌单";

    [ObservableProperty]
    private string _summaryText = "共 0 首 · 本地已存在 0 · NCM 待解密 0 · 需下载 0 · 已选 0";

    [ObservableProperty]
    private int _selectedCount;

    [ObservableProperty]
    private string _qualityText = "音质 极高 320k";

    [ObservableProperty]
    private string _lyricsModeText = "歌词 逐行";

    [ObservableProperty]
    private string _libraryStatusText = "本地曲库：未扫描";

    [ObservableProperty]
    private string _libraryPathText = "曲库目录未设置";

    [ObservableProperty]
    private string _downloadPathText = "下载目录未设置";

    [ObservableProperty]
    private bool _isPlaylistsEmpty = true;

    [ObservableProperty]
    private string _playlistEmptyHint = "登录后在此显示你的歌单";

    [ObservableProperty]
    private bool _showLoginOverlay = true;

    [ObservableProperty]
    private bool _showSelectPlaylistOverlay;

    [ObservableProperty]
    private bool _showEmptyPlaylistOverlay;

    [ObservableProperty]
    private bool _hasSongs;

    public string AppVersionText => "v1.0.0";

    // ===== 属性联动 =====

    partial void OnIsLoggedInChanged(bool value)
    {
        OnPropertyChanged(nameof(IsNotLoggedIn));
        RefreshEmptyStates();
        RefreshCommandStates();
    }

    partial void OnIsBusyChanged(bool value)
    {
        OnPropertyChanged(nameof(IsNotBusy));
        RefreshEmptyStates();
        RefreshCommandStates();
    }

    partial void OnErrorMessageChanged(string? value) => OnPropertyChanged(nameof(HasError));

    partial void OnSelectedCountChanged(int value) => SyncSelectedCommand.NotifyCanExecuteChanged();

    partial void OnSelectedPlaylistChanged(PlaylistItemViewModel? value)
    {
        if (_suppressPlaylistSelection || value is null)
        {
            return;
        }

        _ = LoadPlaylistFromSelectionAsync(value);
    }

    // ===== 启动初始化 =====

    public async Task InitializeAsync()
    {
        LogInfo("网易云音乐管理器已启动");
        var token = _lifetimeCts.Token;

        try
        {
            _settings = await _services.SettingsStore.LoadAsync(token).ConfigureAwait(true);
            LogInfo($"数据目录：{AppPaths.DataDirectory}");
            var standardDataDir = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                AppPaths.AppName);
            if (!string.Equals(
                    Path.GetFullPath(AppPaths.DataDirectory),
                    Path.GetFullPath(standardDataDir),
                    StringComparison.OrdinalIgnoreCase))
            {
                LogWarning("系统数据目录不可写，已切换到便携数据目录：" + AppPaths.DataDirectory);
            }

            LogInfo($"设置已加载，下载目录：{_settings.DownloadDirectory}");
            LogInfo($"曲库目录：{_settings.LibraryDirectory}");
        }
        catch (OperationCanceledException)
        {
            return;
        }
        catch (Exception ex)
        {
            LogWarning("读取设置失败，已使用默认设置：" + ErrorFormatter.ToFriendlyMessage(ex));
        }

        UpdatePathTexts();
        RefreshCommandStates();

        try
        {
            var session = await _services.SessionStore.LoadAsync(token).ConfigureAwait(true);
            if (session is { IsLoggedIn: true })
            {
                _services.Api.SetSession(session);
                ApplySession(session);
                LogSuccess($"已恢复登录状态：{session.DisplayName}");
                BeginBusy("正在同步账号信息…");
                IsProgressIndeterminate = true;
                try
                {
                    await RefreshAccountAndPlaylistsAsync(selectFirstPlaylist: true, token).ConfigureAwait(true);
                    LogSuccess("账号信息同步完成");
                }
                catch (OperationCanceledException)
                {
                    // 启动期间被取消（关闭窗口）。
                }
                catch (Exception ex) when (ErrorFormatter.IsAuthenticationError(ex))
                {
                    await ClearLoginStateAsync("登录状态已失效，请重新扫码登录").ConfigureAwait(true);
                }
                catch (Exception ex)
                {
                    var message = ErrorFormatter.ToFriendlyMessage(ex);
                    LogWarning("无法连接网易云，已进入离线模式：" + message);
                    SetError("无法连接网易云（已离线）：" + message);
                }
                finally
                {
                    IsProgressIndeterminate = false;
                    EndBusy();
                }
            }
            else
            {
                ApplySession(null);
                LogInfo("尚未登录，点击右上角「扫码登录」开始");
            }
        }
        catch (OperationCanceledException)
        {
            return;
        }
        catch (Exception ex)
        {
            LogError("初始化登录态失败", ex);
            SetError("初始化失败：" + ErrorFormatter.ToFriendlyMessage(ex));
        }

        RefreshEmptyStates();
    }

    // ===== 登录 / 退出 =====

    [RelayCommand(CanExecute = nameof(CanLogin))]
    private async Task LoginAsync()
    {
        if (IsLoggedIn)
        {
            return;
        }

        var dialogVm = new QrLoginViewModel(_services.Api, _services.SessionStore);
        NeteaseSession? session;
        try
        {
            session = await _ui.ShowQrLoginAsync(dialogVm).ConfigureAwait(true);
        }
        catch (Exception ex)
        {
            HandleOperationError("打开扫码登录窗口失败", ex);
            dialogVm.Dispose();
            return;
        }

        dialogVm.Dispose();
        if (session is not { IsLoggedIn: true })
        {
            LogInfo("已取消扫码登录");
            return;
        }

        _services.Api.SetSession(session);
        ApplySession(session);
        LogSuccess($"登录成功：{session.DisplayName}");

        var token = BeginOperation();
        BeginBusy("正在加载账号与歌单…");
        IsProgressIndeterminate = true;
        try
        {
            await RefreshAccountAndPlaylistsAsync(selectFirstPlaylist: true, token).ConfigureAwait(true);
        }
        catch (OperationCanceledException)
        {
            LogWarning("加载账号信息已取消");
        }
        catch (Exception ex)
        {
            HandleOperationError("加载账号与歌单失败", ex);
        }
        finally
        {
            IsProgressIndeterminate = false;
            EndBusy();
        }
    }

    private bool CanLogin() => !IsBusy && !IsLoggedIn;

    [RelayCommand(CanExecute = nameof(CanLogout))]
    private async Task LogoutAsync()
    {
        if (!IsLoggedIn)
        {
            return;
        }

        try
        {
            var confirmed = await _ui
                .ConfirmAsync("退出登录", $"确定要退出「{AccountName}」吗？已下载的本地文件不会被删除。", "退出登录", "取消")
                .ConfigureAwait(true);
            if (!confirmed)
            {
                return;
            }
        }
        catch (Exception ex)
        {
            HandleOperationError("确认退出登录失败", ex);
            return;
        }

        await ClearLoginStateAsync("已退出登录").ConfigureAwait(true);
    }

    private bool CanLogout() => !IsBusy && IsLoggedIn;

    [RelayCommand(CanExecute = nameof(CanRefreshPlaylists))]
    private async Task RefreshPlaylistsAsync()
    {
        if (!IsLoggedIn)
        {
            return;
        }

        var token = BeginOperation();
        BeginBusy("正在刷新歌单…");
        IsProgressIndeterminate = true;
        try
        {
            await RefreshAccountAndPlaylistsAsync(selectFirstPlaylist: Playlists.Count == 0, token).ConfigureAwait(true);
        }
        catch (OperationCanceledException)
        {
            LogWarning("刷新歌单已取消");
        }
        catch (Exception ex)
        {
            HandleOperationError("刷新歌单失败", ex);
        }
        finally
        {
            IsProgressIndeterminate = false;
            EndBusy();
        }
    }

    private bool CanRefreshPlaylists() => !IsBusy && IsLoggedIn;

    private async Task RefreshAccountAndPlaylistsAsync(bool selectFirstPlaylist, CancellationToken cancellationToken)
    {
        var user = await _services.Api.GetCurrentUserAsync(cancellationToken).ConfigureAwait(true);
        if (user is not null)
        {
            AccountName = string.IsNullOrWhiteSpace(user.Nickname) ? "网易云用户" : user.Nickname;
            AccountSubtitle = user.VipType > 0 ? "黑胶 VIP" : "网易云用户";
            _ = LoadAvatarAsync(user.AvatarUrl);
        }

        var userId = user?.UserId ?? _services.Api.Session?.UserId ?? 0;
        if (userId <= 0)
        {
            throw new NeteaseApiException("无法获取当前用户信息", 301);
        }

        var playlists = await _services.Api.GetUserPlaylistsAsync(userId, cancellationToken).ConfigureAwait(true);
        Playlists.Clear();
        foreach (var summary in playlists)
        {
            var item = new PlaylistItemViewModel(summary, _services.Covers);
            Playlists.Add(item);
            _ = LoadPlaylistCoverAsync(item);
        }

        LogSuccess($"已加载 {Playlists.Count} 个歌单");
        RefreshEmptyStates();

        if (selectFirstPlaylist && Playlists.Count > 0)
        {
            await SelectPlaylistAsync(Playlists[0]).ConfigureAwait(true);
        }
    }

    private async Task LoadAvatarAsync(string? url)
    {
        try
        {
            Avatar = await _services.Covers.LoadAsync(url).ConfigureAwait(true);
        }
        catch (Exception)
        {
            // 头像只是装饰。
        }
    }

    private async Task LoadPlaylistCoverAsync(PlaylistItemViewModel item)
    {
        try
        {
            await item.LoadCoverAsync().ConfigureAwait(true);
        }
        catch (Exception)
        {
            // 封面只是装饰。
        }
    }

    private void ApplySession(NeteaseSession? session)
    {
        if (session is not { IsLoggedIn: true })
        {
            Avatar = null;
            AccountName = "未登录";
            AccountSubtitle = "扫码登录网易云音乐";
            IsLoggedIn = false;
            return;
        }

        AccountName = session.DisplayName;
        AccountSubtitle = session.VipType > 0 ? "黑胶 VIP" : "网易云用户";
        IsLoggedIn = true;
    }

    private async Task ClearLoginStateAsync(string message)
    {
        try
        {
            await _services.SessionStore.ClearAsync().ConfigureAwait(true);
        }
        catch (Exception ex)
        {
            LogWarning("清除本地登录状态失败：" + ErrorFormatter.ToFriendlyMessage(ex));
        }

        _services.Api.SetSession(null);
        _suppressPlaylistSelection = true;
        SelectedPlaylist = null;
        _suppressPlaylistSelection = false;
        Playlists.Clear();
        Songs.Clear();
        _currentPlaylist = null;
        CurrentPlaylistTitle = "未选择歌单";
        ApplySession(null);
        UpdateSummary();
        RefreshEmptyStates();
        RefreshCommandStates();
        LogInfo(message);
    }

    // ===== 歌单加载 =====

    private async Task LoadPlaylistFromSelectionAsync(PlaylistItemViewModel item)
    {
        try
        {
            await LoadPlaylistDetailAsync(item).ConfigureAwait(true);
        }
        catch (OperationCanceledException)
        {
            // 正常取消。
        }
        catch (Exception ex)
        {
            HandleOperationError("加载歌单失败", ex);
        }
    }

    private async Task SelectPlaylistAsync(PlaylistItemViewModel item)
    {
        _suppressPlaylistSelection = true;
        SelectedPlaylist = item;
        _suppressPlaylistSelection = false;
        await LoadPlaylistDetailAsync(item).ConfigureAwait(true);
    }

    private async Task LoadPlaylistDetailAsync(PlaylistItemViewModel item)
    {
        var version = ++_playlistLoadVersion;
        var token = BeginOperation();
        BeginBusy($"正在加载歌单「{item.Name}」…");
        IsProgressIndeterminate = true;
        ProgressValue = 0;
        StatusText = $"正在加载歌单「{item.Name}」…";

        try
        {
            var detail = await _services.Api.GetPlaylistDetailAsync(item.Id, token).ConfigureAwait(true);
            if (version != _playlistLoadVersion)
            {
                return;
            }

            if (detail.IsPartial)
            {
                detail = await HydratePartialPlaylistAsync(detail, token).ConfigureAwait(true);
            }

            if (version != _playlistLoadVersion)
            {
                return;
            }

            _currentPlaylist = detail;
            CurrentPlaylistTitle = string.IsNullOrWhiteSpace(detail.Name) ? item.Name : detail.Name;
            LogSuccess($"歌单「{CurrentPlaylistTitle}」加载完成，共 {detail.Songs.Count} 首");
            RebuildRows();

            if (_settings.AutoMatchLocal && _localTracks.Count == 0 && detail.Songs.Count > 0)
            {
                BusyText = "正在扫描本地曲库…";
                if (await TryScanLibraryInternalAsync(token, reportErrors: false).ConfigureAwait(true))
                {
                    RebuildRows();
                }
            }
        }
        catch (OperationCanceledException)
        {
            LogWarning("加载歌单已取消");
        }
        catch (Exception ex)
        {
            HandleOperationError("加载歌单失败", ex);
        }
        finally
        {
            if (version == _playlistLoadVersion)
            {
                IsProgressIndeterminate = false;
                EndBusy();
            }
        }
    }

    /// <summary>超大歌单只返回 trackIds 时，分批补全歌曲详情。</summary>
    private async Task<PlaylistDetail> HydratePartialPlaylistAsync(PlaylistDetail detail, CancellationToken cancellationToken)
    {
        var known = detail.Songs.Select(song => song.Id).ToHashSet();
        var missing = detail.TrackIds.Where(id => !known.Contains(id)).ToList();
        if (missing.Count == 0)
        {
            return detail;
        }

        LogInfo($"歌单较大，正在补全 {missing.Count} 首歌曲详情…");
        var songs = new List<NeteaseSong>(detail.Songs);
        foreach (var batch in missing.Chunk(200))
        {
            var part = await _services.Api.GetSongDetailsAsync(batch, cancellationToken).ConfigureAwait(true);
            songs.AddRange(part);
        }

        return new PlaylistDetail
        {
            Id = detail.Id,
            Name = detail.Name,
            CoverUrl = detail.CoverUrl,
            Description = detail.Description,
            CreatorNickname = detail.CreatorNickname,
            TrackCount = detail.TrackCount,
            Songs = songs,
            TrackIds = detail.TrackIds,
            IsPartial = false,
        };
    }

    // ===== 本地曲库扫描 / 对比 =====

    [RelayCommand(CanExecute = nameof(CanRunAction))]
    private async Task ScanLibraryAsync()
    {
        var token = BeginOperation();
        BeginBusy("正在扫描本地曲库…");
        try
        {
            if (await TryScanLibraryInternalAsync(token, reportErrors: true).ConfigureAwait(true))
            {
                RebuildRows();
            }
        }
        finally
        {
            EndBusy();
        }
    }

    /// <summary>扫描设置中的曲库目录并缓存本地文件；不负责重建表格（调用方决定）。</summary>
    private async Task<bool> TryScanLibraryInternalAsync(CancellationToken token, bool reportErrors)
    {
        var libraryDirectory = _settings.LibraryDirectory;
        if (!Directory.Exists(libraryDirectory))
        {
            var message = $"曲库目录不存在：{libraryDirectory}";
            LogWarning(message + "，请在「设置」中重新选择曲库目录");
            if (reportErrors)
            {
                SetError(message + "。请打开「设置」选择曲库目录。");
            }

            return false;
        }

        try
        {
            IsProgressIndeterminate = false;
            ProgressValue = 0;
            var progress = new Progress<LibraryScanProgress>(p =>
            {
                if (p.TotalFiles > 0)
                {
                    ProgressValue = Math.Min(100, p.ScannedFiles * 100.0 / p.TotalFiles);
                }

                BusyText = $"正在扫描本地曲库 {p.ScannedFiles}/{p.TotalFiles}";
                StatusText = $"扫描：{Path.GetFileName(p.CurrentFile)}";
            });

            var tracks = await _services.Library
                .ScanAsync([libraryDirectory], progress, token)
                .ConfigureAwait(true);

            _localTracks.Clear();
            _localTracks.AddRange(tracks);
            var ncmCount = tracks.Count(t => t.IsNcm);
            LogSuccess($"本地曲库扫描完成：{tracks.Count} 个文件（其中 NCM {ncmCount} 个）");
            UpdatePathTexts();
            return true;
        }
        catch (OperationCanceledException)
        {
            LogWarning("扫描本地曲库已取消");
            return false;
        }
        catch (Exception ex)
        {
            if (reportErrors)
            {
                HandleOperationError("扫描本地曲库失败", ex);
            }
            else
            {
                LogWarning("刷新本地曲库失败：" + ErrorFormatter.ToFriendlyMessage(ex));
            }

            return false;
        }
    }

    [RelayCommand(CanExecute = nameof(CanRecomputeDiff))]
    private void RecomputeDiff()
    {
        if (_currentPlaylist is null)
        {
            LogWarning("请先在左侧选择歌单");
            return;
        }

        RebuildRows();
        StatusText = "已重新对比本地曲库";
    }

    private bool CanRecomputeDiff() => !IsBusy && Songs.Count > 0;

    [RelayCommand(CanExecute = nameof(CanSelectMissing))]
    private void SelectMissing()
    {
        _bulkSelectionChange = true;
        try
        {
            foreach (var row in Songs.Where(row => row.Availability != LocalAvailability.Available))
            {
                row.IsSelected = true;
            }
        }
        finally
        {
            _bulkSelectionChange = false;
        }

        SelectedCount = Songs.Count(row => row.IsSelected);
        SummaryText = BuildSummaryText();
        LogInfo($"已勾选 {SelectedCount} 首需要同步的歌曲");
    }

    private bool CanSelectMissing() => !IsBusy && Songs.Count > 0;

    private bool CanRunAction() => !IsBusy;

    /// <summary>根据当前歌单歌曲 + 最新本地曲库重建表格（保留已勾选状态）。</summary>
    private void RebuildRows()
    {
        var songs = _currentPlaylist?.Songs ?? Array.Empty<NeteaseSong>();
        var previouslySelected = Songs.Where(row => row.IsSelected).Select(row => row.Song.Id).ToHashSet();

        var matches = new Dictionary<long, SongMatch>();
        if (songs.Count > 0 && _localTracks.Count > 0)
        {
            foreach (var match in _services.Matcher.Match(songs, _localTracks))
            {
                matches.TryAdd(match.Song.Id, match);
            }
        }

        var rows = new List<SongRowViewModel>(songs.Count);
        var index = 1;
        foreach (var song in songs)
        {
            var match = matches.TryGetValue(song.Id, out var found)
                ? found
                : new SongMatch { Song = song, Availability = LocalAvailability.Missing };
            var row = new SongRowViewModel(match, index++, OnRowSelectionChanged);
            if (previouslySelected.Contains(song.Id))
            {
                row.IsSelected = true;
            }

            rows.Add(row);
        }

        Songs.Clear();
        foreach (var row in rows)
        {
            Songs.Add(row);
        }

        UpdateSummary();
        RefreshEmptyStates();
        RefreshCommandStates();

        if (_currentPlaylist is not null)
        {
            var hint = _localTracks.Count == 0 ? "（尚未扫描本地曲库）" : string.Empty;
            LogInfo($"本地对比：已存在 {_presentCount}，NCM 待解密 {_ncmCount}，需下载 {_missingCount}{hint}");
        }
    }

    private void OnRowSelectionChanged()
    {
        if (_bulkSelectionChange)
        {
            return;
        }

        SelectedCount = Songs.Count(row => row.IsSelected);
        SummaryText = BuildSummaryText();
    }

    private void UpdateSummary()
    {
        _presentCount = Songs.Count(row => row.Availability == LocalAvailability.Available);
        _ncmCount = Songs.Count(row => row.Availability == LocalAvailability.EncryptedNcm);
        _missingCount = Songs.Count(row => row.Availability == LocalAvailability.Missing);
        SelectedCount = Songs.Count(row => row.IsSelected);
        SummaryText = BuildSummaryText();
    }

    private string BuildSummaryText() =>
        $"共 {Songs.Count} 首 · 本地已存在 {_presentCount} · NCM 待解密 {_ncmCount} · 需下载 {_missingCount} · 已选 {SelectedCount}";

    private void ClearSelection()
    {
        _bulkSelectionChange = true;
        try
        {
            foreach (var row in Songs)
            {
                row.IsSelected = false;
            }
        }
        finally
        {
            _bulkSelectionChange = false;
        }

        SelectedCount = 0;
        SummaryText = BuildSummaryText();
    }

    // ===== 同步 / 下载 =====

    [RelayCommand(CanExecute = nameof(CanSyncSelected))]
    private async Task SyncSelectedAsync()
    {
        var rows = Songs.Where(row => row.IsSelected).ToList();
        if (rows.Count == 0)
        {
            LogWarning("请先勾选需要同步的歌曲");
            return;
        }

        var token = BeginOperation();
        BeginBusy($"正在同步 {rows.Count} 首歌曲…");
        IsProgressIndeterminate = false;
        ProgressValue = 0;
        StatusText = $"开始同步 {rows.Count} 首歌曲";
        LogInfo($"开始同步 {rows.Count} 首歌曲 → {_settings.DownloadDirectory}（音质：{_settings.Quality.ToDisplayName()}）");

        try
        {
            var requests = rows
                .Select(row => new SyncRequest { Song = row.Song, LocalMatch = row.Match.LocalTrack })
                .ToList();
            var options = new SyncOptions
            {
                Download = BuildDownloadOptions(),
                RedownloadExisting = _settings.OverwriteExisting,
            };

            var lastCompleted = 0;
            var progress = new Progress<SyncProgress>(p =>
            {
                if (p.Total > 0)
                {
                    ProgressValue = Math.Min(100, p.Completed * 100.0 / p.Total);
                }

                BusyText = $"正在同步 {p.Completed}/{p.Total}";
                StatusText = string.IsNullOrWhiteSpace(p.Message)
                    ? $"{p.SongName} · {p.Stage}"
                    : $"{p.SongName} · {p.Stage} · {p.Message}";
                if (p.Completed > lastCompleted)
                {
                    lastCompleted = p.Completed;
                    LogInfo($"[{p.Completed}/{p.Total}] {p.SongName} · {p.Stage}");
                }
            });

            var summary = await _services.Sync
                .SyncAsync(requests, options, progress, token)
                .ConfigureAwait(true);

            LogSuccess(
                $"同步完成：成功 {summary.SuccessCount} 首" +
                $"（下载 {summary.DownloadedCount}、NCM 解密 {summary.DecryptedCount}、跳过 {summary.SkippedCount}），" +
                $"失败 {summary.FailedCount} 首");
            foreach (var failure in summary.Results.Where(result => !result.Success).Take(15))
            {
                LogError($"同步失败：{failure.SongName} · {failure.Message ?? "未知错误"}");
            }

            StatusText = $"同步完成：成功 {summary.SuccessCount} 首，失败 {summary.FailedCount} 首";
            ProgressValue = 100;
            ClearSelection();

            if (_currentPlaylist is not null)
            {
                BusyText = "正在刷新本地状态…";
                if (await TryScanLibraryInternalAsync(token, reportErrors: false).ConfigureAwait(true))
                {
                    RebuildRows();
                }
            }
        }
        catch (OperationCanceledException)
        {
            LogWarning("同步已取消");
            StatusText = "同步已取消";
        }
        catch (Exception ex)
        {
            HandleOperationError("同步失败", ex);
        }
        finally
        {
            EndBusy();
        }
    }

    private bool CanSyncSelected() => !IsBusy && SelectedCount > 0;

    private DownloadOptions BuildDownloadOptions() => new()
    {
        TargetDirectory = _settings.DownloadDirectory,
        Quality = _settings.Quality,
        LyricsMode = _settings.LyricsMode,
        WriteCover = _settings.WriteCover,
        OverwriteExisting = _settings.OverwriteExisting,
        SkipExisting = _settings.SkipExisting,
        FileNameTemplate = _settings.FileNameTemplate,
        MaxConcurrency = _settings.MaxConcurrency,
    };

    // ===== NCM 导入 / 设置 / 打开目录 =====

    [RelayCommand(CanExecute = nameof(CanRunAction))]
    private async Task ImportNcmAsync()
    {
        var dialogVm = new NcmImportViewModel(_services.NcmImport, _settings);
        try
        {
            await _ui.ShowNcmImportAsync(dialogVm).ConfigureAwait(true);
            if (dialogVm.ImportedCount > 0)
            {
                LogSuccess($"NCM 导入完成：成功 {dialogVm.ImportedCount} 个，共 {dialogVm.TotalCount} 个");
                if (!string.IsNullOrWhiteSpace(dialogVm.LastOutputDirectory) &&
                    string.Equals(
                        Path.GetFullPath(dialogVm.LastOutputDirectory),
                        Path.GetFullPath(_settings.LibraryDirectory),
                        StringComparison.OrdinalIgnoreCase) &&
                    _currentPlaylist is not null)
                {
                    var token = BeginOperation();
                    BeginBusy("正在刷新本地状态…");
                    try
                    {
                        if (await TryScanLibraryInternalAsync(token, reportErrors: false).ConfigureAwait(true))
                        {
                            RebuildRows();
                        }
                    }
                    finally
                    {
                        EndBusy();
                    }
                }
                else
                {
                    LogInfo("解密文件输出到下载目录；如需参与本地对比，请把曲库目录设置为同一路径后重新扫描");
                }
            }
            else if (dialogVm.TotalCount > 0)
            {
                LogWarning($"NCM 导入完成，但全部失败（共 {dialogVm.TotalCount} 个）");
            }
        }
        catch (Exception ex)
        {
            HandleOperationError("导入解密 NCM 失败", ex);
        }
        finally
        {
            dialogVm.Dispose();
        }
    }

    [RelayCommand]
    private Task OpenLyricsEditorAsync() => OpenLyricsEditorForFileAsync(SelectedSong?.LocalPath);

    public async Task OpenLyricsEditorForFileAsync(string? initialPath)
    {
        try
        {
            var viewModel = new LyricsEditorViewModel(
                _services.Tags,
                _ui,
                _services.OnlineSearch,
                _services.BatchLyrics);
            await viewModel.InitializeAsync(initialPath).ConfigureAwait(true);
            await _ui.ShowLyricsEditorAsync(viewModel).ConfigureAwait(true);
        }
        catch (Exception ex)
        {
            HandleOperationError("打开歌词编辑器失败", ex);
        }
    }

    [RelayCommand(CanExecute = nameof(CanRunAction))]
    private async Task OpenSettingsAsync()
    {
        var settingsVm = new SettingsViewModel(_settings, _services.SettingsStore);
        AppSettings? saved;
        try
        {
            saved = await _ui.ShowSettingsAsync(settingsVm).ConfigureAwait(true);
        }
        catch (Exception ex)
        {
            HandleOperationError("打开设置失败", ex);
            return;
        }

        if (saved is null)
        {
            return;
        }

        var libraryChanged = !string.Equals(saved.LibraryDirectory, _settings.LibraryDirectory, StringComparison.OrdinalIgnoreCase);
        _settings = saved;
        UpdatePathTexts();
        LogSuccess("设置已保存");
        RefreshCommandStates();

        if (!libraryChanged)
        {
            return;
        }

        _localTracks.Clear();
        UpdatePathTexts();
        LogInfo($"曲库目录已变更为：{_settings.LibraryDirectory}");
        if (_currentPlaylist is null || Songs.Count == 0)
        {
            return;
        }

        var token = BeginOperation();
        BeginBusy("曲库目录已变更，正在重新扫描…");
        try
        {
            if (await TryScanLibraryInternalAsync(token, reportErrors: true).ConfigureAwait(true))
            {
                RebuildRows();
            }
        }
        finally
        {
            EndBusy();
        }
    }

    [RelayCommand]
    private void OpenDownloadFolder()
    {
        try
        {
            var directory = string.IsNullOrWhiteSpace(_settings.DownloadDirectory)
                ? AppPaths.DefaultDownloadDirectory
                : _settings.DownloadDirectory;
            _ui.OpenFolder(directory);
            LogInfo($"已打开下载目录：{directory}");
        }
        catch (Exception ex)
        {
            HandleOperationError("打开下载目录失败", ex);
        }
    }

    [RelayCommand(CanExecute = nameof(CanCancel))]
    private void Cancel()
    {
        try
        {
            _operationCts?.Cancel();
        }
        catch (ObjectDisposedException)
        {
            // 已结束。
        }
    }

    private bool CanCancel() => IsBusy;

    [RelayCommand]
    private void ClearError() => ErrorMessage = null;

    [RelayCommand]
    private void ClearLog() => LogEntries.Clear();

    // ===== 通用 =====

    private CancellationToken BeginOperation()
    {
        _operationCts?.Dispose();
        _operationCts = CancellationTokenSource.CreateLinkedTokenSource(_lifetimeCts.Token);
        return _operationCts.Token;
    }

    private void BeginBusy(string text)
    {
        _busyDepth++;
        BusyText = text;
        if (!IsBusy)
        {
            IsBusy = true;
        }

        RefreshCommandStates();
    }

    private void EndBusy()
    {
        if (_busyDepth > 0)
        {
            _busyDepth--;
        }

        if (_busyDepth == 0)
        {
            IsBusy = false;
            BusyText = "就绪";
            IsProgressIndeterminate = false;
            ProgressValue = 0;
        }

        RefreshCommandStates();
    }

    private void RefreshCommandStates()
    {
        LoginCommand.NotifyCanExecuteChanged();
        LogoutCommand.NotifyCanExecuteChanged();
        RefreshPlaylistsCommand.NotifyCanExecuteChanged();
        ScanLibraryCommand.NotifyCanExecuteChanged();
        RecomputeDiffCommand.NotifyCanExecuteChanged();
        SelectMissingCommand.NotifyCanExecuteChanged();
        SyncSelectedCommand.NotifyCanExecuteChanged();
        ImportNcmCommand.NotifyCanExecuteChanged();
        OpenSettingsCommand.NotifyCanExecuteChanged();
        CancelCommand.NotifyCanExecuteChanged();
    }

    private void RefreshEmptyStates()
    {
        IsPlaylistsEmpty = Playlists.Count == 0;
        PlaylistEmptyHint = IsLoggedIn
            ? "账号下暂无歌单，点击「刷新」重试"
            : "登录后在此显示你的歌单";
        HasSongs = Songs.Count > 0;
        ShowLoginOverlay = !IsLoggedIn && Songs.Count == 0;
        ShowSelectPlaylistOverlay = IsLoggedIn && Songs.Count == 0 && SelectedPlaylist is null;
        ShowEmptyPlaylistOverlay = IsLoggedIn && Songs.Count == 0 && SelectedPlaylist is not null && !IsBusy;
    }

    private void UpdatePathTexts()
    {
        QualityText = "音质 " + _settings.Quality.ToDisplayName();
        LyricsModeText = _settings.LyricsMode switch
        {
            LyricsWriteMode.None => "歌词 不写入",
            LyricsWriteMode.Word => "歌词 逐字",
            _ => "歌词 逐行",
        };
        LibraryStatusText = $"本地曲库：{_localTracks.Count} 首";
        LibraryPathText = "曲库目录：" + _settings.LibraryDirectory;
        DownloadPathText = "下载目录：" + _settings.DownloadDirectory;
    }

    private void SetError(string message) => ErrorMessage = message;

    private void HandleOperationError(string operation, Exception exception)
    {
        if (exception is OperationCanceledException)
        {
            LogWarning($"{operation}已取消");
            return;
        }

        var message = ErrorFormatter.ToFriendlyMessage(exception);
        LogError($"{operation}：{message}", exception);

        if (ErrorFormatter.IsAuthenticationError(exception))
        {
            _ = ClearLoginStateAsync("登录状态已失效，请重新扫码登录");
            return;
        }

        SetError($"{operation}：{message}".Replace("：：", "："));
    }

    private void Log(LogLevel level, string message)
    {
        if (!UiThread.IsOnUiThread)
        {
            UiThread.Post(() => Log(level, message));
            return;
        }

        LogEntries.Add(new LogEntryViewModel(level, message));
        while (LogEntries.Count > MaxLogEntries)
        {
            LogEntries.RemoveAt(0);
        }
    }

    private void LogInfo(string message) => Log(LogLevel.Info, message);

    private void LogSuccess(string message) => Log(LogLevel.Success, message);

    private void LogWarning(string message) => Log(LogLevel.Warning, message);

    private void LogError(string message, Exception? exception = null)
    {
        var text = exception is null
            ? message
            : $"{message}（{exception.GetType().Name}: {exception.Message}）";
        Log(LogLevel.Error, text);
    }

    public void Shutdown()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        try
        {
            _operationCts?.Cancel();
        }
        catch (ObjectDisposedException)
        {
            // 已释放。
        }

        _lifetimeCts.Cancel();
        _operationCts?.Dispose();
        _lifetimeCts.Dispose();
    }

    public void Dispose() => Shutdown();
}
