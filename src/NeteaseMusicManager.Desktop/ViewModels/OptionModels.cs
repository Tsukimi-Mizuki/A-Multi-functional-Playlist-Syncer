using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Desktop.ViewModels;

/// <summary>下拉框选项：下载音质。</summary>
public sealed record QualityOption(AudioQuality Value, string Label)
{
    public override string ToString() => Label;
}

/// <summary>下拉框选项：歌词写入方式。</summary>
public sealed record LyricModeOption(LyricsWriteMode Value, string Label, string Description)
{
    public override string ToString() => Label;
}

/// <summary>下拉框选项：下载并发数。</summary>
public sealed record ConcurrencyOption(int Value, string Label)
{
    public override string ToString() => Label;
}
