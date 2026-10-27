using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using Xunit;

namespace LunaticPrincess.Core.Tests.Lyrics;

public sealed class LyricParserTests
{
    private readonly LyricParser _parser = new();

    /// <summary>按 \n 拼接期望文本，避免 raw string 随源文件换行符（CRLF/LF）变化而失效。</summary>
    private static string Lines(params string[] lines) => string.Join("\n", lines);

    [Fact]
    public void Parse_PlainLrc_AppliesOffsetAndMultipleTimestamps()
    {
        const string lrc = """
[ti:测试歌曲]
[ar:测试歌手]
[offset:-100]
[00:01.00]第一行
[00:02.50][00:10.00]重复行
[00:03.000]第三行
""";

        var document = _parser.Parse(new RawLyrics(lrc));

        Assert.Equal(4, document.Lines.Count);
        Assert.Equal(new[] { 900, 2400, 2900, 9900 }, document.Lines.Select(line => line.StartMs));
        Assert.Equal(new[] { "第一行", "重复行", "第三行", "重复行" }, document.Lines.Select(line => line.Text));
        Assert.Equal(1500, document.Lines[0].DurationMs);
        Assert.Equal(500, document.Lines[1].DurationMs);
        Assert.Equal(7000, document.Lines[2].DurationMs);
        Assert.False(document.HasWordLevel);

        var plain = _parser.ToPlainLrc(document);
        Assert.Equal(
            Lines(
                "[00:00.900]第一行",
                "[00:02.400]重复行",
                "[00:02.900]第三行",
                "[00:09.900]重复行"),
            plain);
    }

    [Fact]
    public void Parse_PlainLrc_IgnoresMetadataTagsAndParsesFractionVariants()
    {
        const string lrc = """
[ti:标题]
[ar:歌手]
[al:专辑]
[by:制作]
[re:工具]
[00:01.5]A
[00:02.05]B
[00:03.005]C
""";

        var document = _parser.Parse(new RawLyrics(lrc));

        Assert.Equal(3, document.Lines.Count);
        Assert.Equal(new[] { 1500, 2050, 3005 }, document.Lines.Select(line => line.StartMs));
        Assert.Equal(new[] { "A", "B", "C" }, document.Lines.Select(line => line.Text));
    }

    [Fact]
    public void Parse_Translation_MergesByNearestTimestamp()
    {
        const string lrc = """
[00:01.00]Hello
[00:05.00]World
[00:09.00]No translation line
""";
        const string translated = """
[00:05.30]世界
[00:01.05]你好
""";

        var document = _parser.Parse(new RawLyrics(lrc, translated));

        Assert.Equal(3, document.Lines.Count);
        Assert.Equal("你好", document.Lines[0].Translation);
        Assert.Equal("世界", document.Lines[1].Translation);
        Assert.Null(document.Lines[2].Translation);
        Assert.Equal(translated, document.Translation);

        var plain = _parser.ToPlainLrc(document);
        // 时间戳保留到毫秒，译文行与原文行使用同一时间戳，不再人为顺延。
        Assert.Contains("[00:01.000]Hello", plain);
        Assert.Contains("[00:01.000]你好", plain);
        Assert.Contains("[00:05.000]World", plain);
        Assert.Contains("[00:05.000]世界", plain);
        Assert.Contains("[00:09.000]No translation line", plain);
        Assert.DoesNotContain("[00:09.000]世界", plain);
    }

    [Fact]
    public void Parse_EnhancedLrc_ParsesWordTags()
    {
        const string lrc = """
[00:01.00]<00:01.00>Hello<00:02.00> world
[00:05.00]plain line
""";

        var document = _parser.Parse(new RawLyrics(lrc));

        Assert.True(document.HasWordLevel);
        Assert.Equal(2, document.Lines.Count);
        var first = document.Lines[0];
        Assert.Equal(1000, first.StartMs);
        Assert.Equal(4000, first.DurationMs);
        Assert.Equal("Hello world", first.Text);
        Assert.True(first.HasWordTiming);
        Assert.Equal(2, first.Words.Count);
        Assert.Equal(1000, first.Words[0].StartMs);
        Assert.Equal(1000, first.Words[0].DurationMs);
        Assert.Equal(2000, first.Words[1].StartMs);
        Assert.Equal(3000, first.Words[1].DurationMs);
        Assert.Equal(" world", first.Words[1].Text);
        Assert.False(document.Lines[1].HasWordTiming);

        // 与 LDDC 一致：结束标签总是写出，重复的起始标签省略，行尾补行结束标签。
        Assert.Equal(
            Lines(
                "[00:01.000]<00:01.000>Hello<00:02.000> world<00:05.000>",
                "[00:05.000]plain line"),
            _parser.ToEnhancedLrc(document));

        Assert.Equal(
            Lines(
                "[00:01.000]Hello world",
                "[00:05.000]plain line"),
            _parser.ToPlainLrc(document));
    }

    [Fact]
    public void Parse_YrcV0_RealNetEaseSample()
    {
        // 来自 .research/lyric_v0_33894312.json 的真实网易云 YRC 样本（子集）。
        const string yrc = """
[0,1000](0,1000,0) 作词 : 张国祥
[12580,3470](12580,250,0)难(12830,300,0)以(13130,370,0)忘(13500,430,0)记(13930,570,0)初(14500,280,0)次(14780,300,0)见(15080,970,0)你
[16960,3560](16960,240,0)一(17200,350,0)双(17550,470,0)迷(18020,490,0)人(18510,790,0)的(19300,270,0)眼(19570,950,0)睛
""";

        var document = _parser.Parse(new RawLyrics(null, Yrc: yrc));

        Assert.Equal(3, document.Lines.Count);
        Assert.True(document.HasWordLevel);

        var credit = document.Lines[0];
        Assert.Equal(0, credit.StartMs);
        Assert.Equal(1000, credit.DurationMs);
        Assert.Equal(" 作词 : 张国祥", credit.Text);
        Assert.Single(credit.Words);

        var line = document.Lines[1];
        Assert.Equal(12580, line.StartMs);
        Assert.Equal(3470, line.DurationMs);
        Assert.Equal("难以忘记初次见你", line.Text);
        Assert.Equal(8, line.Words.Count);
        Assert.Equal(12580, line.Words[0].StartMs);
        Assert.Equal(250, line.Words[0].DurationMs);
        Assert.Equal("难", line.Words[0].Text);
        Assert.Equal("你", line.Words[^1].Text);
        Assert.Equal(15080, line.Words[^1].StartMs);
        Assert.Equal(970, line.Words[^1].DurationMs);

        Assert.Equal(16960, document.Lines[2].StartMs);
        Assert.Equal("一双迷人的眼睛", document.Lines[2].Text);

        var plain = _parser.ToPlainLrc(document);
        Assert.Contains("[00:00.000]作词 : 张国祥", plain);
        Assert.Contains("[00:12.580]难以忘记初次见你", plain);
        Assert.Contains("[00:16.960]一双迷人的眼睛", plain);

        var enhanced = _parser.ToEnhancedLrc(document);
        Assert.Contains("[00:12.580]<00:12.580>难<00:12.830>以<00:13.130>忘", enhanced);
        Assert.Contains("<00:15.080>你", enhanced);
        Assert.Contains("[00:00.000]<00:00.000> 作词 : 张国祥", enhanced);
    }

    [Fact]
    public void Parse_Yrc_RelativeWordTimestampsAreNormalized()
    {
        const string yrc = "[10000,2000](0,500,0)甲(500,500,0)乙";

        var document = _parser.Parse(new RawLyrics(null, Yrc: yrc));

        var line = Assert.Single(document.Lines);
        Assert.Equal(10000, line.StartMs);
        Assert.Equal("甲乙", line.Text);
        Assert.Equal(2, line.Words.Count);
        Assert.Equal(10000, line.Words[0].StartMs);
        Assert.Equal(10500, line.Words[1].StartMs);
        Assert.Equal(500, line.Words[0].DurationMs);
        Assert.Equal(500, line.Words[1].DurationMs);
    }

    [Fact]
    public void Parse_JsonFlavorYrc_SupportsMixedLines()
    {
        // 来自 .research/lyric_v1_33894312.json：JSON 风格行 + v0 风格行混合。
        const string yrc = """
{"t":0,"c":[{"tx":"作词: "},{"tx":"张国祥"}]}
{"t":1000,"c":[{"tx":"作曲: 汤小康"}]}
[12580,3470](12580,250,0)难
""";

        var document = _parser.Parse(new RawLyrics(null, Yrc: yrc));

        Assert.Equal(3, document.Lines.Count);
        Assert.Equal(0, document.Lines[0].StartMs);
        Assert.Equal("作词: 张国祥", document.Lines[0].Text);
        Assert.Equal(1000, document.Lines[0].DurationMs);
        Assert.Equal(1000, document.Lines[1].StartMs);
        Assert.Equal("作曲: 汤小康", document.Lines[1].Text);
        Assert.Equal(12580, document.Lines[2].StartMs);
        Assert.Single(document.Lines[2].Words);

        Assert.Equal(
            Lines(
                "[00:00.000]作词: 张国祥",
                "[00:01.000]作曲: 汤小康",
                "[00:12.580]难"),
            _parser.ToPlainLrc(document));
    }

    [Fact]
    public void Parse_YrcWithoutWordTags_ProducesLineLevelLyrics()
    {
        var document = _parser.Parse(new RawLyrics(null, Yrc: "[1000,500]纯文本"));

        var line = Assert.Single(document.Lines);
        Assert.Equal(1000, line.StartMs);
        Assert.Equal(500, line.DurationMs);
        Assert.Equal("纯文本", line.Text);
        Assert.False(line.HasWordTiming);
        Assert.False(document.HasWordLevel);
    }

    [Fact]
    public void Parse_YrcTakesPriorityOverLrc()
    {
        var raw = new RawLyrics("[00:99.00]不应该出现", Yrc: "[1000,500]逐字行");

        var document = _parser.Parse(raw);

        var line = Assert.Single(document.Lines);
        Assert.Equal(1000, line.StartMs);
        Assert.Equal("逐字行", line.Text);
    }

    [Fact]
    public void Parse_YrcWithTranslation_MergesAndSerializes()
    {
        const string yrc = "[12580,3470](12580,250,0)难(12830,300,0)以";
        const string translated = "[00:12.58]Hard to";

        var document = _parser.Parse(new RawLyrics(null, translated, Yrc: yrc));

        var line = Assert.Single(document.Lines);
        Assert.Equal("Hard to", line.Translation);

        var enhanced = _parser.ToEnhancedLrc(document);
        Assert.Equal(
            Lines(
                "[00:12.580]<00:12.580>难<00:12.830>以<00:13.130>",
                "[00:12.580]Hard to"),
            enhanced);
    }

    [Fact]
    public void ToEnhancedLrc_FallsBackToPlainWhenNoWordLevel()
    {
        var document = _parser.Parse(new RawLyrics("[00:01.00]只有行时间\n[00:02.00]第二行"));

        Assert.Equal(_parser.ToPlainLrc(document), _parser.ToEnhancedLrc(document));
    }

    [Fact]
    public void ToPlainLrc_SupportsBilingualAndIgnoresSlashPlaceholders()
    {
        const string lrc = """
[00:01.00]line one
[00:02.00]line two
""";
        const string translated = """
[00:01.00]翻译一
[00:02.00]//
""";

        var document = _parser.Parse(new RawLyrics(lrc, translated));

        var bilingual = _parser.ToPlainLrc(document, includeTranslation: true);
        var original = _parser.ToPlainLrc(document, includeTranslation: false);

        Assert.Contains("翻译一", bilingual);
        Assert.DoesNotContain("翻译一", original);
        Assert.DoesNotContain("//", bilingual);
        Assert.Contains("[00:02.000]line two", bilingual);
        Assert.DoesNotContain("[00:02.000]//", bilingual);
    }

    [Fact]
    public void Parse_EmptyInput_ReturnsEmptyDocument()
    {
        var document = _parser.Parse(new RawLyrics(null));

        Assert.True(document.IsEmpty);
        Assert.False(document.HasWordLevel);
        Assert.Empty(document.Lines);
        Assert.Null(document.Translation);
        Assert.Null(document.Romanization);
        Assert.Equal(string.Empty, _parser.ToPlainLrc(document));
        Assert.Equal(string.Empty, _parser.ToEnhancedLrc(document));
    }

    [Fact]
    public void Parse_Romanization_IsExposedOnDocument()
    {
        const string romanized = "[00:01.00]roman line";

        var document = _parser.Parse(new RawLyrics("[00:01.00]原文", RomanizedLrc: romanized));

        Assert.Equal(romanized, document.Romanization);
        Assert.Single(document.Lines);
    }

    [Fact]
    public void Parse_LrcWithJsonFlavorCreditLines_ParsesThem()
    {
        const string lrc = """
{"t":0,"c":[{"tx":"作词: "},{"tx":"张国祥"}]}
[00:12.57]难以忘记初次见你
""";

        var document = _parser.Parse(new RawLyrics(lrc));

        Assert.Equal(2, document.Lines.Count);
        Assert.Equal("作词: 张国祥", document.Lines[0].Text);
        Assert.Equal(0, document.Lines[0].StartMs);
        Assert.Equal(12570, document.Lines[1].StartMs);
        Assert.Equal("难以忘记初次见你", document.Lines[1].Text);
    }

    [Fact]
    public void Parse_WordsWithRelativeTimesBelowLineStart_GetShifted()
    {
        // 增强 LRC 的词标签若小于行起始时间，按相对时间处理。
        const string lrc = "[00:10.00]<00:00.50>甲<00:01.00>乙";

        var document = _parser.Parse(new RawLyrics(lrc));

        var line = Assert.Single(document.Lines);
        Assert.Equal(10000, line.StartMs);
        Assert.Equal(10500, line.Words[0].StartMs);
        Assert.Equal(11000, line.Words[1].StartMs);
    }
}
