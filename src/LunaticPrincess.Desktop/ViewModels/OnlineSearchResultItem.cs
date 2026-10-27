using CommunityToolkit.Mvvm.ComponentModel;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Online;

namespace LunaticPrincess.Desktop.ViewModels;

/// <summary>歌词编辑器在线搜索列表项：搜索结果 + 与本地文件的匹配度。</summary>
public sealed partial class OnlineSearchResultItem : ObservableObject
{
    public OnlineSearchResultItem(OnlineSongResult song, LyricsMatchScore? score, bool belowThreshold)
    {
        Song = song;
        Score = score;
        _belowThreshold = belowThreshold;
    }

    public OnlineSongResult Song { get; }

    public LyricsMatchScore? Score { get; }

    public string DisplayTitle => Song.DisplayTitle;

    public string Subtitle => Song.Subtitle;

    public string DurationText => Song.DurationText;

    public bool HasScore => Score is not null;

    public string MatchText =>
        Score is not { } score
            ? string.Empty
            : BelowThreshold
                ? $"匹配度 {Math.Round(score.Total * 100):0}%（低于阈值）"
                : $"匹配度 {Math.Round(score.Total * 100):0}%";

    public string ScoreToolTip =>
        Score is not { } score
            ? string.Empty
            : $"标题 {Math.Round(score.Title * 100):0}% · 歌手 {Math.Round(score.Artist * 100):0}% · 专辑 {Math.Round(score.Album * 100):0}% · 时长 {Math.Round(score.Duration * 100):0}%";

    [ObservableProperty]
    private bool _belowThreshold;

    public void UpdateThreshold(double threshold) =>
        BelowThreshold = Score is { } score && score.Total < threshold;

    partial void OnBelowThresholdChanged(bool value) => OnPropertyChanged(nameof(MatchText));
}
