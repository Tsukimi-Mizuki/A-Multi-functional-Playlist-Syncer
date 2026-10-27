using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using Xunit;

namespace LunaticPrincess.Core.Tests.Lyrics;

/// <summary>QQ 音乐 QRC 逐字歌词的解析与 LDDC 风格序列化。</summary>
public sealed class QrcLyricParserTests
{
    private const string SampleQrc = "[11720,3670]看(11720,370)着(12090,310)你(12400,290)\n[15400,2000]微笑(15400,500)的脸(15900,600)";

    private readonly LyricParser _parser = new();

    [Fact]
    public void Parse_Qrc_WordsUseAbsoluteTimestamps()
    {
        var document = _parser.Parse(new RawLyrics(Lrc: null, Qrc: SampleQrc));

        Assert.True(document.HasWordLevel);
        Assert.Equal(2, document.Lines.Count);
        var first = document.Lines[0];
        Assert.Equal(11720, first.StartMs);
        Assert.Equal(3670, first.DurationMs);
        Assert.Equal("看着你", first.Text);
        Assert.True(first.HasWordTiming);
        Assert.Equal(new[] { 11720, 12090, 12400 }, first.Words.Select(word => word.StartMs));
        Assert.Equal(new[] { 370, 310, 290 }, first.Words.Select(word => word.DurationMs));
        Assert.Equal("看", first.Words[0].Text);
        Assert.Equal("你", first.Words[2].Text);
    }

    [Fact]
    public void Parse_QrcEnvelope_UnwrapsAndDecodesEntities()
    {
        const string qrc = "<Lyric_1 LyricType=\"1\" LyricContent=\"[1000,500]Tom &amp; Jerry(1000,500)\"/>";

        var document = _parser.Parse(new RawLyrics(Lrc: null, Qrc: qrc));

        var line = Assert.Single(document.Lines);
        Assert.Equal("Tom & Jerry", line.Text);
        Assert.Equal(1000, line.Words[0].StartMs);
        Assert.Equal(500, line.Words[0].DurationMs);
    }

    [Fact]
    public void Parse_Qrc_SkipsOptionalWordBrackets()
    {
        const string qrc = "[2000,500][2000,200]嘿(2000,200)呀(2200,300)";

        var document = _parser.Parse(new RawLyrics(Lrc: null, Qrc: qrc));

        var line = Assert.Single(document.Lines);
        Assert.Equal("嘿呀", line.Text);
        Assert.Equal(2, line.Words.Count);
        Assert.Equal(2000, line.Words[0].StartMs);
        Assert.Equal(2200, line.Words[1].StartMs);
    }

    [Fact]
    public void Parse_QrcTakesPriorityOverYrcAndLrc()
    {
        var raw = new RawLyrics("[00:99.00]普通LRC", Yrc: "[1000,500]逐字YRC", Qrc: "[2000,300]逐字QRC(2000,300)");

        var document = _parser.Parse(raw);

        var line = Assert.Single(document.Lines);
        Assert.Equal(2000, line.StartMs);
        Assert.Equal("逐字QRC", line.Text);
        Assert.True(line.HasWordTiming);
    }

    [Fact]
    public void Parse_Qrc_MixedPlainLrcLinesFallBack()
    {
        const string qrc = "[ti:标题]\n[00:01.00]plain line\n[2000,500]逐字(2000,250)行(2250,250)";

        var document = _parser.Parse(new RawLyrics(Lrc: null, Qrc: qrc));

        Assert.Equal(2, document.Lines.Count);
        Assert.Equal("plain line", document.Lines[0].Text);
        Assert.False(document.Lines[0].HasWordTiming);
        Assert.Equal("逐字行", document.Lines[1].Text);
        Assert.True(document.Lines[1].HasWordTiming);
    }

    [Fact]
    public void ToEnhancedLrc_QrcWords_EmitsStartAndEndTags()
    {
        var enhanced = _parser.ToEnhancedLrc(_parser.Parse(new RawLyrics(Lrc: null, Qrc: SampleQrc)), includeTranslation: false);

        Assert.Equal(
            string.Join(
                "\n",
                "[00:11.720]<00:11.720>看<00:12.090>着<00:12.400>你<00:12.690>",
                "[00:15.400]<00:15.400>微笑<00:15.900>的脸<00:16.500>"),
            enhanced);
    }

    [Fact]
    public void Parse_QrcWithTranslation_MergesTranslation()
    {
        const string qrc = "[1000,1000]Hello(1000,500) world(1500,500)";

        var document = _parser.Parse(new RawLyrics(Lrc: null, TranslatedLrc: "[00:01.00]你好世界", Qrc: qrc));

        var line = Assert.Single(document.Lines);
        Assert.Equal("Hello world", line.Text);
        Assert.Equal("你好世界", line.Translation);
    }
}
