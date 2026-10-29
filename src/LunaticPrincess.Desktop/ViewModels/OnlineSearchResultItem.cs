using CommunityToolkit.Mvvm.ComponentModel;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Online;

namespace LunaticPrincess.Desktop.ViewModels;

/// <summary>
/// 歌词编辑器在线搜索列表项：主词源（网易云 / QQ 音乐）的歌曲结果，
/// 或备用词源（酷狗音乐 / LRCLIB）的歌词结果（只有歌词，没有标签与封面），
/// 都带有与本地文件的匹配度打分。
/// </summary>
public sealed partial class OnlineSearchResultItem : ObservableObject
{
    public OnlineSearchResultItem(OnlineSongResult song, LyricsMatchScore? score, bool belowThreshold)
    {
        Song = song;
        Score = score;
        _belowThreshold = belowThreshold;
    }

    /// <summary>备用词源结果：只提供歌词。</summary>
    public OnlineSearchResultItem(LyricsSourceCandidate fallback, LyricsMatchScore? score, bool belowThreshold)
    {
        Fallback = fallback;
        Score = score;
        _belowThreshold = belowThreshold;
    }

    /// <summary>主词源结果；备用词源结果为 null。</summary>
    public OnlineSongResult? Song { get; }

    /// <summary>备用词源结果；主词源结果为 null。</summary>
    public LyricsSourceCandidate? Fallback { get; }

    public bool IsFallback => Fallback is not null;

    public LyricsMatchScore? Score { get; }

    public string DisplayTitle => Fallback is { } fallback
        ? string.IsNullOrWhiteSpace(fallback.Title) ? "未知歌曲" : fallback.Title
        : Song!.DisplayTitle;

    public string Subtitle => Fallback is { } fallback
        ? $"{DisplayArtist(fallback.Artist)} · {fallback.SourceLabel}（仅歌词）"
        : Song!.Subtitle;

    public string DurationText => Fallback is { } fallback ? FormatDuration(fallback.DurationMs) : Song!.DurationText;

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

    private static string DisplayArtist(string? artist) =>
        string.IsNullOrWhiteSpace(artist) ? "未知歌手" : artist;

    private static string FormatDuration(long durationMs)
    {
        if (durationMs <= 0)
        {
            return "--:--";
        }

        var duration = TimeSpan.FromMilliseconds(durationMs);
        return $"{(int)duration.TotalMinutes}:{duration.Seconds:00}";
    }
}
