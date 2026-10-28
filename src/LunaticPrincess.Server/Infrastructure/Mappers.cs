using System.Globalization;
using LunaticPrincess.Core.Download;
using LunaticPrincess.Core.Library;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Settings;
using LunaticPrincess.Core.Sync;
using LunaticPrincess.Core.Tags;
using LunaticPrincess.Server.Contracts;

namespace LunaticPrincess.Server.Infrastructure;

/// <summary>Core 领域模型与 HTTP DTO 之间的映射。</summary>
public static class Mappers
{
    public static UserDto ToDto(this NeteaseUser user)
        => new(user.UserId, user.Nickname ?? "", user.AvatarUrl, user.VipType);

    public static UserDto ToDto(this NeteaseSession session)
        => new(session.UserId, session.Nickname ?? "", session.AvatarUrl, session.VipType);

    public static ArtistDto ToDto(this ArtistRef artist) => new(artist.Id, artist.Name ?? "");

    public static SongDto ToDto(this NeteaseSong song) => new(
        song.Id,
        song.Name ?? "",
        song.DisplayName,
        song.ArtistText,
        song.Artists is null ? Array.Empty<ArtistDto>() : song.Artists.Select(a => a.ToDto()).ToArray(),
        song.Album?.Id ?? 0,
        song.Album?.Name ?? "",
        song.CoverUrl,
        song.DurationMs,
        song.TrackNumber,
        song.Fee);

    public static NeteaseSong ToDomain(this SongDto? dto)
    {
        if (dto is null)
        {
            throw new ApiException(StatusCodes.Status400BadRequest, "歌曲信息不能为空", "invalid_request");
        }

        var artists = dto.Artists is null
            ? Array.Empty<ArtistRef>()
            : dto.Artists.Select(a => new ArtistRef(a.Id, a.Name ?? "")).ToArray();
        return new NeteaseSong
        {
            Id = dto.Id,
            Name = dto.Name ?? "",
            Artists = artists,
            Album = new AlbumRef(dto.AlbumId, dto.AlbumName ?? "", dto.CoverUrl),
            DurationMs = dto.DurationMs,
            TrackNumber = dto.TrackNumber,
            Fee = dto.Fee,
        };
    }

    public static PlaylistSummaryDto ToDto(this PlaylistSummary playlist)
        => new(playlist.Id, playlist.Name ?? "", playlist.CoverUrl, playlist.TrackCount, playlist.CreatorNickname, playlist.Description);

    public static PlaylistDetailDto ToDto(this PlaylistDetail detail) => new(
        detail.Id,
        detail.Name ?? "",
        detail.CoverUrl,
        detail.Description,
        detail.CreatorNickname,
        detail.TrackCount,
        detail.IsPartial,
        detail.Songs is null ? Array.Empty<SongDto>() : detail.Songs.Select(s => s.ToDto()).ToArray());

    public static LocalTrackDto ToDto(this LocalTrack track) => new(
        track.Path,
        track.Title ?? "",
        track.Artist ?? "",
        track.Album,
        track.DurationMs,
        track.Format,
        track.Bitrate,
        track.IsNcm,
        track.HasCover,
        track.NeteaseId);

    public static LocalTrack ToDomain(this LocalTrackDto? dto)
    {
        if (dto is null)
        {
            throw new ApiException(StatusCodes.Status400BadRequest, "本地曲目信息不能为空", "invalid_request");
        }

        var path = dto.Path ?? "";
        return new LocalTrack
        {
            Path = path,
            Title = dto.Title ?? "",
            Artist = dto.Artist,
            Album = dto.Album,
            DurationMs = dto.DurationMs,
            Format = dto.Format,
            Bitrate = dto.Bitrate,
            IsNcm = dto.IsNcm || string.Equals(Path.GetExtension(path), ".ncm", StringComparison.OrdinalIgnoreCase),
            HasCover = dto.HasCover,
            NeteaseId = dto.NeteaseId,
        };
    }

    public static DownloadItemDto ToDto(this DownloadItemResult result) => new(
        result.SongId,
        result.SongName ?? "",
        result.Success,
        result.OutputPath,
        result.ErrorMessage,
        result.DecryptedFromNcm,
        result.Skipped);

    public static DownloadSummaryDto ToDto(this DownloadSummary summary) => new(
        summary.SuccessCount,
        summary.SkippedCount,
        summary.FailedCount,
        summary.Results is null ? Array.Empty<DownloadItemDto>() : summary.Results.Select(r => r.ToDto()).ToArray());

    public static SyncItemDto ToDto(this SyncItemResult result) => new(
        result.SongId,
        result.SongName ?? "",
        result.Success,
        result.Source switch
        {
            SyncSourceKind.Downloaded => "Downloaded",
            SyncSourceKind.DecryptedNcm => "DecryptedNcm",
            SyncSourceKind.Skipped => "Skipped",
            _ => "Failed",
        },
        result.OutputPath,
        result.Message);

    public static SyncSummaryDto ToDto(this SyncSummary summary) => new(
        summary.SuccessCount,
        summary.FailedCount,
        summary.DownloadedCount,
        summary.DecryptedCount,
        summary.SkippedCount,
        summary.Results is null ? Array.Empty<SyncItemDto>() : summary.Results.Select(r => r.ToDto()).ToArray());

    public static NcmImportItemDto ToDto(this NcmDecryptResult result) => new(
        result.SourcePath,
        result.OutputPath,
        result.Success,
        result.Format,
        result.ErrorMessage,
        result.EnrichmentMessage);

    public static TagInfoDto ToTagInfoDto(this AudioTagInfo info, string path) => new(
        path,
        info.Title,
        info.Artist,
        info.Album,
        info.AlbumArtist,
        info.Year?.ToString(CultureInfo.InvariantCulture),
        info.TrackNumber?.ToString(CultureInfo.InvariantCulture),
        info.Genre,
        info.Composer,
        info.Comment,
        info.DurationMs,
        info.Bitrate,
        info.FileFormat,
        info.HasCover,
        info.CoverData is { Length: > 0 } cover ? Convert.ToBase64String(cover) : null,
        info.Lyrics,
        info.Lyrics?.Length ?? 0);

    public static AudioTagData ToAudioTagData(this TagWriteDto? dto, AudioTagInfo? existing, string? lyrics, string? coverBase64)
    {
        var tags = dto ?? new TagWriteDto(null, null, null, null, null, null, null, null, null, null);
        return new AudioTagData
        {
            ReplaceAll = true,
            Title = BlankToNull(tags.Title),
            Artists = SplitArtists(tags.Artist),
            Album = BlankToNull(tags.Album),
            AlbumArtist = BlankToNull(tags.AlbumArtist),
            Year = ParseUInt(tags.Year),
            TrackNumber = ParseUInt(tags.TrackNumber),
            Genre = BlankToNull(tags.Genre),
            Composer = BlankToNull(tags.Composer),
            Comment = BlankToNull(tags.Comment),
            PlainLyrics = lyrics is null ? existing?.Lyrics : BlankToNull(lyrics),
            CoverData = coverBase64 is null ? existing?.CoverData : DecodeBase64(coverBase64),
            NeteaseId = tags.NeteaseId,
        };
    }

    public static AppSettingsDto ToDto(this AppSettings settings) => new(
        settings.DownloadDirectory,
        settings.LibraryDirectory,
        settings.Quality,
        settings.LyricsMode,
        settings.WriteCover,
        settings.OverwriteExisting,
        settings.SkipExisting,
        settings.FileNameTemplate,
        settings.MaxConcurrency,
        settings.AutoMatchLocal,
        settings.TagSourcePriority,
        settings.LyricsSourcePriority,
        settings.NcmImportFolders is null ? Array.Empty<string>() : settings.NcmImportFolders.ToArray(),
        settings.NcmImportOutputDirectory,
        settings.LyricsEditorSearchSource,
        settings.LyricsEditorPreferWordLyrics,
        settings.MatchThreshold,
        settings.Theme,
        settings.RenderFrameRate);

    /// <summary>把 PUT /api/settings 的部分字段合并进当前设置（null 表示不变），随后由 Normalize 兜底。</summary>
    public static void ApplyPatch(this AppSettings settings, AppSettingsUpdateDto? patch)
    {
        if (patch is null)
        {
            return;
        }

        if (patch.DownloadDirectory is not null) settings.DownloadDirectory = patch.DownloadDirectory;
        if (patch.LibraryDirectory is not null) settings.LibraryDirectory = patch.LibraryDirectory;
        if (patch.Quality is not null) settings.Quality = ParseEnumOrThrow<AudioQuality>(patch.Quality, "quality");
        if (patch.LyricsMode is not null) settings.LyricsMode = ParseEnumOrThrow<LyricsWriteMode>(patch.LyricsMode, "lyricsMode");
        if (patch.WriteCover is not null) settings.WriteCover = patch.WriteCover.Value;
        if (patch.OverwriteExisting is not null) settings.OverwriteExisting = patch.OverwriteExisting.Value;
        if (patch.SkipExisting is not null) settings.SkipExisting = patch.SkipExisting.Value;
        if (patch.FileNameTemplate is not null) settings.FileNameTemplate = patch.FileNameTemplate;
        if (patch.MaxConcurrency is not null) settings.MaxConcurrency = patch.MaxConcurrency.Value;
        if (patch.AutoMatchLocal is not null) settings.AutoMatchLocal = patch.AutoMatchLocal.Value;
        if (patch.TagSourcePriority is not null)
        {
            settings.TagSourcePriority = ParseEnumOrThrow<OnlineSourcePriority>(patch.TagSourcePriority, "tagSourcePriority");
        }

        if (patch.LyricsSourcePriority is not null)
        {
            settings.LyricsSourcePriority = ParseEnumOrThrow<OnlineSourcePriority>(patch.LyricsSourcePriority, "lyricsSourcePriority");
        }

        if (patch.NcmImportFolders is not null) settings.NcmImportFolders = patch.NcmImportFolders.ToList();
        if (patch.NcmImportOutputDirectory is not null) settings.NcmImportOutputDirectory = patch.NcmImportOutputDirectory;
        if (patch.LyricsEditorSearchSource is not null)
        {
            settings.LyricsEditorSearchSource = ParseEnumOrThrow<OnlineMusicSource>(patch.LyricsEditorSearchSource, "lyricsEditorSearchSource");
        }

        if (patch.LyricsEditorPreferWordLyrics is not null) settings.LyricsEditorPreferWordLyrics = patch.LyricsEditorPreferWordLyrics.Value;
        if (patch.MatchThreshold is not null) settings.MatchThreshold = patch.MatchThreshold.Value;
        if (patch.Theme is not null) settings.Theme = ParseEnumOrThrow<ThemeMode>(patch.Theme, "theme");
        if (patch.RenderFrameRate is not null) settings.RenderFrameRate = ParseEnumOrThrow<RenderFrameRateMode>(patch.RenderFrameRate, "renderFrameRate");
    }

    public static DownloadOptions BuildDownloadOptions(TransferOptionsDto? options, AppSettings settings)
    {
        var targetDirectory = string.IsNullOrWhiteSpace(options?.TargetDirectory)
            ? settings.DownloadDirectory
            : options!.TargetDirectory!;
        var template = string.IsNullOrWhiteSpace(options?.FileNameTemplate)
            ? settings.FileNameTemplate
            : options!.FileNameTemplate!;
        var concurrency = options?.MaxConcurrency is > 0
            ? Math.Clamp(options.MaxConcurrency.Value, 1, 8)
            : settings.MaxConcurrency;
        return new DownloadOptions
        {
            TargetDirectory = targetDirectory,
            Quality = ParseEnumOrDefault(options?.Quality, settings.Quality, "quality"),
            LyricsMode = ParseEnumOrDefault(options?.LyricsMode, settings.LyricsMode, "lyricsMode"),
            WriteCover = options?.WriteCover ?? settings.WriteCover,
            OverwriteExisting = options?.OverwriteExisting ?? settings.OverwriteExisting,
            SkipExisting = options?.SkipExisting ?? settings.SkipExisting,
            FileNameTemplate = template,
            MaxConcurrency = concurrency,
        };
    }

    public static BatchLyricsOptions BuildBatchLyricsOptions(
        string? tagPriority,
        string? lyricsPriority,
        bool? overwriteExisting,
        bool? fetchLyrics,
        bool? preferWordLyrics,
        double? matchThreshold,
        AppSettings settings)
        => new(
            TagPriority: ParseEnumOrDefault(tagPriority, settings.TagSourcePriority, "tagSourcePriority"),
            LyricsPriority: ParseEnumOrDefault(lyricsPriority, settings.LyricsSourcePriority, "lyricsSourcePriority"),
            OverwriteExisting: overwriteExisting ?? true,
            FetchLyrics: fetchLyrics ?? (settings.LyricsMode != LyricsWriteMode.None),
            PreferWordLyrics: preferWordLyrics ?? (settings.LyricsMode == LyricsWriteMode.Word),
            MinScore: ClampThreshold(matchThreshold ?? settings.MatchThreshold));

    public static double ClampThreshold(double value)
    {
        if (double.IsNaN(value) || double.IsInfinity(value) || value <= 0)
        {
            return LyricsMatchScorer.DefaultMinScore;
        }

        return Math.Clamp(Math.Round(value, 2), LyricsMatchScorer.MinThreshold, LyricsMatchScorer.MaxThreshold);
    }

    public static TEnum ParseEnumOrDefault<TEnum>(string? value, TEnum fallback, string fieldName) where TEnum : struct, Enum
        => string.IsNullOrWhiteSpace(value) ? fallback : ParseEnumOrThrow<TEnum>(value, fieldName);

    public static TEnum ParseEnumOrThrow<TEnum>(string value, string fieldName) where TEnum : struct, Enum
    {
        if (Enum.TryParse<TEnum>(value.Trim(), ignoreCase: true, out var parsed) && Enum.IsDefined(parsed))
        {
            return parsed;
        }

        throw new ApiException(StatusCodes.Status400BadRequest, "无效的 " + fieldName + "：" + value, "invalid_request");
    }

    private static string? BlankToNull(string? value) => string.IsNullOrWhiteSpace(value) ? null : value.Trim();

    private static IReadOnlyList<string> SplitArtists(string? artist)
    {
        if (string.IsNullOrWhiteSpace(artist))
        {
            return Array.Empty<string>();
        }

        return artist
            .Split(new[] { '/', ';', '；', '、' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
    }

    private static uint? ParseUInt(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return null;
        }

        return uint.TryParse(value.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out var parsed) ? parsed : null;
    }

    private static byte[]? DecodeBase64(string value)
    {
        var trimmed = value.Trim();
        if (trimmed.Length == 0)
        {
            return null;
        }

        try
        {
            return Convert.FromBase64String(trimmed);
        }
        catch (FormatException)
        {
            throw new ApiException(StatusCodes.Status400BadRequest, "coverBase64 不是合法的 Base64", "invalid_request");
        }
    }
}
