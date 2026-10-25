using CommunityToolkit.Mvvm.ComponentModel;
using NeteaseMusicManager.Core.Library;
using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Desktop.ViewModels;

/// <summary>歌曲表格中的一行：歌单歌曲 + 本地匹配结果（含勾选状态）。</summary>
public sealed partial class SongRowViewModel : ObservableObject
{
    private readonly Action? _selectionChanged;

    public SongRowViewModel(SongMatch match, int index, Action? selectionChanged = null)
    {
        Match = match;
        Index = index;
        _selectionChanged = selectionChanged;
    }

    public SongMatch Match { get; }
    public NeteaseSong Song => Match.Song;
    public int Index { get; }
    public LocalAvailability Availability => Match.Availability;

    [ObservableProperty]
    private bool _isSelected;

    /// <summary>长按行后显示勾选框；勾选状态下保持可见。</summary>
    [ObservableProperty]
    private bool _showSelectionBox;

    partial void OnIsSelectedChanged(bool value)
    {
        if (value)
        {
            ShowSelectionBox = true;
        }

        _selectionChanged?.Invoke();
    }

    public string Title => string.IsNullOrWhiteSpace(Song.Name) ? "未知歌曲" : Song.Name;

    public bool HasAlias => !string.IsNullOrWhiteSpace(Song.Alias);
    public string AliasText => Song.Alias ?? string.Empty;

    public string ArtistText => string.IsNullOrWhiteSpace(Song.ArtistText) ? "未知歌手" : Song.ArtistText;

    public string AlbumText => string.IsNullOrWhiteSpace(Song.Album.Name) ? "—" : Song.Album.Name;

    public string DurationText
    {
        get
        {
            if (Song.DurationMs <= 0)
            {
                return "--:--";
            }

            var duration = Song.Duration;
            return $"{(int)duration.TotalMinutes}:{duration.Seconds:00}";
        }
    }

    public string LocalStateText => Availability switch
    {
        LocalAvailability.Available => "本地已存在",
        LocalAvailability.EncryptedNcm => "NCM 待解密",
        _ => "需下载",
    };

    public bool HasLocalFile => Match.LocalTrack is not null;

    public string? LocalPath => Match.LocalTrack?.Path;

    public string DetailText
    {
        get
        {
            if (Match.LocalTrack is not { } track)
            {
                return "未在本地曲库中找到";
            }

            var method = Match.Method switch
            {
                MatchMethod.NeteaseId => "标签 ID 匹配",
                MatchMethod.Metadata => "元数据匹配",
                MatchMethod.FileName => "文件名匹配",
                _ => "已匹配",
            };
            return $"{Path.GetFileName(track.Path)} · {method}";
        }
    }

    public string DetailToolTip => Match.LocalTrack?.Path ?? "未在本地曲库中找到匹配文件";

    public string MatchConfidenceText =>
        Match.Method == MatchMethod.None ? string.Empty : $"{Math.Round(Match.Confidence * 100):0}%";
}
