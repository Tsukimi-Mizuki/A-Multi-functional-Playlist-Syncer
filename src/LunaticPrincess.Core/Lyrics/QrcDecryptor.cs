using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;

namespace LunaticPrincess.Core.Lyrics;

/// <summary>
/// QQ 音乐 QRC 逐字歌词解密器，按 LDDC 的思路实现。
/// </summary>
/// <remarks>
/// <para>云端 QRC 的加密流程：明文（&lt;Lyric_1 LyricType="1" LyricContent="..."/&gt; 或纯文本行）→ zlib 压缩 →
/// 3DES-ECB（24 字节固定密钥、无填充，每 8 字节独立解密）→ 十六进制字符串。</para>
/// <para>解密时反向执行；长度不是 8 的倍数的尾部字节直接丢弃，与 3DES NoPadding 的块语义一致。</para>
/// </remarks>
public static class QrcDecryptor
{
    /// <summary>判断文本是否可能是加密后的 QRC（纯十六进制、长度为 8 的倍数）。</summary>
    public static bool LooksEncrypted(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return false;
        }

        var text = value.Trim();
        if (text.Length < 16 || text.Length % 16 != 0)
        {
            return false;
        }

        foreach (var ch in text)
        {
            if (!char.IsAsciiHexDigit(ch))
            {
                return false;
            }
        }

        return true;
    }

    /// <summary>解密 QRC 十六进制密文；失败返回 <see langword="null"/>。</summary>
    public static string? TryDecrypt(string? encryptedHex)
    {
        if (!LooksEncrypted(encryptedHex))
        {
            return null;
        }

        byte[] cipher;
        try
        {
            cipher = Convert.FromHexString(encryptedHex!.Trim());
        }
        catch (FormatException)
        {
            return null;
        }

        var length = cipher.Length - (cipher.Length % 8);
        if (length <= 0)
        {
            return null;
        }

        byte[] plain;
        try
        {
            // QQ 音乐的 DES 变体无法用 System.Security.Cryptography.TripleDES 直接替代，按位运算实现。
            plain = QqTripleDes.Decrypt(QqTripleDes.QrcKey, cipher, length);
        }
        catch (Exception ex) when (ex is CryptographicException or IndexOutOfRangeException or ArgumentException)
        {
            return null;
        }

        try
        {
            using var source = new MemoryStream(plain);
            using var inflater = new ZLibStream(source, CompressionMode.Decompress);
            using var target = new MemoryStream();
            inflater.CopyTo(target);
            var text = Encoding.UTF8.GetString(target.ToArray()).TrimEnd('\0');
            if (string.IsNullOrWhiteSpace(text))
            {
                return null;
            }

            return text.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n');
        }
        catch (Exception ex) when (ex is InvalidDataException or IOException or NotSupportedException or ArgumentException)
        {
            return null;
        }
    }
}