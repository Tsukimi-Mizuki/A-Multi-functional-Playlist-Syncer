using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;

namespace NeteaseMusicManager.Core.Tests.Ncm;

/// <summary>
/// 按 NCM 格式的逆算法构造合成的 .ncm 测试文件（AES-128-ECB 加密密钥/元数据 + RC4 变种流式解扰）。
/// 与生产实现相互独立：生产者按格式规范加密，被测实现按格式规范解密，从而验证双向一致性。
/// </summary>
internal static class SyntheticNcmFile
{
    internal const string DefaultMetadataJson =
        """{"musicName":"测试歌曲","album":"测试专辑","artist":[["测试歌手",123],["第二歌手",456]],"bitrate":320000,"duration":215000,"format":"mp3","musicId":1234567890,"alias":["别名"]}""";

    private static readonly byte[] NeteaseMagic = "CTENFDAM"u8.ToArray();
    private static readonly byte[] MetaPrefix = Encoding.ASCII.GetBytes("163 key(Don't modify):");
    private static readonly byte[] MusicPrefix = Encoding.ASCII.GetBytes("music:");

    internal static readonly byte[] CoreKey =
    {
        0x68, 0x7A, 0x48, 0x52, 0x41, 0x6D, 0x73, 0x6F, 0x35, 0x6B, 0x49, 0x6E, 0x62, 0x61, 0x78, 0x57,
    };

    internal static readonly byte[] ModifyKey =
    {
        0x23, 0x31, 0x34, 0x6C, 0x6A, 0x6B, 0x5F, 0x21, 0x5C, 0x5D, 0x26, 0x30, 0x55, 0x3C, 0x27, 0x28,
    };

    internal static byte[] Build(
        byte[] audio,
        byte[]? cover = null,
        string? metadataJson = DefaultMetadataJson,
        int coverPadding = 0,
        bool includeMetadata = true,
        int keyMaterialLength = 95)
    {
        ArgumentNullException.ThrowIfNull(audio);
        if (keyMaterialLength <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(keyMaterialLength));
        }

        // 17 字节前缀之后的 keyMaterial 才是 keyBox 的密钥流。
        var keyPlain = new byte[17 + keyMaterialLength];
        for (var i = 0; i < 17; i++)
        {
            keyPlain[i] = (byte)(0xA0 + i);
        }

        var keyMaterial = new byte[keyMaterialLength];
        for (var i = 0; i < keyMaterial.Length; i++)
        {
            keyMaterial[i] = (byte)(i * 7 + 3);
        }

        keyMaterial.CopyTo(keyPlain, 17);

        var encryptedKey = AesEcbEncrypt(PadPkcs7(keyPlain), CoreKey);
        for (var i = 0; i < encryptedKey.Length; i++)
        {
            encryptedKey[i] ^= 0x64;
        }

        using var stream = new MemoryStream();
        stream.Write(NeteaseMagic);
        stream.Write(new byte[] { 0x01, 0x00 });

        WriteInt32(stream, encryptedKey.Length);
        stream.Write(encryptedKey);

        if (includeMetadata && metadataJson is not null)
        {
            var json = "music:" + metadataJson;
            var plainMetadata = Encoding.UTF8.GetBytes(json);
            var encryptedMetadata = AesEcbEncrypt(PadPkcs7(plainMetadata), ModifyKey);
            var base64 = Convert.ToBase64String(encryptedMetadata);
            var payload = Encoding.ASCII.GetBytes("163 key(Don't modify):" + base64);
            for (var i = 0; i < payload.Length; i++)
            {
                payload[i] ^= 0x63;
            }

            WriteInt32(stream, payload.Length);
            stream.Write(payload);
        }
        else
        {
            WriteInt32(stream, 0);
        }

        // 4 字节 CRC + 1 字节封面版本（解密器直接跳过）。
        stream.Write(new byte[] { 0x11, 0x22, 0x33, 0x44, 0x02 });

        var image = cover ?? Array.Empty<byte>();
        WriteInt32(stream, image.Length + coverPadding);
        WriteInt32(stream, image.Length);
        stream.Write(image);
        for (var i = 0; i < coverPadding; i++)
        {
            stream.WriteByte(0x55);
        }

        var keyBox = BuildKeyBox(keyMaterial);
        for (var offset = 0; offset < audio.Length; offset += 32 * 1024)
        {
            var count = Math.Min(32 * 1024, audio.Length - offset);
            var chunk = audio.AsSpan(offset, count).ToArray();
            for (var i = 0; i < count; i++)
            {
                var j = (i + 1) & 0xFF;
                chunk[i] ^= keyBox[(keyBox[j] + keyBox[(keyBox[j] + j) & 0xFF]) & 0xFF];
            }

            stream.Write(chunk);
        }

        return stream.ToArray();
    }

    /// <summary>与生产实现相同的 keyBox 构造（按规范独立实现）。</summary>
    internal static byte[] BuildKeyBox(byte[] keyMaterial)
    {
        var keyBox = new byte[256];
        for (var i = 0; i < keyBox.Length; i++)
        {
            keyBox[i] = (byte)i;
        }

        var keyOffset = 0;
        byte last = 0;
        for (var i = 0; i < keyBox.Length; i++)
        {
            var swap = keyBox[i];
            var c = (byte)((swap + last + keyMaterial[keyOffset]) & 0xFF);
            keyOffset++;
            if (keyOffset >= keyMaterial.Length)
            {
                keyOffset = 0;
            }

            keyBox[i] = keyBox[c];
            keyBox[c] = swap;
            last = c;
        }

        return keyBox;
    }

    internal static byte[] AesEcbEncrypt(byte[] plainText, byte[] key)
    {
        using var aes = Aes.Create();
        aes.Key = key;
        aes.Mode = CipherMode.ECB;
        aes.Padding = PaddingMode.None;
        using var encryptor = aes.CreateEncryptor();
        return encryptor.TransformFinalBlock(plainText, 0, plainText.Length);
    }

    internal static byte[] PadPkcs7(byte[] data)
    {
        var padding = 16 - (data.Length % 16);
        var padded = new byte[data.Length + padding];
        Buffer.BlockCopy(data, 0, padded, 0, data.Length);
        for (var i = data.Length; i < padded.Length; i++)
        {
            padded[i] = (byte)padding;
        }

        return padded;
    }

    private static void WriteInt32(Stream stream, int value)
    {
        Span<byte> buffer = stackalloc byte[4];
        BinaryPrimitives.WriteInt32LittleEndian(buffer, value);
        stream.Write(buffer);
    }
}
