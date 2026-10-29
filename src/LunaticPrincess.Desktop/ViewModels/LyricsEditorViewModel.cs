using System.Globalization;
using System.Text.RegularExpressions;
using Avalonia.Media.Imaging;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Settings;
using LunaticPrincess.Core.Tags;
using LunaticPrincess.Desktop.Services;

namespace LunaticPrincess.Desktop.ViewModels;

/// <summary>歌词与标签编辑器：读取指定音频文件的标签/封面/歌词，修改后写回标签。</summary>
public sealed partial class LyricsEditorViewModel : ObservableObject, IDisposable
{
    private readonly IAudioTagService _tags;
    private readonly IDesktopUiService _ui;
    private readonly IOnlineMusicSearchService _onlineSearch;
    private readonly IBatchLyricsService _batchLyrics;
    private readonly OnlineSourcePriority _tagPriority;
    private readonly OnlineSourcePriority _lyricsPriority;
    private readonly AppSettings? _settings;
    private readonly ISettingsStore? _settingsStore;
    private readonly IReadOnlyList<ILyricsSource> _fallbackSources;
    private byte[]? _coverData;
    private long? _neteaseId;
    // 用于匹配度打分的本地原始信息（应用在线结果后 Title/Artist 会变，打分仍以文件为准）。
    private string _localTitle = string.Empty;
    private string _localArtist = string.Empty;
    private string _localAlbum = string.Empty;
    private long _localDurationMs;

    public LyricsEditorViewModel(
        IAudioTagService tags,
        IDesktopUiService ui,
        IOnlineMusicSearchService onlineSearch,
        IBatchLyricsService batchLyrics,
        OnlineSourcePriority tagPriority,
        OnlineSourcePriority lyricsPriority,
        AppSettings? settings = null,
        ISettingsStore? settingsStore = null,
        IReadOnlyList<ILyricsSource>? fallbackSources = null)
    {
        _tags = tags;
        _ui = ui;
        _onlineSearch = onlineSearch;
        _batchLyrics = batchLyrics;
        _tagPriority = tagPriority;
        _lyricsPriority = lyricsPriority;
        _settings = settings;
        _settingsStore = settingsStore;
        _fallbackSources = fallbackSources ?? Array.Empty<ILyricsSource>();
        _matchThreshold = settings?.MatchThreshold ?? LyricsMatchScorer.DefaultMinScore;
        // 主词源带标签与封面；酷狗 / LRCLIB 是备用词源，只提供歌词。
        SearchSources =
        [
            new(LyricsSourceKind.Netease, LyricsSourceKind.Netease.ToDisplayLabel()),
            new(LyricsSourceKind.QQMusic, LyricsSourceKind.QQMusic.ToDisplayLabel()),
            new(LyricsSourceKind.Kugou, LyricsSourceKind.Kugou.ToDisplayLabel()),
            new(LyricsSourceKind.Lrclib, LyricsSourceKind.Lrclib.ToDisplayLabel()),
        ];
        // 在线搜索来源与歌词格式跟随上次使用（记忆在 settings.json，无开关）。
        var rememberedSource = settings?.LyricsEditorSearchSource ?? LyricsSourceKind.Netease;
        _selectedSearchSource = SearchSources.FirstOrDefault(option => option.Value == rememberedSource) ?? SearchSources[0];
        SearchLyricModeOptions =
        [
            new(false, "逐行歌词"),
            new(true, "逐字歌词（有则优先）"),
        ];
        var rememberedWordLyrics = settings?.LyricsEditorPreferWordLyrics ?? false;
        _selectedSearchLyricMode = SearchLyricModeOptions.FirstOrDefault(option => option.PreferWord == rememberedWordLyrics)
            ?? SearchLyricModeOptions[0];
    }

    public async Task InitializeAsync(string? initialPath)
    {
        if (!string.IsNullOrWhiteSpace(initialPath))
        {
            await LoadFileAsync(initialPath).ConfigureAwait(true);
        }
    }

    /// <summary>给歌词编辑窗口的“批量标签”按钮创建子窗口 ViewModel。</summary>
    public BatchLyricsViewModel CreateBatchLyricsViewModel()
    {
        var directory = FilePath is { Length: > 0 } path ? Path.GetDirectoryName(path) : null;
        return new BatchLyricsViewModel(
            _batchLyrics,
            _ui,
            directory,
            _tagPriority,
            _lyricsPriority,
            MatchThreshold);
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

    public IReadOnlyList<LyricsSourceOption> SearchSources { get; }

    public IReadOnlyList<OnlineLyricModeOption> SearchLyricModeOptions { get; }

    [ObservableProperty]
    private LyricsSourceOption _selectedSearchSource;

    [ObservableProperty]
    private OnlineLyricModeOption _selectedSearchLyricMode;

    [ObservableProperty]
    private string _searchKeyword = string.Empty;

    [ObservableProperty]
    private string _searchStatus = string.Empty;

    [ObservableProperty]
    private bool _isSearching;

    [ObservableProperty]
    private bool _hasSearchResults;

    [ObservableProperty]
    private IReadOnlyList<OnlineSearchResultItem> _searchResults = Array.Empty<OnlineSearchResultItem>();

    /// <summary>在线搜索结果的当前选中项（带匹配度打分）。</summary>
    [ObservableProperty]
    private OnlineSearchResultItem? _selectedSearchResult;

    /// <summary>匹配度阈值：低于该分数的搜索结果视为不可用，同时影响批量获取的精度。</summary>
    [ObservableProperty]
    private double _matchThreshold = LyricsMatchScorer.DefaultMinScore;

    public string MatchThresholdText => $"{Math.Round(MatchThreshold * 100):0}%";

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

    partial void OnSelectedSearchResultChanged(OnlineSearchResultItem? value)
    {
        if (value is null)
        {
            return;
        }

        if (value.Fallback is { } fallback)
        {
            ApplyFallbackLyrics(fallback);
        }
        else
        {
            _ = ApplyOnlineSongAsync(value.Song!);
        }
    }

    partial void OnMatchThresholdChanged(double value)
    {
        var clamped = Math.Clamp(
            Math.Round(value, 2),
            LyricsMatchScorer.MinThreshold,
            LyricsMatchScorer.MaxThreshold);
        if (Math.Abs(clamped - value) > 0.0001)
        {
            MatchThreshold = clamped;
            return;
        }

        OnPropertyChanged(nameof(MatchThresholdText));
        // RememberThreshold 会顺带刷新每个结果的“低于阈值”标注。
        RememberThreshold();
    }

    partial void OnSelectedSearchLyricModeChanged(OnlineLyricModeOption value)
    {
        RememberOptions();
        if (SelectedSearchResult is not { } result)
        {
            return;
        }

        if (result.Fallback is { } fallback)
        {
            ApplyFallbackLyrics(fallback);
        }
        else
        {
            _ = ApplyOnlineSongAsync(result.Song!);
        }
    }

    partial void OnSelectedSearchSourceChanged(LyricsSourceOption value) => RememberOptions();

    /// <summary>匹配度阈值改动后只更新内存，真正落盘在搜索时与关闭窗口时做（避免拖动滑块频繁写盘）。</summary>
    private void RememberThreshold()
    {
        if (_settings is not null)
        {
            _settings.MatchThreshold = Math.Round(MatchThreshold, 2);
        }

        foreach (var item in SearchResults)
        {
            item.UpdateThreshold(MatchThreshold);
        }
    }

    private async Task PersistThresholdAsync()
    {
        if (_settings is null || _settingsStore is null)
        {
            return;
        }

        await PersistOptionsAsync(_settingsStore, _settings).ConfigureAwait(true);
    }

    /// <summary>把在线搜索来源与歌词格式记到设置里（默认开启，无界面开关）。</summary>
    private void RememberOptions()
    {
        if (_settings is null || _settingsStore is null)
        {
            return;
        }

        var source = SelectedSearchSource.Value;
        var preferWord = SelectedSearchLyricMode.PreferWord;
        if (_settings.LyricsEditorSearchSource == source && _settings.LyricsEditorPreferWordLyrics == preferWord)
        {
            return;
        }

        _settings.LyricsEditorSearchSource = source;
        _settings.LyricsEditorPreferWordLyrics = preferWord;
        _ = PersistOptionsAsync(_settingsStore, _settings);
    }

    private static async Task PersistOptionsAsync(ISettingsStore settingsStore, AppSettings settings)
    {
        try
        {
            await settingsStore.SaveAsync(settings, CancellationToken.None).ConfigureAwait(false);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or NotSupportedException)
        {
            // 记忆失败不影响编辑；下次改选项会再试。
        }
    }

    [RelayCommand(CanExecute = nameof(CanSearch))]
    private async Task SearchOnlineAsync()
    {
        // 备用词源按当前文件的标题/歌手/时长检索，不需要关键词。
        if (SelectedSearchSource.Value.IsFallback())
        {
            await SearchFallbackLyricsAsync(SelectedSearchSource.Value).ConfigureAwait(true);
            return;
        }

        if (string.IsNullOrWhiteSpace(SearchKeyword))
        {
            SearchStatus = "请输入要搜索的歌曲、歌手或专辑。";
            return;
        }

        IsSearching = true;
        HasSearchResults = false;
        SearchResults = Array.Empty<OnlineSearchResultItem>();
        SelectedSearchResult = null;
        SearchStatus = "正在搜索...";
        await PersistThresholdAsync().ConfigureAwait(true);
        try
        {
            var results = await _onlineSearch
                .SearchAsync(SelectedSearchSource.Value.ToMusicSource()!.Value, SearchKeyword.Trim())
                .ConfigureAwait(true);
            var items = BuildScoredResults(results);
            SearchResults = items;
            HasSearchResults = items.Count > 0;
            if (items.Count == 0)
            {
                SearchStatus = "没有找到匹配歌曲，换个关键词试试。";
            }
            else
            {
                // 有本地文件时按匹配度排序，并自动选中第一个达到阈值的结果。
                var acceptable = HasFile ? items.FirstOrDefault(item => !item.BelowThreshold) : null;
                if (acceptable is not null)
                {
                    SelectedSearchResult = acceptable;
                }

                SearchStatus = HasFile
                    ? $"找到 {items.Count} 首歌曲，按匹配度从高到低排序（低于 {MatchThresholdText} 的已标注）。"
                    : $"找到 {items.Count} 首歌曲，点击结果即可载入标签和歌词。";
            }
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

    /// <summary>
    /// 备用词源（酷狗音乐 / LRCLIB）：按当前文件的标题、歌手与时长检索一份歌词。
    /// 备用源只提供歌词，所以结果只有一条，载入时也不改标签与封面。
    /// </summary>
    private async Task SearchFallbackLyricsAsync(LyricsSourceKind kind)
    {
        IsSearching = true;
        HasSearchResults = false;
        SearchResults = Array.Empty<OnlineSearchResultItem>();
        SelectedSearchResult = null;
        var label = kind.ToDisplayLabel();
        SearchStatus = $"正在从{label}检索歌词...";
        await PersistThresholdAsync().ConfigureAwait(true);
        try
        {
            if (!HasFile)
            {
                SearchStatus = "备用词源按当前音频的标题、歌手和时长检索，请先打开要编辑的音频文件。";
                return;
            }

            var source = FindFallbackSource(kind);
            if (source is null)
            {
                SearchStatus = $"{label}不可用。";
                return;
            }

            var queries = LyricsMatchScorer.BuildQueries(_localTitle, _localArtist);
            var candidate = await source
                .FetchAsync(queries, _localTitle, _localArtist, _localAlbum, _localDurationMs, MatchThreshold)
                .ConfigureAwait(true);
            if (candidate is null)
            {
                SearchStatus = $"没有在{label}找到匹配的歌词；可以改用网易云音乐 / QQ 音乐按关键词搜索。";
                return;
            }

            var score = LyricsMatchScorer.Score(
                _localTitle,
                _localArtist,
                _localAlbum,
                _localDurationMs,
                candidate.Title,
                candidate.Artist,
                candidate.Album,
                candidate.DurationMs);
            var item = new OnlineSearchResultItem(candidate, score, belowThreshold: false);
            SearchResults = [item];
            HasSearchResults = true;
            SelectedSearchResult = item;
        }
        catch (Exception ex)
        {
            SearchStatus = $"{label}检索失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            IsSearching = false;
        }
    }

    private ILyricsSource? FindFallbackSource(LyricsSourceKind kind)
    {
        var label = kind switch
        {
            LyricsSourceKind.Kugou => KugouLyricsSource.LyricsSourceLabel,
            LyricsSourceKind.Lrclib => LrclibLyricsSource.LyricsSourceLabel,
            _ => null,
        };
        return label is null
            ? null
            : _fallbackSources.FirstOrDefault(source => string.Equals(source.Label, label, StringComparison.Ordinal));
    }

    /// <summary>把备用词源命中的歌词填进编辑框（不动标签与封面）。</summary>
    private void ApplyFallbackLyrics(LyricsSourceCandidate candidate)
    {
        var preferWord = SelectedSearchLyricMode.PreferWord;
        var lyrics = candidate.GetLyrics(preferWord, bilingual: true);
        if (string.IsNullOrWhiteSpace(lyrics))
        {
            SearchStatus = $"{candidate.SourceLabel}没有返回可用歌词。";
            return;
        }

        Lyrics = lyrics!;
        var formatLabel = preferWord
            ? candidate.HasWordLyrics ? "逐字歌词" : "逐行歌词（该来源无逐字）"
            : "逐行歌词";
        formatLabel = candidate.HasTranslation
            ? "双语" + formatLabel
            : formatLabel + "（该来源无翻译）";
        SearchStatus = $"已从{candidate.SourceLabel}载入{formatLabel}（仅歌词，标签与封面保持当前内容），请确认后保存。";
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

            var preferWord = SelectedSearchLyricMode.PreferWord;
            var lyrics = details?.GetLyrics(preferWord, bilingual: true);
            var hasLyrics = !string.IsNullOrWhiteSpace(lyrics);
            var wordFallback = preferWord && hasLyrics && details?.HasWordLyrics != true;
            var hasTranslation = details?.HasTranslation == true;
            if (hasLyrics)
            {
                Lyrics = lyrics!;
            }

            var hasCover = details?.CoverData is { Length: > 0 };
            if (hasCover)
            {
                SetCover(details!.CoverData);
            }

            var detailParts = new List<string>();
            if (hasLyrics)
            {
                var formatLabel = preferWord
                    ? wordFallback ? "逐行歌词（该来源无逐字）" : "逐字歌词"
                    : "逐行歌词";
                formatLabel = hasTranslation
                    ? "双语" + formatLabel
                    : formatLabel + "（该来源无翻译）";

                detailParts.Add(formatLabel);
            }
            else
            {
                detailParts.Add("未获取到歌词");
            }

            if (hasCover)
            {
                detailParts.Add("封面");
            }

            SearchStatus = $"已载入{summary.SourceLabel}的标签{string.Join("、", detailParts)}，请确认后保存。";
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
        var loaded = false;
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
            _localTitle = info.Title?.Trim() ?? string.Empty;
            _localArtist = info.Artist?.Trim() ?? string.Empty;
            _localAlbum = info.Album?.Trim() ?? string.Empty;
            _localDurationMs = info.DurationMs;
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
            loaded = true;
        }
        catch (Exception ex)
        {
            StatusText = "读取失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            IsBusy = false;
        }

        if (loaded)
        {
            RefreshSearchScores();
            await AutoSearchOpenedFileAsync(path).ConfigureAwait(true);
        }
    }

    private async Task AutoSearchOpenedFileAsync(string path)
    {
        var keyword = BuildSearchKeywordFromFileName(path);
        if (string.IsNullOrWhiteSpace(keyword))
        {
            return;
        }

        SearchKeyword = keyword;
        await SearchOnlineAsync().ConfigureAwait(true);
    }

    /// <summary>把在线搜索结果按匹配度打分重排；没有打开本地文件时不做打分（没有可比对的本地信息）。</summary>
    private IReadOnlyList<OnlineSearchResultItem> BuildScoredResults(IReadOnlyList<OnlineSongResult> results)
    {
        if (results.Count == 0)
        {
            return Array.Empty<OnlineSearchResultItem>();
        }

        if (!HasFile)
        {
            return results.Select(song => new OnlineSearchResultItem(song, null, belowThreshold: false)).ToList();
        }

        var threshold = MatchThreshold;
        return results
            .Select(song =>
            {
                var score = ScoreCandidate(song);
                return new OnlineSearchResultItem(song, score, score.Total < threshold);
            })
            .OrderByDescending(item => item.Score?.Total ?? 0)
            .ToList();
    }

    private LyricsMatchScore ScoreCandidate(OnlineSongResult song)
    {
        var title = !string.IsNullOrWhiteSpace(_localTitle)
            ? _localTitle
            : HasFile && FilePath is { Length: > 0 } path
                ? BuildSearchKeywordFromFileName(path)
                : SearchKeyword;

        return LyricsMatchScorer.Score(
            title,
            _localArtist,
            _localAlbum,
            _localDurationMs,
            song.Title,
            song.Artist,
            song.Album,
            song.DurationMs);
    }

    /// <summary>换了本地文件后重新给已有搜索结果打分。</summary>
    private void RefreshSearchScores()
    {
        if (SearchResults.Count == 0)
        {
            return;
        }

        // 备用词源结果没有标签信息，只对主词源的歌曲结果重新打分。
        SearchResults = BuildScoredResults(
            SearchResults.Where(item => item.Song is not null).Select(item => item.Song!).ToList());
        SelectedSearchResult = null;
    }

    private static string BuildSearchKeywordFromFileName(string path)
    {
        var name = Path.GetFileNameWithoutExtension(path).Trim();
        if (string.IsNullOrWhiteSpace(name))
        {
            return string.Empty;
        }

        // 去掉常见的 “01 - ” / “01.” 音轨前缀，并把下划线替换为空格。
        name = Regex.Replace(name, @"^\s*\d{1,3}[\s._-]+", string.Empty);
        name = name.Replace('_', ' ');
        return Regex.Replace(name, @"\s+", " ").Trim();
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

    public void Dispose()
    {
        CoverImage?.Dispose();
        // 阈值拖动过程中只改内存，关闭窗口时补一次落盘（失败不影响关闭）。
        if (_settings is not null && _settingsStore is not null)
        {
            _ = PersistThresholdAsync();
        }
    }
}
