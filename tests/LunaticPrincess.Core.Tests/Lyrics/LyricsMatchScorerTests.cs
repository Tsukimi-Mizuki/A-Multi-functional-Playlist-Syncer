using LunaticPrincess.Core.Lyrics;
using Xunit;

namespace LunaticPrincess.Core.Tests.Lyrics;

/// <summary>在线匹配打分：多变体关键词、四路加权、版本降权与伴奏剔除（音樂標籤 思路）。</summary>
public sealed class LyricsMatchScorerTests
{
    [Fact]
    public void BuildQueries_PrefersTitleArtistAndAddsVariants()
    {
        var queries = LyricsMatchScorer.BuildQueries("Lemon (Live)", "米津玄師");

        // 先是“标题 歌手”，再是纯标题、反序，最后退化成去掉括号的标题主体。
        Assert.Equal("Lemon (Live) 米津玄師", queries[0]);
        Assert.Equal("Lemon (Live)", queries[1]);
        Assert.Contains("米津玄師 Lemon (Live)", queries);
        Assert.Contains("Lemon 米津玄師", queries);
        Assert.Equal(queries.Count, queries.Distinct(StringComparer.OrdinalIgnoreCase).Count());
    }

    [Fact]
    public void BuildQueries_WithoutArtistStillReturnsTitleVariants()
    {
        var queries = LyricsMatchScorer.BuildQueries("晴天（钢琴版）", null);

        Assert.Equal("晴天（钢琴版）", queries[0]);
        Assert.Contains("晴天", queries);
    }

    [Fact]
    public void Score_ExactMatch_ScoresOneAndIsConfident()
    {
        var score = LyricsMatchScorer.Score(
            "晴天", "周杰伦", "叶惠美", 269_000,
            "晴天", "周杰伦", "叶惠美", 269_000);

        Assert.False(score.Rejected);
        Assert.True(score.Total > 0.95, score.Total.ToString("0.###"));
        Assert.True(score.IsConfident);
    }

    [Fact]
    public void Score_LiveVersion_IsPenalizedButStillMatched()
    {
        var exact = LyricsMatchScorer.Score("Lemon", "米津玄師", null, 255_000, "Lemon", "米津玄師", null, 255_000);
        var live = LyricsMatchScorer.Score("Lemon", "米津玄師", null, 255_000, "Lemon (Live)", "米津玄師", null, 255_000);

        Assert.False(live.Rejected);
        Assert.True(live.Total < exact.Total, $"{live.Total} < {exact.Total}");
        Assert.True(live.Title > LyricsMatchScorer.PoorTitleScore, live.Title.ToString("0.###"));
    }

    [Fact]
    public void Score_SameTitleButDifferentArtist_FallsBelowDefaultThreshold()
    {
        // 真实案例：本地 “レッド・パージ！！！” 的歌手是 +α、あるふぁきゅん。，QQ 上同名歌曲属于 Reol。
        var score = LyricsMatchScorer.Score(
            "レッド・パージ！！！", "+α、あるふぁきゅん。", null, 220_000,
            "レッド・パージ！！！", "Reol", null, 220_000);

        Assert.True(score.Title > 0.95);
        Assert.True(score.Artist < 0.10);
        Assert.True(score.Total < LyricsMatchScorer.DefaultMinScore, score.Total.ToString("0.###"));
    }

    [Fact]
    public void Score_InstrumentalCandidate_IsRejected()
    {
        var score = LyricsMatchScorer.Score(
            "晴天", "周杰伦", null, 269_000,
            "晴天 (伴奏)", "周杰伦", null, 269_000);

        Assert.True(score.Rejected);
        Assert.False(string.IsNullOrWhiteSpace(score.RejectReason));
    }

    [Fact]
    public void Score_MultiArtistLocal_MatchesSingleArtistCandidate()
    {
        var score = LyricsMatchScorer.Score(
            "晴天", "周杰伦/杨瑞代", null, 269_000,
            "晴天", "周杰伦", null, 269_500);

        Assert.False(score.Rejected);
        Assert.True(score.Artist >= 0.9, score.Artist.ToString("0.###"));
        Assert.True(score.Total >= LyricsMatchScorer.DefaultMinScore, score.Total.ToString("0.###"));
    }

    [Fact]
    public void Score_UnrelatedSong_IsBelowDefaultThreshold()
    {
        var score = LyricsMatchScorer.Score(
            "完全无关的歌曲", "陌生歌手", null, 200_000,
            "晴天", "周杰伦", null, 269_500);

        Assert.False(score.Rejected);
        Assert.True(score.Total < LyricsMatchScorer.DefaultMinScore, score.Total.ToString("0.###"));
    }

    [Theory]
    [InlineData(1_000, 1.0)]
    [InlineData(4_000, 0.90)]
    [InlineData(8_000, 0.75)]
    [InlineData(15_000, 0.45)]
    [InlineData(35_000, 0.15)]
    [InlineData(120_000, 0.0)]
    public void DurationScore_UsesToleranceBuckets(int deltaMs, double expected)
    {
        Assert.Equal(expected, LyricsMatchScorer.DurationScore(200_000, 200_000 + deltaMs), 3);
        Assert.Equal(expected, LyricsMatchScorer.DurationScore(200_000 + deltaMs, 200_000), 3);
    }

    [Fact]
    public void DurationScore_UnknownDuration_ReturnsNeutralHalf()
    {
        Assert.Equal(0.5, LyricsMatchScorer.DurationScore(0, 200_000), 3);
        Assert.Equal(0.5, LyricsMatchScorer.DurationScore(200_000, 0), 3);
    }

    [Fact]
    public void NormalizeText_IgnoringWidthCaseAndPunctuation()
    {
        Assert.Equal(
            LyricsMatchScorer.NormalizeText("Ｌｅｍｏｎ   -   Lemon"),
            LyricsMatchScorer.NormalizeText("lemon lemon"));
    }
}
