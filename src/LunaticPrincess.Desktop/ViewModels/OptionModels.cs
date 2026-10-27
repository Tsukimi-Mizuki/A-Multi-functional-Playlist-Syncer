using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Online;

namespace LunaticPrincess.Desktop.ViewModels;

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

/// <summary>文件名模板预设。</summary>
public sealed record FileNameTemplateOption(string Value, string Label)
{
    public static FileNameTemplateOption Custom { get; } = new(string.Empty, "自定义");

    public override string ToString() => Label;
}

/// <summary>界面帧率上限选项。</summary>
public sealed record RenderFrameRateOption(RenderFrameRateMode Value, string Label)
{
    public override string ToString() => Label;
}

/// <summary>在线搜索音乐源选项。</summary>
public sealed record OnlineSourceOption(OnlineMusicSource Value, string Label)
{
    public override string ToString() => Label;
}

/// <summary>在线搜索歌词格式选项。</summary>
public sealed record OnlineLyricModeOption(bool PreferWord, string Label)
{
    public override string ToString() => Label;
}

/// <summary>在线来源优先级选项。</summary>
public sealed record OnlinePriorityOption(OnlineSourcePriority Value, string Label)
{
    public override string ToString() => Label;
}

