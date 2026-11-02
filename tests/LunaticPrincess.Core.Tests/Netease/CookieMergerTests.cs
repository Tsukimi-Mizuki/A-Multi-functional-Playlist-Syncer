using LunaticPrincess.Core.Netease;
using Xunit;

namespace LunaticPrincess.Core.Tests.Netease;

public sealed class CookieMergerTests
{
    private static readonly DateTimeOffset Now = new(2026, 11, 2, 12, 0, 0, TimeSpan.Zero);

    [Fact]
    public void Merge_OverwritesExistingValueAndKeepsOrder()
    {
        var result = CookieMerger.Merge(
            "os=pc; MUSIC_U=old; __csrf=c1",
            ["MUSIC_U=new; Path=/; HttpOnly"],
            Now);

        Assert.True(result.Changed);
        Assert.Equal("os=pc; MUSIC_U=new; __csrf=c1", result.Cookie);
    }

    [Fact]
    public void Merge_AppendsNewCookies()
    {
        var result = CookieMerger.Merge("MUSIC_U=a", ["NMTID=xyz; Max-Age=3600; Path=/"], Now);

        Assert.True(result.Changed);
        Assert.Equal("MUSIC_U=a; NMTID=xyz", result.Cookie);
    }

    [Fact]
    public void Merge_NameMatchIgnoresCase()
    {
        var result = CookieMerger.Merge("MUSIC_U=a", ["music_u=b"], Now);

        Assert.True(result.Changed);
        Assert.Equal("MUSIC_U=b", result.Cookie);
    }

    [Fact]
    public void Merge_NoChange_ReportsUnchangedAndKeepsOriginalText()
    {
        const string existing = "MUSIC_U=a;  __csrf=c1";
        var result = CookieMerger.Merge(existing, ["MUSIC_U=a; Path=/", "__csrf=c1"], Now);

        Assert.False(result.Changed);
        Assert.Equal(existing, result.Cookie);
    }

    [Fact]
    public void Merge_EmptyOrNullHeaders_AreUnchanged()
    {
        Assert.False(CookieMerger.Merge("MUSIC_U=a", [], Now).Changed);
        Assert.False(CookieMerger.Merge("MUSIC_U=a", null, Now).Changed);
        Assert.False(CookieMerger.Merge("MUSIC_U=a", ["", "garbage", "=novalue"], Now).Changed);
    }

    [Fact]
    public void Merge_EmptyValue_DoesNotOverwriteOrAdd()
    {
        var result = CookieMerger.Merge("MUSIC_U=a", ["MUSIC_U=; Path=/", "NEW=; Path=/"], Now);

        Assert.False(result.Changed);
        Assert.Equal("MUSIC_U=a", result.Cookie);
    }

    [Fact]
    public void Merge_MaxAgeZeroOrNegative_DeletesCookie()
    {
        var zero = CookieMerger.Merge("MUSIC_U=a; os=pc", ["os=pc; Max-Age=0"], Now);
        Assert.True(zero.Changed);
        Assert.Equal("MUSIC_U=a", zero.Cookie);

        var negative = CookieMerger.Merge("MUSIC_U=a; os=pc", ["MUSIC_U=; Max-Age=-1; Path=/"], Now);
        Assert.True(negative.Changed);
        Assert.Equal("os=pc", negative.Cookie);
    }

    [Fact]
    public void Merge_ExpiredExpires_DeletesCookie()
    {
        var result = CookieMerger.Merge(
            "MUSIC_U=a; os=pc",
            ["os=pc; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/"],
            Now);

        Assert.True(result.Changed);
        Assert.Equal("MUSIC_U=a", result.Cookie);
    }

    [Fact]
    public void Merge_ExpiredDashedExpires_DeletesCookie()
    {
        // 网易云等 Java 服务端常用 "dd-MMM-yyyy" 形式的 Expires。
        var result = CookieMerger.Merge(
            "MUSIC_U=a; os=pc",
            ["os=pc; expires=Thu, 01-Jan-1970 00:00:10 GMT; path=/"],
            Now);

        Assert.True(result.Changed);
        Assert.Equal("MUSIC_U=a", result.Cookie);
    }

    [Fact]
    public void Merge_FutureExpires_UpdatesCookie()
    {
        var result = CookieMerger.Merge(
            "MUSIC_U=a",
            ["MUSIC_U=b; Expires=Fri, 01 Jan 2100 00:00:00 GMT; Path=/"],
            Now);

        Assert.True(result.Changed);
        Assert.Equal("MUSIC_U=b", result.Cookie);
    }

    [Fact]
    public void Merge_MaxAgeTakesPrecedenceOverExpires()
    {
        var result = CookieMerger.Merge(
            "MUSIC_U=a",
            ["MUSIC_U=b; Max-Age=600; Expires=Thu, 01 Jan 1970 00:00:00 GMT"],
            Now);

        Assert.True(result.Changed);
        Assert.Equal("MUSIC_U=b", result.Cookie);
    }

    [Fact]
    public void Merge_DeletingMissingCookie_IsUnchanged()
    {
        var result = CookieMerger.Merge("MUSIC_U=a", ["gone=1; Max-Age=0"], Now);

        Assert.False(result.Changed);
    }

    [Fact]
    public void Merge_LaterHeaderWinsWithinOneResponse()
    {
        var result = CookieMerger.Merge("MUSIC_U=a", ["MUSIC_U=b", "MUSIC_U=c"], Now);

        Assert.Equal("MUSIC_U=c", result.Cookie);
    }

    [Fact]
    public void Merge_EmptyExisting_BuildsFromHeaders()
    {
        var result = CookieMerger.Merge(null, ["MUSIC_U=a; Path=/", "__csrf=c"], Now);

        Assert.True(result.Changed);
        Assert.Equal("MUSIC_U=a; __csrf=c", result.Cookie);
    }
}
