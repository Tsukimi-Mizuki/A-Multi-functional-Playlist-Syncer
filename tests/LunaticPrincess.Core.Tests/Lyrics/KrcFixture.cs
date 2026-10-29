using System.IO.Compression;
using System.Text;

namespace LunaticPrincess.Core.Tests.Lyrics;

/// <summary>按酷狗 KRC 的格式逆算法构造测试数据（明文 krc1 魔数 + XOR 混淆的 zlib 流）。</summary>
internal static class KrcFixture
{
    internal static readonly byte[] XorKey =
    [
        0x40, 0x47, 0x61, 0x77, 0x5E, 0x32, 0x74, 0x47,
        0x51, 0x36, 0x31, 0x2D, 0xCE, 0xD2, 0x6E, 0x69,
    ];

    /// <summary>把 KRC 明文加密成接口里的 base64 内容。</summary>
    internal static string EncryptToBase64(string text, bool withMagic = true)
    {
        var plain = Encoding.UTF8.GetBytes(text);
        using var buffer = new MemoryStream();
        using (var zlib = new ZLibStream(buffer, CompressionLevel.Optimal, leaveOpen: true))
        {
            zlib.Write(plain);
        }

        var compressed = buffer.ToArray();
        var magic = withMagic ? "krc1"u8.ToArray() : Array.Empty<byte>();
        var encrypted = new byte[magic.Length + compressed.Length];
        magic.CopyTo(encrypted, 0);
        for (var i = 0; i < compressed.Length; i++)
        {
            encrypted[magic.Length + i] = (byte)(compressed[i] ^ XorKey[i % XorKey.Length]);
        }

        return Convert.ToBase64String(encrypted);
    }

    /// <summary>生成 <c>[language:…]</c> 头（type=1 的逐行译文）。</summary>
    internal static string LanguageHeader(params string[] translations)
    {
        var lines = string.Join(",", translations.Select(text => "[\"" + text + "\"]"));
        var json = "{\"content\":[{\"type\":1,\"lyricContent\":[" + lines + "]}]}";
        return "[language:" + Convert.ToBase64String(Encoding.UTF8.GetBytes(json)) + "]";
    }
}
