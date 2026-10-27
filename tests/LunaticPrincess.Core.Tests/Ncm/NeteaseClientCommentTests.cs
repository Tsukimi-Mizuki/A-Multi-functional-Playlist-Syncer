using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Tags;
using LunaticPrincess.Core.Tests.Tags;
using Xunit;

namespace LunaticPrincess.Core.Tests.Ncm;

/// <summary>网易云客户端注释里的“163 key”元数据：解密、取 musicId、以及标签读取时的回落。</summary>
public sealed class NeteaseClientCommentTests : IDisposable
{
    private const string MetadataJson =
        """{"musicId":33894312,"musicName":"晴天","artist":[["周杰伦",6452]],"album":"叶惠美","format":"mp3"}""";

    private readonly TempDirectory _temp = new("netease-comment");

    public void Dispose() => _temp.Dispose();

    [Fact]
    public void TryDecrypt_RoundTripsClientPayload()
    {
        var comment = NeteaseClientComment.BuildComment(MetadataJson);

        Assert.StartsWith(NeteaseClientComment.MetadataPrefix, comment, StringComparison.Ordinal);
        Assert.Equal(MetadataJson, NeteaseClientComment.TryDecrypt(comment));
        Assert.Equal(33894312L, NeteaseClientComment.TryReadMusicId(comment));
    }

    [Fact]
    public void TryReadMusicId_ToleratesSurroundingComment()
    {
        var comment = "自定义备注; " + NeteaseClientComment.BuildComment(MetadataJson) + " ; id3v2";

        Assert.Equal(33894312L, NeteaseClientComment.TryReadMusicId(comment));
    }

    [Fact]
    public void TryReadMusicId_ReadsStringEncodedId()
    {
        var comment = NeteaseClientComment.BuildComment("""{"musicId":"123456"}""");

        Assert.Equal(123456L, NeteaseClientComment.TryReadMusicId(comment));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("普通备注")]
    [InlineData("LunaticPrincess:186016")]
    [InlineData("163 key(Don't modify):")]
    [InlineData("163 key(Don't modify):not-base64!!")]
    [InlineData("163 key(Don't modify):AAAA")]
    public void TryReadMusicId_IgnoresForeignComments(string? comment)
    {
        Assert.Null(NeteaseClientComment.TryReadMusicId(comment));
    }

    [Fact]
    public async Task ReadAsync_RecoversNeteaseIdFromClientComment()
    {
        var path = _temp.File("client.mp3");
        File.WriteAllBytes(path, Mp3Fixture.BuildMinimalMp3());
        var service = new AudioTagService();
        await service.WriteAsync(path, new AudioTagData
        {
            Title = "晴天",
            Comment = NeteaseClientComment.BuildComment(MetadataJson),
            ReplaceAll = true,
        });

        var info = await service.ReadAsync(path);

        Assert.NotNull(info);
        Assert.Equal(33894312L, info!.NeteaseId);
    }
}
