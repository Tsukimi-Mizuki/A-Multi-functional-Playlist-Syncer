using System.Text;
using LunaticPrincess.Core.Lyrics;
using Xunit;

namespace LunaticPrincess.Core.Tests.Lyrics;

/// <summary>酷狗 KRC 解密与解析：魔数 + XOR + zlib 往返、逐字时间轴、逐行译文与元数据行过滤。</summary>
public sealed class KrcLyricsParserTests
{
    [Fact]
    public void DecryptBase64_RoundTripsKrcText()
    {
        var text = BuildSample();

        Assert.Equal(text, KrcLyricsParser.DecryptBase64(KrcFixture.EncryptToBase64(text)));
    }

    [Fact]
    public void DecryptBase64_HandlesLegacyFormatWithoutMagic()
    {
        var text = BuildSample();

        Assert.Equal(text, KrcLyricsParser.DecryptBase64(KrcFixture.EncryptToBase64(text, withMagic: false)));
    }

    [Fact]
    public void DecryptBase64_ReturnsNullForInvalidInput()
    {
        Assert.Null(KrcLyricsParser.DecryptBase64(null));
        Assert.Null(KrcLyricsParser.DecryptBase64("   "));
        Assert.Null(KrcLyricsParser.DecryptBase64("not-base64!!"));
        Assert.Null(KrcLyricsParser.DecryptBase64(Convert.ToBase64String(new byte[32])));
    }

    [Fact]
    public void Parse_BuildsWordTimingAndTranslations()
    {
        var document = KrcLyricsParser.Parse(BuildSample());

        Assert.False(document.IsEmpty);
        Assert.True(document.HasWordLevel);
        Assert.Equal(2, document.Lines.Count);

        var first = document.Lines[0];
        Assert.Equal(1000, first.StartMs);
        Assert.Equal(2000, first.DurationMs);
        Assert.Equal("Hello world!", first.Text);
        Assert.Equal("你好", first.Translation);
        Assert.Equal(3, first.Words.Count);
        Assert.Equal(1000, first.Words[0].StartMs);
        Assert.Equal(500, first.Words[0].DurationMs);
        Assert.Equal("Hello ", first.Words[0].Text);
        Assert.Equal(1500, first.Words[1].StartMs);
        Assert.Equal("world", first.Words[1].Text);
        Assert.Equal(2000, first.Words[2].StartMs);
        Assert.Equal("!", first.Words[2].Text);

        var second = document.Lines[1];
        Assert.Equal(3000, second.StartMs);
        Assert.Equal("第二行", second.Text);
        Assert.Equal("第二行译", second.Translation);
    }

    [Fact]
    public void Parse_SkipsMetadataAndBlankLines()
    {
        var text = string.Join("\n",
            "[ti:标题]",
            "[ar:歌手]",
            "[id:$00000000]",
            "[offset:0]",
            string.Empty,
            "[0,1000]<0,400,0>只<400,600,0>有一行");

        var document = KrcLyricsParser.Parse(text);

        Assert.Single(document.Lines);
        Assert.Equal("只有一行", document.Lines[0].Text);
        Assert.Null(document.Lines[0].Translation);
    }

    [Fact]
    public void Parse_ReturnsEmptyDocumentForBlankText()
    {
        Assert.True(KrcLyricsParser.Parse(null).IsEmpty);
        Assert.True(KrcLyricsParser.Parse("   ").IsEmpty);
        Assert.True(KrcLyricsParser.Parse("[ti:只有元数据]").IsEmpty);
    }

    private static string BuildSample()
    {
        var language = KrcFixture.LanguageHeader("你好 ", "第二行译");
        return string.Join("\n",
            "[ti:テスト]",
            "[ar:歌手]",
            "[id:$00000000]",
            language,
            "[1000,2000]<0,500,0>Hello <500,500,0>world<1000,1000,0>!",
            "[3000,1500]<0,750,0>第二<750,750,0>行");
    }
}
