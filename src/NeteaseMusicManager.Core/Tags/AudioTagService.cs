using System.Globalization;
using System.Text.RegularExpressions;
using TagLibFile = TagLib.File;

namespace NeteaseMusicManager.Core.Tags;

/// <summary>
/// 基于 TagLibSharp 2.3.0 的音频标签读写服务，支持 MP3(ID3v2 APIC) / FLAC(VorbisComment + PICTURE) /
/// M4A(covr) / OGG 等容器；按容器自动选择标签实现。
/// </summary>
/// <remarks>
/// <para>网易云歌曲 ID 写入 Comment 标记 <c>NeteaseMusicManager:&lt;id&gt;</c>，写入时保留其他备注文本，
/// 读取时从 Comment 中解析该标记。</para>
/// <para><see cref="AudioTagInfo.Bitrate"/> 为 TagLib 报告的 kbps 值。读取不可识别/损坏文件返回
/// <see langword="null"/>；写入不支持或损坏的文件抛出带说明的 <see cref="InvalidOperationException"/>。</para>
/// </remarks>
public sealed class AudioTagService : IAudioTagService
{
    private const string NeteaseCommentPrefix = "NeteaseMusicManager:";

    private static readonly Regex NeteaseIdPattern = new(
        @"NeteaseMusicManager\s*:\s*(?<id>\d{1,19})",
        RegexOptions.Compiled | RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    private static readonly Regex NeteaseCommentCleanupPattern = new(
        @"NeteaseMusicManager(?:\s*:\s*\d{1,19})?",
        RegexOptions.Compiled | RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

    private static readonly byte[] PngMagic = { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A };
    private static readonly byte[] JpegMagic = { 0xFF, 0xD8, 0xFF };
    private static readonly byte[] GifMagic = { 0x47, 0x49, 0x46, 0x38 };
    private static readonly byte[] BmpMagic = { 0x42, 0x4D };

    /// <inheritdoc />
    public Task<AudioTagInfo?> ReadAsync(string audioPath, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(audioPath))
        {
            throw new ArgumentException("音频文件路径不能为空。", nameof(audioPath));
        }

        return Task.Run(() => ReadCore(audioPath, cancellationToken), cancellationToken);
    }

    /// <inheritdoc />
    public Task WriteAsync(string audioPath, AudioTagData tags, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(audioPath))
        {
            throw new ArgumentException("音频文件路径不能为空。", nameof(audioPath));
        }

        ArgumentNullException.ThrowIfNull(tags);

        return Task.Run(() => WriteCore(audioPath, tags, cancellationToken), cancellationToken);
    }

    private static AudioTagInfo? ReadCore(string audioPath, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (!System.IO.File.Exists(audioPath))
        {
            return null;
        }

        TagLibFile? file = null;
        try
        {
            file = TagLibFile.Create(audioPath);
            var tag = file.Tag;
            var properties = file.Properties;

            var durationMs = properties is null
                ? 0L
                : Math.Max(0L, (long)Math.Round(properties.Duration.TotalMilliseconds, MidpointRounding.AwayFromZero));
            var bitrate = properties?.AudioBitrate ?? 0;

            var fileInfo = new System.IO.FileInfo(audioPath);
            return new AudioTagInfo(
                Title: NullIfBlank(tag.Title),
                Artist: JoinValues(tag.Performers),
                Album: NullIfBlank(tag.Album),
                AlbumArtist: JoinValues(tag.AlbumArtists),
                DurationMs: durationMs,
                Bitrate: bitrate,
                Lyrics: NullIfBlank(tag.Lyrics),
                HasCover: HasCover(tag),
                NeteaseId: TryParseNeteaseId(tag.Comment),
                Year: tag.Year > 0 ? tag.Year : null,
                TrackNumber: tag.Track > 0 ? tag.Track : null,
                DiscNumber: tag.Disc > 0 ? tag.Disc : null,
                Genre: JoinValues(tag.Genres),
                Composer: JoinValues(tag.Composers),
                Comment: CleanDisplayComment(tag.Comment),
                CoverData: TryGetCoverData(tag),
                FileSize: fileInfo.Length,
                FileFormat: System.IO.Path.GetExtension(audioPath).TrimStart('.').ToUpperInvariant());
        }
        catch (Exception ex) when (IsRecoverableReadError(ex))
        {
            return null;
        }
        finally
        {
            file?.Dispose();
        }
    }

    private static void WriteCore(string audioPath, AudioTagData tags, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (!System.IO.File.Exists(audioPath))
        {
            throw new FileNotFoundException("找不到要写入标签的音频文件。", audioPath);
        }

        TagLibFile file;
        try
        {
            file = TagLibFile.Create(audioPath);
        }
        catch (Exception ex) when (ex is TagLib.UnsupportedFormatException or TagLib.CorruptFileException)
        {
            throw new InvalidOperationException(
                $"无法写入标签：不支持或已损坏的音频文件「{System.IO.Path.GetFileName(audioPath)}」。",
                ex);
        }

        using (file)
        {
            try
            {
                var tag = ResolveWritableTag(file);
                ApplyTag(file, tag, tags);
                file.Save();
            }
            catch (Exception ex) when (ex is TagLib.CorruptFileException or TagLib.UnsupportedFormatException)
            {
                throw new InvalidOperationException(
                    $"保存标签失败：文件格式不支持或已损坏「{System.IO.Path.GetFileName(audioPath)}」。",
                    ex);
            }
        }
    }

    /// <summary>MP3 优先使用 ID3v2（UTF-16/UTF-8 中文安全）；其他容器使用容器自身的标签实现。</summary>
    private static TagLib.Tag ResolveWritableTag(TagLibFile file)
    {
        if (file is TagLib.Mpeg.File)
        {
            if (file.GetTag(TagLib.TagTypes.Id3v2, true) is TagLib.Id3v2.Tag id3v2)
            {
                return id3v2;
            }
        }
        else if (file is TagLib.Ogg.File)
        {
            if (file.GetTag(TagLib.TagTypes.Xiph, true) is TagLib.Ogg.XiphComment xiph)
            {
                return xiph;
            }
        }

        return file.Tag;
    }

    private static void ApplyTag(TagLibFile file, TagLib.Tag tag, AudioTagData data)
    {
        var replace = data.ReplaceAll;

        if (replace || data.Title is not null)
        {
            tag.Title = data.Title ?? string.Empty;
        }

        var artists = CleanValues(data.Artists);
        if (replace || artists.Length > 0)
        {
            tag.Performers = artists;
        }

        if (replace || data.Album is not null)
        {
            tag.Album = data.Album ?? string.Empty;
        }

        if (replace || data.AlbumArtist is not null)
        {
            tag.AlbumArtists = string.IsNullOrWhiteSpace(data.AlbumArtist)
                ? Array.Empty<string>()
                : new[] { data.AlbumArtist.Trim() };
        }

        if (replace || data.TrackNumber is uint)
        {
            tag.Track = data.TrackNumber ?? 0;
        }

        if (replace || data.DiscNumber is uint)
        {
            tag.Disc = data.DiscNumber ?? 0;
        }

        if (replace || data.Year is uint)
        {
            tag.Year = data.Year ?? 0;
        }

        var genres = SplitValues(data.Genre);
        if (replace || genres.Length > 0)
        {
            tag.Genres = genres;
        }

        var composers = SplitValues(data.Composer);
        if (replace || composers.Length > 0)
        {
            tag.Composers = composers;
        }

        var lyrics = FirstNonBlank(data.WordLyrics, data.PlainLyrics);
        if (replace || lyrics is not null)
        {
            tag.Lyrics = lyrics ?? string.Empty;
        }

        ApplyComment(tag, data);
        ApplyCover(tag, data);
    }

    private static void ApplyComment(TagLib.Tag tag, AudioTagData data)
    {
        var existing = data.ReplaceAll ? null : NullIfBlank(tag.Comment);
        var baseComment = data.Comment ?? existing;

        if (data.NeteaseId is long neteaseId && neteaseId > 0)
        {
            var marker = NeteaseCommentPrefix + neteaseId.ToString(CultureInfo.InvariantCulture);
            var cleaned = CleanComment(NeteaseCommentCleanupPattern.Replace(baseComment ?? string.Empty, string.Empty));
            tag.Comment = cleaned.Length == 0 ? marker : $"{cleaned}; {marker}";
            return;
        }

        if (data.ReplaceAll)
        {
            tag.Comment = NullIfBlank(baseComment) ?? string.Empty;
            return;
        }

        var comment = NullIfBlank(baseComment);
        if (comment is not null)
        {
            tag.Comment = comment;
        }
    }

    private static void ApplyCover(TagLib.Tag tag, AudioTagData data)
    {
        if (data.CoverData is not { Length: > 0 } coverData)
        {
            return;
        }

        var mimeType = string.IsNullOrWhiteSpace(data.CoverMimeType)
            ? DetectImageMime(coverData)
            : data.CoverMimeType.Trim();

        var picture = new TagLib.Picture(new TagLib.ByteVector(coverData))
        {
            Type = TagLib.PictureType.FrontCover,
            MimeType = mimeType,
            Description = "Cover",
        };

        // TagLibSharp 2.3.0 统一通过 Tag.Pictures 处理容器图片：
        // MP3 → ID3v2 APIC，FLAC → VorbisComment METADATA_BLOCK_PICTURE（FLAC 原生图片结构），M4A → covr。
        tag.Pictures = new TagLib.IPicture[] { picture };
    }

    private static bool HasCover(TagLib.Tag tag)
    {
        try
        {
            return tag.Pictures is { Length: > 0 };
        }
        catch (Exception ex) when (ex is TagLib.CorruptFileException or IOException or InvalidOperationException)
        {
            return false;
        }
    }

    private static long? TryParseNeteaseId(string? comment)
    {
        if (string.IsNullOrWhiteSpace(comment))
        {
            return null;
        }

        var match = NeteaseIdPattern.Match(comment);
        if (!match.Success)
        {
            return null;
        }

        return long.TryParse(
            match.Groups["id"].Value,
            NumberStyles.Integer,
            CultureInfo.InvariantCulture,
            out var id)
            ? id
            : null;
    }

    private static string CleanComment(string value) =>
        value.Trim().Trim(';', ',', '|', '\r', '\n', '\t', ' ', '\0').Trim();

    private static string? FirstNonBlank(params string?[] values)
    {
        foreach (var value in values)
        {
            if (!string.IsNullOrWhiteSpace(value))
            {
                return value;
            }
        }

        return null;
    }

    private static string? NullIfBlank(string? value) => string.IsNullOrWhiteSpace(value) ? null : value.Trim();

    private static string? JoinValues(string[]? values)
    {
        if (values is null)
        {
            return null;
        }

        var joined = string.Join(
            "/",
            values.Where(value => !string.IsNullOrWhiteSpace(value)).Select(value => value.Trim()));

        return string.IsNullOrWhiteSpace(joined) ? null : joined;
    }

    private static string[] SplitValues(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return Array.Empty<string>();
        }

        return value
            .Split(new[] { '/', ';', '；', '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
            .Where(part => !string.IsNullOrWhiteSpace(part))
            .ToArray();
    }

    private static string? CleanDisplayComment(string? comment)
    {
        if (string.IsNullOrWhiteSpace(comment))
        {
            return null;
        }

        var cleaned = CleanComment(NeteaseCommentCleanupPattern.Replace(comment, string.Empty));
        return NullIfBlank(cleaned);
    }

    private static byte[]? TryGetCoverData(TagLib.Tag tag)
    {
        try
        {
            return tag.Pictures is { Length: > 0 } pictures ? pictures[0]?.Data?.Data : null;
        }
        catch (Exception ex) when (ex is TagLib.CorruptFileException or IOException or InvalidOperationException)
        {
            return null;
        }
    }

    private static string[] CleanValues(IReadOnlyList<string>? values)
    {
        if (values is null || values.Count == 0)
        {
            return Array.Empty<string>();
        }

        return values
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .Select(value => value.Trim())
            .ToArray();
    }

    private static string DetectImageMime(ReadOnlySpan<byte> data)
    {
        if (data.Length >= PngMagic.Length && data[..PngMagic.Length].SequenceEqual(PngMagic))
        {
            return "image/png";
        }

        if (data.Length >= JpegMagic.Length && data[..JpegMagic.Length].SequenceEqual(JpegMagic))
        {
            return "image/jpeg";
        }

        if (data.Length >= GifMagic.Length && data[..GifMagic.Length].SequenceEqual(GifMagic))
        {
            return "image/gif";
        }

        if (data.Length >= BmpMagic.Length && data[..BmpMagic.Length].SequenceEqual(BmpMagic))
        {
            return "image/bmp";
        }

        if (data.Length >= 12
            && data[..4].SequenceEqual("RIFF"u8)
            && data.Slice(8, 4).SequenceEqual("WEBP"u8))
        {
            return "image/webp";
        }

        return "image/jpeg";
    }

    private static bool IsRecoverableReadError(Exception exception) =>
        exception is TagLib.CorruptFileException
            or TagLib.UnsupportedFormatException
            or IOException
            or UnauthorizedAccessException
            or ArgumentException
            or NotSupportedException
            or InvalidOperationException;
}
