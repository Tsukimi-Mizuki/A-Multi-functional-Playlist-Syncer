using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace LunaticPrincess.Core.Ncm;

/// <summary>
/// 解析网易云客户端写进标签注释的 <c>163 key(Don't modify):&lt;base64&gt;</c> 元数据。
/// </summary>
/// <remarks>
/// <para>客户端下载或解密出来的音频不带本程序的 <c>LunaticPrincess:&lt;id&gt;</c> 标记，但注释里保留了这段
/// 加密 JSON（明文为 <c>music:{...}</c>，AES-128-ECB + PKCS7，密钥与 NCM 元数据相同）。
/// 思路来自 MusicTag 桌面端与音樂標籤：从注释还原 musicId，本地曲库对比即可按 ID 精确命中。</para>
/// <para>只读注释，不改写任何标签；格式不符或数据损坏时返回 <see langword="null"/>，不影响其它标签读取。</para>
/// </remarks>
public static class NeteaseClientComment
{
    /// <summary>网易云客户端注释前缀。</summary>
    public const string MetadataPrefix = "163 key(Don't modify):";

    /// <summary>与 NCM 元数据相同的 AES 密钥。</summary>
    internal static readonly byte[] ModifyKey =
    {
        0x23, 0x31, 0x34, 0x6C, 0x6A, 0x6B, 0x5F, 0x21, 0x5C, 0x5D, 0x26, 0x30, 0x55, 0x3C, 0x27, 0x28,
    };

    private const string MusicPrefixText = "music:";

    /// <summary>解密注释中的元数据，返回 <c>music:</c> 之后的 JSON 文本；不是该格式或损坏时返回 null。</summary>
    public static string? TryDecrypt(string? comment)
    {
        if (string.IsNullOrWhiteSpace(comment))
        {
            return null;
        }

        var index = comment.IndexOf(MetadataPrefix, StringComparison.Ordinal);
        if (index < 0)
        {
            return null;
        }

        var payload = comment[(index + MetadataPrefix.Length)..].Trim();
        var length = 0;
        while (length < payload.Length && IsBase64Char(payload[length]))
        {
            length++;
        }

        if (length == 0)
        {
            return null;
        }

        try
        {
            var cipher = Convert.FromBase64String(payload[..length]);
            var plain = AesEcbDecrypt(cipher, ModifyKey);
            var text = Encoding.UTF8.GetString(plain).TrimEnd('\0', ' ', '\r', '\n', '\t');
            if (text.StartsWith('{'))
            {
                return text;
            }

            return text.StartsWith(MusicPrefixText, StringComparison.Ordinal)
                ? text[MusicPrefixText.Length..]
                : null;
        }
        catch (Exception ex) when (ex is FormatException or CryptographicException or ArgumentException)
        {
            return null;
        }
    }

    /// <summary>从注释还原网易云歌曲 ID；注释不带该元数据时返回 null。</summary>
    public static long? TryReadMusicId(string? comment)
    {
        var json = TryDecrypt(comment);
        if (json is null)
        {
            return null;
        }

        try
        {
            using var document = JsonDocument.Parse(json);
            if (document.RootElement.ValueKind != JsonValueKind.Object
                || !document.RootElement.TryGetProperty("musicId", out var element))
            {
                return null;
            }

            if (element.ValueKind == JsonValueKind.Number && element.TryGetInt64(out var numeric))
            {
                return numeric > 0 ? numeric : null;
            }

            return element.ValueKind == JsonValueKind.String
                && long.TryParse(element.GetString(), out var parsed)
                && parsed > 0
                ? parsed
                : null;
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>按客户端格式构造注释内容（加密方向，供测试与迁移使用）。</summary>
    internal static string BuildComment(string metadataJson)
    {
        ArgumentNullException.ThrowIfNull(metadataJson);
        var plain = Encoding.UTF8.GetBytes("music:" + metadataJson);
        var cipher = AesEcbEncrypt(plain, ModifyKey);
        return MetadataPrefix + Convert.ToBase64String(cipher);
    }

    private static bool IsBase64Char(char value) =>
        char.IsLetterOrDigit(value) || value is '+' or '/' or '=';

    private static byte[] AesEcbEncrypt(byte[] plain, byte[] key)
    {
        using var aes = Aes.Create();
        aes.Key = key;
        aes.Mode = CipherMode.ECB;
        aes.Padding = PaddingMode.PKCS7;
        using var encryptor = aes.CreateEncryptor();
        return encryptor.TransformFinalBlock(plain, 0, plain.Length);
    }

    private static byte[] AesEcbDecrypt(byte[] cipher, byte[] key)
    {
        using var aes = Aes.Create();
        aes.Key = key;
        aes.Mode = CipherMode.ECB;
        aes.Padding = PaddingMode.PKCS7;
        using var decryptor = aes.CreateDecryptor();
        return decryptor.TransformFinalBlock(cipher, 0, cipher.Length);
    }
}
