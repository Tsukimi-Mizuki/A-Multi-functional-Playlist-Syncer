using CommunityToolkit.Mvvm.ComponentModel;
using LunaticPrincess.Core.Lyrics;

namespace LunaticPrincess.Desktop.ViewModels;

/// <summary>批量标签/封面结果列表中的一行。</summary>
public sealed partial class BatchLyricsItemViewModel : ObservableObject
{
    public BatchLyricsItemViewModel(string filePath)
    {
        FilePath = filePath;
        FileName = Path.GetFileName(filePath);
    }

    public string FilePath { get; }
    public string FileName { get; }

    [ObservableProperty]
    private string _matchedText = string.Empty;

    [ObservableProperty]
    private string _sourceText = string.Empty;

    [ObservableProperty]
    private string _formatText = string.Empty;

    [ObservableProperty]
    private string _statusText = "等待处理";

    [ObservableProperty]
    private string _statusColor = "#96A1B0";

    public void Apply(BatchLyricsResult result)
    {
        MatchedText = string.IsNullOrWhiteSpace(result.MatchedArtist)
            ? result.MatchedTitle ?? string.Empty
            : $"{result.MatchedTitle} - {result.MatchedArtist}";
        SourceText = result.SourceLabel ?? string.Empty;
        FormatText = result.LyricsFormat ?? string.Empty;
        StatusText = result.Message;
        StatusColor = result.Success
            ? "#5BDB95"
            : result.Skipped
                ? "#96A1B0"
                : "#FF7D7D";
    }
}
