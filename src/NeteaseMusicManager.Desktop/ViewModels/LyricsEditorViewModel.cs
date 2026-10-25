using System.Globalization;
using Avalonia.Media.Imaging;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using NeteaseMusicManager.Core.Online;
using NeteaseMusicManager.Core.Tags;
using NeteaseMusicManager.Desktop.Services;

namespace NeteaseMusicManager.Desktop.ViewModels;

/// <summary>歌词与标签编辑器：读取指定音频文件的标签/封面/歌词，修改后写回标签。</summary>
public sealed partial class LyricsEditorViewModel : ObservableObject, IDisposable
{
    private readonly IAudioTagService _tags;
    private readonly IDesktopUiService _ui;
    private readonly IOnlineMusicSearchService _onlineSearch;
    private byte[]? _coverData;
    private long? _neteaseId;

    public LyricsEditorViewModel(
        IAudioTagService tags,
        IDesktopUiService ui,
        IOnlineMusicSearchService onlineSearch)
    {
        _tags = tags;
        _ui = ui;
        _onlineSearch = onlineSearch;
        SearchSources =
        [
            new(OnlineMusicSource.Netease, "网易云音乐"),
            new(OnlineMusicSource.QQMusic, "QQ 音乐"),
        ];
        _selectedSearchSource = SearchSources[0];
    }

    public async Task InitializeAsync(string? initialPath)
    {
        if (!string.IsNullOrWhiteSpace(initialPath))
        {
            await LoadFileAsync(initialPath).ConfigureAwait(true);
        }
    }

    [ObservableProperty]
    private string? _filePath;

    [ObservableProperty]
    private string _fileName = "尚未选择音频文件";

    [ObservableProperty]
    private string _fileInfoText = "支持 MP3 / FLAC / M4A / OGG / WAV 等格式";

    [ObservableProperty]
    private string _statusText = "点击「打开音频文件」读取标签和歌词。";

    [ObservableProperty]
    private bool _isBusy;

    [ObservableProperty]
    private string _title = string.Empty;

    [ObservableProperty]
    private string _artist = string.Empty;

    [ObservableProperty]
    private string _album = string.Empty;

    [ObservableProperty]
    private string _albumArtist = string.Empty;

    [ObservableProperty]
    private string _year = string.Empty;

    [ObservableProperty]
    private string _trackNumber = string.Empty;

    [ObservableProperty]
    private string _discNumber = string.Empty;

    [ObservableProperty]
    private string _genre = string.Empty;

    [ObservableProperty]
    private string _composer = string.Empty;

    [ObservableProperty]
    private string _comment = string.Empty;

    [ObservableProperty]
    private string _lyrics = string.Empty;

    [ObservableProperty]
    private Bitmap? _coverImage;

    [ObservableProperty]
    private bool _hasCover;

    // ===== 在线搜索 =====

    public IReadOnlyList<OnlineSourceOption> SearchSources { get; }

    [ObservableProperty]
    private OnlineSourceOption _selectedSearchSource;

    [ObservableProperty]
    private string _searchKeyword = string.Empty;

    [ObservableProperty]
    private string _searchStatus = string.Empty;

    [ObservableProperty]
    private bool _isSearching;

    [ObservableProperty]
    private bool _hasSearchResults;

    [ObservableProperty]
    private IReadOnlyList<OnlineSongResult> _searchResults = Array.Empty<OnlineSongResult>();

    [ObservableProperty]
    private OnlineSongResult? _selectedSearchResult;

    public bool HasFile => !string.IsNullOrWhiteSpace(FilePath);

    public string LyricsLengthText => $"{Lyrics.Length:N0} 字符";

    public bool CanInteract => !IsBusy;

    public bool CanSave => HasFile && !IsBusy;

    public bool CanReload => HasFile && !IsBusy;

    public bool CanSearch => !IsSearching && !IsBusy;

    partial void OnFilePathChanged(string? value)
    {
        OnPropertyChanged(nameof(HasFile));
        OnPropertyChanged(nameof(CanSave));
        OnPropertyChanged(nameof(CanReload));
        SaveCommand.NotifyCanExecuteChanged();
        ReloadCommand.NotifyCanExecuteChanged();
    }

    partial void OnIsBusyChanged(bool value)
    {
        OnPropertyChanged(nameof(CanInteract));
        OnPropertyChanged(nameof(CanSave));
        OnPropertyChanged(nameof(CanReload));
        OpenFileCommand.NotifyCanExecuteChanged();
        ReloadCommand.NotifyCanExecuteChanged();
        SaveCommand.NotifyCanExecuteChanged();
        SearchOnlineCommand.NotifyCanExecuteChanged();
    }

    partial void OnLyricsChanged(string value) => OnPropertyChanged(nameof(LyricsLengthText));

    partial void OnIsSearchingChanged(bool value) => SearchOnlineCommand.NotifyCanExecuteChanged();

    partial void OnSelectedSearchResultChanged(OnlineSongResult? value)
    {
        if (value is not null)
        {
            _ = ApplyOnlineSongAsync(value);
        }
    }

    [RelayCommand(CanExecute = nameof(CanSearch))]
    private async Task SearchOnlineAsync()
    {
        if (string.IsNullOrWhiteSpace(SearchKeyword))
        {
            SearchStatus = "请输入要搜索的歌曲、歌手或专辑。";
            return;
        }

        IsSearching = true;
        HasSearchResults = false;
        SearchResults = Array.Empty<OnlineSongResult>();
        SelectedSearchResult = null;
        SearchStatus = "正在搜索...";
        try
        {
            var results = await _onlineSearch
                .SearchAsync(SelectedSearchSource.Value, SearchKeyword.Trim())
                .ConfigureAwait(true);
            SearchResults = results;
            HasSearchResults = results.Count > 0;
            SearchStatus = results.Count == 0
                ? "没有找到匹配歌曲，换个关键词试试。"
                : $"找到 {results.Count} 首歌曲，点击结果即可载入标签和歌词。";
        }
        catch (Exception ex)
        {
            SearchStatus = "搜索失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            IsSearching = false;
        }
    }

    private async Task ApplyOnlineSongAsync(OnlineSongResult summary)
    {
        try
        {
            SearchStatus = $"正在获取{summary.SourceLabel}的标签和歌词...";
            var details = await _onlineSearch.GetDetailsAsync(summary).ConfigureAwait(true);
            var song = details?.Song ?? summary;

            Title = song.Title;
            Artist = song.Artist;
            Album = song.Album;
            if (song.Year is uint year)
            {
                Year = year.ToString(CultureInfo.InvariantCulture);
            }

            if (song.TrackNumber is uint track)
            {
                TrackNumber = track.ToString(CultureInfo.InvariantCulture);
            }

            if (song.DiscNumber is uint disc)
            {
                DiscNumber = disc.ToString(CultureInfo.InvariantCulture);
            }

            if (!string.IsNullOrWhiteSpace(song.Genre))
            {
                Genre = song.Genre;
            }

            if (!string.IsNullOrWhiteSpace(song.Composer))
            {
                Composer = song.Composer;
            }

            var hasLyrics = !string.IsNullOrWhiteSpace(details?.Lyrics);
            if (hasLyrics)
            {
                Lyrics = details!.Lyrics!;
            }

            SearchStatus = hasLyrics
                ? $"已载入{summary.SourceLabel}的标签与歌词，请确认后保存。"
                : $"已载入{summary.SourceLabel}的标签；未获取到歌词。";
        }
        catch (Exception ex)
        {
            SearchStatus = "获取在线信息失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
    }

    [RelayCommand(CanExecute = nameof(CanInteract))]
    private async Task OpenFileAsync()
    {
        try
        {
            var path = await _ui.PickAudioFileAsync("选择要编辑歌词和标签的音频文件").ConfigureAwait(true);
            if (!string.IsNullOrWhiteSpace(path))
            {
                await LoadFileAsync(path).ConfigureAwait(true);
            }
        }
        catch (Exception ex)
        {
            StatusText = "选择文件失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
    }

    [RelayCommand(CanExecute = nameof(CanReload))]
    private async Task ReloadAsync()
    {
        if (FilePath is not null)
        {
            await LoadFileAsync(FilePath).ConfigureAwait(true);
        }
    }

    [RelayCommand(CanExecute = nameof(CanSave))]
    private async Task SaveAsync()
    {
        if (string.IsNullOrWhiteSpace(FilePath) || !File.Exists(FilePath))
        {
            StatusText = "音频文件不存在，无法保存。";
            return;
        }

        if (!TryParseOptionalUInt(Year, "年份", out var year) ||
            !TryParseOptionalUInt(TrackNumber, "音轨号", out var track) ||
            !TryParseOptionalUInt(DiscNumber, "碟号", out var disc))
        {
            return;
        }

        IsBusy = true;
        StatusText = "正在写入标签与歌词...";
        try
        {
            var data = new AudioTagData
            {
                ReplaceAll = true,
                Title = Normalize(Title),
                Artists = SplitArtists(Artist),
                Album = Normalize(Album),
                AlbumArtist = Normalize(AlbumArtist),
                Year = year,
                TrackNumber = track,
                DiscNumber = disc,
                Genre = Normalize(Genre),
                Composer = Normalize(Composer),
                Comment = Normalize(Comment),
                PlainLyrics = string.IsNullOrWhiteSpace(Lyrics) ? null : Lyrics,
                CoverData = _coverData,
                NeteaseId = _neteaseId,
            };

            await _tags.WriteAsync(FilePath, data).ConfigureAwait(true);
            StatusText = $"已保存：{Path.GetFileName(FilePath)}";
        }
        catch (Exception ex)
        {
            StatusText = "保存失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            IsBusy = false;
        }
    }

    public async Task LoadFileAsync(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return;
        }

        IsBusy = true;
        StatusText = "正在读取标签与歌词...";
        try
        {
            var info = await _tags.ReadAsync(path).ConfigureAwait(true);
            if (info is null)
            {
                StatusText = "无法读取该文件：格式不支持或文件已损坏。";
                return;
            }

            FilePath = path;
            FileName = Path.GetFileName(path);
            Title = info.Title ?? string.Empty;
            Artist = info.Artist ?? string.Empty;
            Album = info.Album ?? string.Empty;
            AlbumArtist = info.AlbumArtist ?? string.Empty;
            Year = info.Year is uint y ? y.ToString(CultureInfo.InvariantCulture) : string.Empty;
            TrackNumber = info.TrackNumber is uint t ? t.ToString(CultureInfo.InvariantCulture) : string.Empty;
            DiscNumber = info.DiscNumber is uint d ? d.ToString(CultureInfo.InvariantCulture) : string.Empty;
            Genre = info.Genre ?? string.Empty;
            Composer = info.Composer ?? string.Empty;
            Comment = info.Comment ?? string.Empty;
            Lyrics = info.Lyrics ?? string.Empty;
            _neteaseId = info.NeteaseId;
            SetCover(info.CoverData);

            var format = string.IsNullOrWhiteSpace(info.FileFormat)
                ? Path.GetExtension(path).TrimStart('.').ToUpperInvariant()
                : info.FileFormat;
            var duration = info.DurationMs > 0
                ? TimeSpan.FromMilliseconds(info.DurationMs)
                : TimeSpan.Zero;
            var durationText = info.DurationMs > 0
                ? $"{(int)duration.TotalMinutes}:{duration.Seconds:00}"
                : "--:--";
            FileInfoText = $"{format} · {durationText} · {info.Bitrate} kbps · {FormatBytes(info.FileSize)}";
            StatusText = "标签与歌词已读取。";
        }
        catch (Exception ex)
        {
            StatusText = "读取失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            IsBusy = false;
        }
    }

    private void SetCover(byte[]? coverData)
    {
        var previous = CoverImage;
        CoverImage = null;
        HasCover = false;
        previous?.Dispose();
        _coverData = null;

        if (coverData is not { Length: > 0 })
        {
            return;
        }

        try
        {
            using var stream = new MemoryStream(coverData, writable: false);
            CoverImage = new Bitmap(stream);
            _coverData = coverData;
            HasCover = true;
        }
        catch (Exception)
        {
            CoverImage = null;
            _coverData = null;
        }
    }

    private bool TryParseOptionalUInt(string text, string fieldName, out uint? value)
    {
        value = null;
        if (string.IsNullOrWhiteSpace(text))
        {
            return true;
        }

        if (uint.TryParse(text.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var parsed))
        {
            value = parsed;
            return true;
        }

        StatusText = $"{fieldName}必须是数字。";
        return false;
    }

    private static string? Normalize(string? value) => string.IsNullOrWhiteSpace(value) ? null : value.Trim();

    private static IReadOnlyList<string> SplitArtists(string artists) =>
        artists
            .Split(new[] { '/', ';', '；', '、' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .ToArray();

    private static string FormatBytes(long bytes)
    {
        if (bytes <= 0)
        {
            return "0 B";
        }

        string[] units = ["B", "KB", "MB", "GB"];
        var order = 0;
        double size = bytes;
        while (size >= 1024 && order < units.Length - 1)
        {
            size /= 1024;
            order++;
        }

        return $"{size:0.#} {units[order]}";
    }

    public void Dispose() => CoverImage?.Dispose();
}
