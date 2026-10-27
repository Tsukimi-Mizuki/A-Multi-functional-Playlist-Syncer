using Avalonia.Media.Imaging;
using CommunityToolkit.Mvvm.ComponentModel;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Desktop.Services;

namespace LunaticPrincess.Desktop.ViewModels;

/// <summary>左侧歌单列表项（封面异步加载，失败保持占位图）。</summary>
public sealed partial class PlaylistItemViewModel : ObservableObject
{
    private readonly PlaylistSummary _summary;
    private readonly CoverImageService _covers;

    public PlaylistItemViewModel(PlaylistSummary summary, CoverImageService covers)
    {
        _summary = summary;
        _covers = covers;
    }

    public long Id => _summary.Id;
    public string Name => string.IsNullOrWhiteSpace(_summary.Name) ? "未命名歌单" : _summary.Name;
    public int TrackCount => _summary.TrackCount;
    public string CountText => $"{_summary.TrackCount} 首";
    public string? CreatorText => _summary.CreatorNickname;

    [ObservableProperty]
    private Bitmap? _cover;

    public async Task LoadCoverAsync()
    {
        if (Cover is not null)
        {
            return;
        }

        try
        {
            Cover = await _covers.LoadAsync(_summary.CoverUrl);
        }
        catch (Exception)
        {
            // 封面加载失败不影响列表展示。
        }
    }
}
