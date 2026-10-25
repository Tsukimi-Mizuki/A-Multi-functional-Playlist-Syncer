using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace NeteaseMusicManager.Core.Ncm;

/// <summary>
/// 网易云 .ncm 加密文件解密器。
/// 文件结构：CTENFDAM 魔数 → 2 字节跳过 → AES-128-ECB 加密的 RC4 密钥 → 加密的 music JSON 元数据
/// → CRC/版本 → 封面帧 → AES 流式解扰后的 MP3/FLAC 音频。
/// 算法参考社区 ncmdump/ncmcrypt 实现（.research/ncmcrypt.cpp）。
/// </summary>
/// <remarks>
/// <para>
/// <see cref="DecryptAsync"/> 对“可预期”的失败（目标文件已存在且未允许覆盖、文件头损坏、不是 NCM）返回
/// <see cref="NcmDecryptResult.Success"/> 为 <see langword="false"/> 的结果而不是抛异常，便于批量导入逐条展示错误；
/// 文件不存在、参数非法、取消以及磁盘 I/O 错误仍会抛出异常。
/// </para>
/// <para>输出先写入同目录临时文件，成功后再原子移动为目标文件，绝不会留下半截成品；取消/失败会清理临时文件。</para>
/// </remarks>
public sealed class NcmDecoder : INcmDecoder
{
    private static readonly byte[] NeteaseMagic = "CTENFDAM"u8.ToArray();
    private static readonly byte[] CoreKey =
    {
        0x68, 0x7A, 0x48, 0x52, 0x41, 0x6D, 0x73, 0x6F, 0x35, 0x6B, 0x49, 0x6E, 0x62, 0x61, 0x78, 0x57,
    };

    private static readonly byte[] ModifyKey =
    {
        0x23, 0x31, 0x34, 0x6C, 0x6A, 0x6B, 0x5F, 0x21, 0x5C, 0x5D, 0x26, 0x30, 0x55, 0x3C, 0x27, 0x28,
    };

    private static readonly byte[] FlacMagic = "fLaC"u8.ToArray();
    private static readonly byte[] Id3Magic = "ID3"u8.ToArray();
    private static readonly byte[] MetaKeyPrefix = Encoding.ASCII.GetBytes("163 key(Don't modify):");
    private static readonly byte[] MusicPrefix = Encoding.ASCII.GetBytes("music:");

    private const int AudioChunkSize = 32 * 1024;
    private const int MaxKeyLength = 4 * 1024 * 1024;
    private const int MaxMetadataLength = 16 * 1024 * 1024;
    private const int MaxCoverLength = 64 * 1024 * 1024;

    /// <inheritdoc />
    public bool IsNcmFile(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return false;
        }

        try
        {
            using var stream = new FileStream(
                path,
                FileMode.Open,
                FileAccess.Read,
                FileShare.Read,
                bufferSize: 8,
                FileOptions.SequentialScan);

            Span<byte> header = stackalloc byte[8];
            return stream.ReadAtLeast(header, header.Length, throwOnEndOfStream: false) == header.Length
                   && header.SequenceEqual(NeteaseMagic);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return false;
        }
    }

    /// <inheritdoc />
    public async Task<NcmFileInfo> ReadInfoAsync(string ncmPath, CancellationToken cancellationToken = default)
    {
        ValidateInputPath(ncmPath);
        cancellationToken.ThrowIfCancellationRequested();

        if (!File.Exists(ncmPath))
        {
            throw new FileNotFoundException("找不到要读取的 NCM 文件。", ncmPath);
        }

        await using var stream = OpenRead(ncmPath);
        var header = await ParseHeaderAsync(stream, readCover: false, cancellationToken).ConfigureAwait(false);
        return new NcmFileInfo(ncmPath, header.Metadata);
    }

    /// <inheritdoc />
    public async Task<NcmDecryptResult> DecryptAsync(
        string ncmPath,
        string outputDirectory,
        bool overwrite = false,
        CancellationToken cancellationToken = default)
    {
        ValidateInputPath(ncmPath);
        if (string.IsNullOrWhiteSpace(outputDirectory))
        {
            throw new ArgumentException("输出目录不能为空。", nameof(outputDirectory));
        }

        cancellationToken.ThrowIfCancellationRequested();

        if (!File.Exists(ncmPath))
        {
            throw new FileNotFoundException("找不到要解密的 NCM 文件。", ncmPath);
        }

        string fullOutputDirectory;
        try
        {
            fullOutputDirectory = Path.GetFullPath(outputDirectory);
        }
        catch (Exception ex) when (ex is ArgumentException or NotSupportedException or PathTooLongException)
        {
            throw new ArgumentException("输出目录路径无效。", nameof(outputDirectory), ex);
        }

        Directory.CreateDirectory(fullOutputDirectory);

        await using var source = OpenRead(ncmPath);

        ParsedHeader header;
        try
        {
            header = await ParseHeaderAsync(source, readCover: true, cancellationToken).ConfigureAwait(false);
        }
        catch (InvalidDataException ex)
        {
            return new NcmDecryptResult(ncmPath, null, false, ErrorMessage: ex.Message);
        }

        // 音频解扰：每 32KB 一块，j 在每个块内从 1 重新开始。
        var chunk = new byte[AudioChunkSize];
        var firstRead = await ReadChunkAsync(source, chunk, cancellationToken).ConfigureAwait(false);
        if (firstRead <= 0)
        {
            return new NcmDecryptResult(ncmPath, null, false, ErrorMessage: "NCM 文件中没有音频数据。");
        }

        DecryptChunk(chunk, firstRead, header.KeyBox);
        var format = DetectFormat(chunk.AsSpan(0, firstRead));
        var outputPath = Path.Combine(fullOutputDirectory, BuildOutputFileName(ncmPath, format));

        if (!overwrite && File.Exists(outputPath))
        {
            return new NcmDecryptResult(
                ncmPath,
                null,
                false,
                ErrorMessage: $"目标文件已存在，未启用覆盖：{outputPath}");
        }

        var tempPath = Path.Combine(
            fullOutputDirectory,
            $".{GetSafeStem(ncmPath)}.{Guid.NewGuid():N}.ncmtmp");

        try
        {
            await using (var output = new FileStream(
                             tempPath,
                             FileMode.CreateNew,
                             FileAccess.Write,
                             FileShare.None,
                             bufferSize: 64 * 1024,
                             FileOptions.Asynchronous | FileOptions.SequentialScan))
            {
                await output.WriteAsync(chunk.AsMemory(0, firstRead), cancellationToken).ConfigureAwait(false);

                var buffer = new byte[AudioChunkSize];
                int read;
                while ((read = await ReadChunkAsync(source, buffer, cancellationToken).ConfigureAwait(false)) > 0)
                {
                    DecryptChunk(buffer, read, header.KeyBox);
                    await output.WriteAsync(buffer.AsMemory(0, read), cancellationToken).ConfigureAwait(false);
                }

                await output.FlushAsync(cancellationToken).ConfigureAwait(false);
            }

            File.Move(tempPath, outputPath, overwrite);
        }
        catch
        {
            TryDeleteFile(tempPath);
            throw;
        }

        return new NcmDecryptResult(
            ncmPath,
            outputPath,
            true,
            format,
            header.Metadata,
            header.CoverData);
    }

    private static FileStream OpenRead(string path) =>
        new(path, FileMode.Open, FileAccess.Read, FileShare.Read, bufferSize: 64 * 1024, FileOptions.Asynchronous | FileOptions.SequentialScan);

    private static void ValidateInputPath(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            throw new ArgumentException("NCM 文件路径不能为空。", nameof(path));
        }
    }

    /// <summary>解析文件头：密钥、加密元数据与封面帧，返回时流已定位到音频数据起点。</summary>
    private static async Task<ParsedHeader> ParseHeaderAsync(
        FileStream stream,
        bool readCover,
        CancellationToken cancellationToken)
    {
        var magic = new byte[NeteaseMagic.Length];
        await ReadExactAsync(stream, magic, cancellationToken).ConfigureAwait(false);
        if (!magic.AsSpan().SequenceEqual(NeteaseMagic))
        {
            throw new InvalidDataException("不是有效的网易云 NCM 文件（文件头不匹配）。");
        }

        Skip(stream, 2, "文件头中的保留字段");

        var keyLength = await ReadInt32Async(stream, cancellationToken).ConfigureAwait(false);
        if (keyLength <= 0 || keyLength > MaxKeyLength)
        {
            throw new InvalidDataException($"NCM 密钥长度无效（{keyLength}）。");
        }

        var encryptedKey = new byte[keyLength];
        await ReadExactAsync(stream, encryptedKey, cancellationToken).ConfigureAwait(false);
        XorInPlace(encryptedKey, 0x64);

        var keyData = AesEcbDecrypt(encryptedKey, CoreKey);
        var keyBox = BuildKeyBox(keyData);

        NcmMetadata? metadata = null;
        var metadataLength = await ReadInt32Async(stream, cancellationToken).ConfigureAwait(false);
        if (metadataLength > 0)
        {
            if (metadataLength > MaxMetadataLength)
            {
                throw new InvalidDataException($"NCM 元数据长度无效（{metadataLength}）。");
            }

            var encryptedMetadata = new byte[metadataLength];
            await ReadExactAsync(stream, encryptedMetadata, cancellationToken).ConfigureAwait(false);
            XorInPlace(encryptedMetadata, 0x63);

            // 元数据缺失/损坏绝不能中断音频解密，因此这里只返回 null。
            metadata = TryParseMetadata(encryptedMetadata);
        }

        Skip(stream, 5, "CRC 与封面版本号");

        var coverFrameLength = await ReadUInt32Async(stream, cancellationToken).ConfigureAwait(false);
        var imageLength = await ReadInt32Async(stream, cancellationToken).ConfigureAwait(false);
        if (imageLength < 0 || imageLength > MaxCoverLength)
        {
            throw new InvalidDataException($"NCM 封面长度无效（{imageLength}）。");
        }

        byte[]? coverData = null;
        if (imageLength > 0)
        {
            if (readCover)
            {
                coverData = new byte[imageLength];
                await ReadExactAsync(stream, coverData, cancellationToken).ConfigureAwait(false);
            }
            else
            {
                Skip(stream, imageLength, "封面数据");
            }
        }

        var padding = (long)coverFrameLength - imageLength;
        if (padding > 0)
        {
            Skip(stream, padding, "封面帧填充数据");
        }

        return new ParsedHeader(keyBox, metadata, coverData);
    }

    private static async Task<int> ReadChunkAsync(Stream stream, byte[] buffer, CancellationToken cancellationToken)
    {
        var total = 0;
        while (total < buffer.Length)
        {
            var read = await stream
                .ReadAsync(buffer.AsMemory(total, buffer.Length - total), cancellationToken)
                .ConfigureAwait(false);
            if (read == 0)
            {
                break;
            }

            total += read;
        }

        return total;
    }

    private static async Task ReadExactAsync(Stream stream, byte[] buffer, CancellationToken cancellationToken)
    {
        var offset = 0;
        while (offset < buffer.Length)
        {
            var read = await stream
                .ReadAsync(buffer.AsMemory(offset, buffer.Length - offset), cancellationToken)
                .ConfigureAwait(false);
            if (read == 0)
            {
                throw new InvalidDataException("NCM 文件不完整（读取越界）。");
            }

            offset += read;
        }
    }

    private static async Task<int> ReadInt32Async(Stream stream, CancellationToken cancellationToken)
    {
        var buffer = new byte[4];
        await ReadExactAsync(stream, buffer, cancellationToken).ConfigureAwait(false);
        return BinaryPrimitives.ReadInt32LittleEndian(buffer);
    }

    private static async Task<uint> ReadUInt32Async(Stream stream, CancellationToken cancellationToken)
    {
        var buffer = new byte[4];
        await ReadExactAsync(stream, buffer, cancellationToken).ConfigureAwait(false);
        return BinaryPrimitives.ReadUInt32LittleEndian(buffer);
    }

    private static void Skip(Stream stream, long count, string description)
    {
        if (count <= 0)
        {
            return;
        }

        if (stream.CanSeek)
        {
            if (count > stream.Length - stream.Position)
            {
                throw new InvalidDataException($"NCM 文件不完整（跳过 {description} 时越界）。");
            }

            stream.Seek(count, SeekOrigin.Current);
            return;
        }

        var buffer = new byte[Math.Min(count, 8192)];
        var remaining = count;
        while (remaining > 0)
        {
            var want = (int)Math.Min(buffer.Length, remaining);
            var read = stream.Read(buffer, 0, want);
            if (read == 0)
            {
                throw new InvalidDataException($"NCM 文件不完整（跳过 {description} 时越界）。");
            }

            remaining -= read;
        }
    }

    private static void XorInPlace(byte[] data, byte mask)
    {
        for (var i = 0; i < data.Length; i++)
        {
            data[i] ^= mask;
        }
    }

    /// <summary>AES-128-ECB 解密（无填充模式手动去除 PKCS#7 填充，与参考实现逐块行为一致）。</summary>
    private static byte[] AesEcbDecrypt(byte[] cipherText, byte[] key)
    {
        if (cipherText.Length == 0)
        {
            return Array.Empty<byte>();
        }

        var blockCount = cipherText.Length / 16;
        if (blockCount == 0)
        {
            return Array.Empty<byte>();
        }

        using var aes = Aes.Create();
        aes.Key = key;
        aes.Mode = CipherMode.ECB;
        aes.Padding = PaddingMode.None;

        using var decryptor = aes.CreateDecryptor();
        var plain = decryptor.TransformFinalBlock(cipherText, 0, blockCount * 16);
        return StripPkcs7Padding(plain);
    }

    /// <summary>与参考实现一致：最后字节为 1..16 时视为 PKCS#7 填充并去除，否则保留全部数据。</summary>
    private static byte[] StripPkcs7Padding(byte[] data)
    {
        if (data.Length == 0)
        {
            return data;
        }

        var padding = data[^1];
        if (padding is > 0 and <= 16 && padding <= data.Length)
        {
            return data[..^padding];
        }

        return data;
    }

    /// <summary>用解密出的 RC4 变种密钥数据构建 256 字节 keyBox。</summary>
    private static byte[] BuildKeyBox(byte[] keyData)
    {
        if (keyData.Length <= 17)
        {
            throw new InvalidDataException("NCM 密钥数据长度无效。");
        }

        var key = keyData.AsSpan(17);
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
            var c = (byte)((swap + last + key[keyOffset]) & 0xFF);
            keyOffset++;
            if (keyOffset >= key.Length)
            {
                keyOffset = 0;
            }

            keyBox[i] = keyBox[c];
            keyBox[c] = swap;
            last = c;
        }

        return keyBox;
    }

    private static void DecryptChunk(byte[] buffer, int count, byte[] keyBox)
    {
        for (var i = 0; i < count; i++)
        {
            var j = (i + 1) & 0xFF;
            buffer[i] ^= keyBox[(keyBox[j] + keyBox[(keyBox[j] + j) & 0xFF]) & 0xFF];
        }
    }

    private static string DetectFormat(ReadOnlySpan<byte> firstChunk)
    {
        if (firstChunk.Length >= Id3Magic.Length && firstChunk[..Id3Magic.Length].SequenceEqual(Id3Magic))
        {
            return "mp3";
        }

        if (firstChunk.Length >= FlacMagic.Length && firstChunk[..FlacMagic.Length].SequenceEqual(FlacMagic))
        {
            return "flac";
        }

        // 无 ID3v2 头的 MP3 帧同步字（0xFFEx）。
        if (firstChunk.Length >= 2 && firstChunk[0] == 0xFF && (firstChunk[1] & 0xE0) == 0xE0)
        {
            return "mp3";
        }

        return "flac";
    }

    private static string BuildOutputFileName(string ncmPath, string format)
    {
        var stem = Path.GetFileNameWithoutExtension(ncmPath);
        if (string.IsNullOrWhiteSpace(stem))
        {
            stem = "decrypted";
        }

        return $"{stem}.{format}";
    }

    private static string GetSafeStem(string ncmPath)
    {
        var stem = Path.GetFileNameWithoutExtension(ncmPath);
        foreach (var c in Path.GetInvalidFileNameChars())
        {
            stem = stem.Replace(c, '_');
        }

        return string.IsNullOrWhiteSpace(stem) ? "ncm" : stem;
    }

    private static NcmMetadata? TryParseMetadata(byte[] encryptedMetadata)
    {
        try
        {
            var decrypted = DecryptMetadata(encryptedMetadata);
            return ParseMetadataJson(decrypted);
        }
        catch (Exception ex) when (ex is InvalidDataException or FormatException or JsonException or CryptographicException or ArgumentException)
        {
            // 坏元数据只意味着不能自动补标签，音频仍应解密成功。
            return null;
        }
    }

    private static string DecryptMetadata(byte[] encryptedMetadata)
    {
        var span = encryptedMetadata.AsSpan();
        if (span.Length >= MetaKeyPrefix.Length && span[..MetaKeyPrefix.Length].SequenceEqual(MetaKeyPrefix))
        {
            span = span[MetaKeyPrefix.Length..];
        }
        else
        {
            throw new InvalidDataException("NCM 元数据缺少 163 key 前缀。");
        }

        if (span.Length == 0)
        {
            throw new InvalidDataException("NCM 元数据为空。");
        }

        var base64 = Encoding.ASCII.GetString(span);
        byte[] cipher;
        try
        {
            cipher = Convert.FromBase64String(base64);
        }
        catch (FormatException ex)
        {
            throw new InvalidDataException("NCM 元数据 base64 解码失败。", ex);
        }

        var plain = AesEcbDecrypt(cipher, ModifyKey);
        if (plain.Length < MusicPrefix.Length || !plain.AsSpan(0, MusicPrefix.Length).SequenceEqual(MusicPrefix))
        {
            throw new InvalidDataException("NCM 元数据解密结果缺少 music: 前缀。");
        }

        var json = Encoding.UTF8.GetString(plain, MusicPrefix.Length, plain.Length - MusicPrefix.Length);
        return json.TrimEnd('\0', ' ', '\r', '\n', '\t');
    }

    private static NcmMetadata? ParseMetadataJson(string json)
    {
        using var document = JsonDocument.Parse(json);
        var root = document.RootElement;
        if (root.ValueKind != JsonValueKind.Object)
        {
            return null;
        }

        return new NcmMetadata(
            Title: ReadString(root, "musicName"),
            Artists: ReadArtists(root),
            Album: ReadString(root, "album"),
            Format: ReadString(root, "format"),
            Bitrate: ReadInt32(root, "bitrate"),
            DurationMs: ReadInt64(root, "duration"),
            MusicId: ReadInt64OrNull(root, "musicId"),
            Alias: ReadAlias(root));
    }

    private static string? ReadString(JsonElement root, string propertyName)
    {
        if (!root.TryGetProperty(propertyName, out var value))
        {
            return null;
        }

        var text = value.ValueKind switch
        {
            JsonValueKind.String => value.GetString(),
            JsonValueKind.Number => value.GetRawText(),
            _ => null,
        };

        return string.IsNullOrWhiteSpace(text) ? null : text.Trim();
    }

    private static string? ReadArtists(JsonElement root)
    {
        if (!root.TryGetProperty("artist", out var artists) || artists.ValueKind != JsonValueKind.Array)
        {
            return null;
        }

        var names = new List<string>();
        foreach (var artist in artists.EnumerateArray())
        {
            string? name = artist.ValueKind switch
            {
                JsonValueKind.Array when artist.GetArrayLength() > 1 && artist[0].ValueKind == JsonValueKind.String
                    => artist[0].GetString(),
                JsonValueKind.Array when artist.GetArrayLength() > 0 && artist[0].ValueKind == JsonValueKind.String
                    => artist[0].GetString(),
                JsonValueKind.String => artist.GetString(),
                JsonValueKind.Object when artist.TryGetProperty("name", out var nameValue) && nameValue.ValueKind == JsonValueKind.String
                    => nameValue.GetString(),
                _ => null,
            };

            if (!string.IsNullOrWhiteSpace(name))
            {
                names.Add(name.Trim());
            }
        }

        return names.Count == 0 ? null : string.Join("/", names);
    }

    private static string? ReadAlias(JsonElement root)
    {
        if (!root.TryGetProperty("alias", out var alias))
        {
            return null;
        }

        if (alias.ValueKind == JsonValueKind.String)
        {
            var single = alias.GetString();
            return string.IsNullOrWhiteSpace(single) ? null : single.Trim();
        }

        if (alias.ValueKind != JsonValueKind.Array)
        {
            return null;
        }

        var values = alias.EnumerateArray()
            .Where(item => item.ValueKind == JsonValueKind.String)
            .Select(item => item.GetString()?.Trim())
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .ToArray();

        return values.Length == 0 ? null : string.Join("/", values);
    }

    private static int ReadInt32(JsonElement root, string propertyName)
    {
        if (!root.TryGetProperty(propertyName, out var value))
        {
            return 0;
        }

        return value.ValueKind switch
        {
            JsonValueKind.Number when value.TryGetInt32(out var number) => number,
            JsonValueKind.Number when value.TryGetInt64(out var number) => (int)Math.Clamp(number, int.MinValue, int.MaxValue),
            JsonValueKind.String when int.TryParse(value.GetString(), out var number) => number,
            _ => 0,
        };
    }

    private static long ReadInt64(JsonElement root, string propertyName)
    {
        if (!root.TryGetProperty(propertyName, out var value))
        {
            return 0;
        }

        return value.ValueKind switch
        {
            JsonValueKind.Number when value.TryGetInt64(out var number) => number,
            JsonValueKind.Number when value.TryGetDouble(out var number) => (long)number,
            JsonValueKind.String when long.TryParse(value.GetString(), out var number) => number,
            _ => 0,
        };
    }

    private static long? ReadInt64OrNull(JsonElement root, string propertyName)
    {
        var value = ReadInt64(root, propertyName);
        return value > 0 ? value : null;
    }

    private static void TryDeleteFile(string? path)
    {
        if (string.IsNullOrEmpty(path))
        {
            return;
        }

        try
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // 临时文件清理失败不覆盖原始异常。
        }
    }

    private readonly record struct ParsedHeader(byte[] KeyBox, NcmMetadata? Metadata, byte[]? CoverData);
}
