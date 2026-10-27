using System.IO.Compression;
using System.Text;
using LunaticPrincess.Core.Lyrics;
using Xunit;

namespace LunaticPrincess.Core.Tests.Lyrics;

/// <summary>QRC 解密器：3DES-ECB（无填充）→ zlib 的解密链路与输入校验。</summary>
public sealed class QrcDecryptorTests
{
    [Theory]
    [InlineData("0123456789abcdef")]
    [InlineData("0123456789abcdef0123456789abcdef")]
    public void LooksEncrypted_AcceptsCipherLikeHex(string value)
    {
        Assert.True(QrcDecryptor.LooksEncrypted(value));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("hello world")]
    [InlineData("[00:12.34]plain lrc line")]
    [InlineData("0123456789abc")]
    [InlineData("0123456789abcdeZ")]
    public void LooksEncrypted_RejectsNonCipherInput(string? value)
    {
        Assert.False(QrcDecryptor.LooksEncrypted(value));
    }

    [Fact]
    public void TryDecrypt_RoundTripsTripleDesPlusZlib()
    {
        const string plain = "<Lyric_1 LyricType=\"1\" LyricContent=\"[0,500]Hello(0,250) world(250,250)\n[500,400]Second(500,400)\"/>";

        var decrypted = QrcDecryptor.TryDecrypt(Encrypt(plain));

        Assert.Equal(plain, decrypted);
    }

    [Fact]
    public void TryDecrypt_NormalizesLineEndingsAndDropsTrailingNul()
    {
        var plain = "[0,500]line one\r\n[500,400]line two\0";

        var decrypted = QrcDecryptor.TryDecrypt(Encrypt(plain));

        Assert.Equal("[0,500]line one\n[500,400]line two", decrypted);
    }

    [Fact]
    public void EncryptBlock_MatchesQqDesVariantVector()
    {
        // 该向量取自 LDDC 的 tripledes 实现：与教科书 DES 的字节/位约定不同，锁定变体行为。
        var cipher = QqTripleDes.EncryptBlock(
            Convert.FromHexString("133457799BBCDFF1"),
            Convert.FromHexString("0123456789ABCDEF"));

        Assert.Equal("F63A972CE52AA327", Convert.ToHexString(cipher));
    }

    [Fact]
    public void TryDecrypt_ReturnsNullForGarbageCipher()
    {
        Assert.Null(QrcDecryptor.TryDecrypt(new string('0', 32)));
    }

    [Fact]
    public void TryDecrypt_ReturnsNullForPlainLrc()
    {
        Assert.Null(QrcDecryptor.TryDecrypt("[00:01.00]not encrypted"));
    }

    private static string Encrypt(string plain)
    {
        using var compressed = new MemoryStream();
        using (var zlib = new ZLibStream(compressed, CompressionLevel.Optimal, leaveOpen: true))
        {
            var raw = Encoding.UTF8.GetBytes(plain);
            zlib.Write(raw, 0, raw.Length);
        }

        var cipher = compressed.ToArray();
        var padded = new byte[(cipher.Length + 7) / 8 * 8];
        cipher.CopyTo(padded, 0);

        return Convert.ToHexString(QqTripleDes.Encrypt(QqTripleDes.QrcKey, padded, padded.Length)).ToLowerInvariant();
    }
}