using System.Text.Json.Serialization;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Settings;

namespace LunaticPrincess.Server.Contracts;

public sealed record ErrorResponse(ErrorBody Error);

public sealed record ErrorBody(
    string Message,
    [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Code = null);

public sealed record HealthDto(bool Ok, string Version);

public sealed record JobStartDto(string JobId);

public sealed record ProgressDto(int Completed, int Total, string Stage, string Item, string? Message = null, double? Percent = null);

public sealed record JobStateDto(string Id, string State, ProgressDto? Progress, object? Result, string? Error);

public sealed record LogEntryDto(string Time, string Level, string Message);

public sealed record UserDto(long UserId, string Nickname, string? AvatarUrl, int VipType);

public sealed record SessionDto(bool LoggedIn, UserDto? User);

public sealed record QrLoginStartDto(string Key, string QrContent, DateTimeOffset ExpiresAt);

public sealed record QrLoginStatusDto(string State, string Message, UserDto? User);

public sealed record ArtistDto(long Id, string Name);

public sealed record SongDto(
    long Id,
    string Name,
    string DisplayName,
    string ArtistText,
    IReadOnlyList<ArtistDto> Artists,
    long AlbumId,
    string AlbumName,
    string? CoverUrl,
    int DurationMs,
    int TrackNumber,
    int Fee);

public sealed record PlaylistSummaryDto(
    long Id,
    string Name,
    string? CoverUrl,
    int TrackCount,
    string? CreatorNickname,
    string? Description);

public sealed record PlaylistListDto(long UserId, IReadOnlyList<PlaylistSummaryDto> Playlists);

public sealed record PlaylistDetailDto(
    long Id,
    string Name,
    string? CoverUrl,
    string? Description,
    string? CreatorNickname,
    int TrackCount,
    bool IsPartial,
    IReadOnlyList<SongDto> Songs);

public sealed record LocalTrackDto(
    string Path,
    string Title,
    string Artist,
    string? Album,
    long DurationMs,
    string? Format,
    int? Bitrate,
    bool IsNcm,
    bool HasCover,
    long? NeteaseId);

public sealed record LibraryScanRequest(IReadOnlyList<string>? Directories);

public sealed record LibraryScanResultDto(IReadOnlyList<LocalTrackDto> Tracks);

public sealed record CompareRequest(IReadOnlyList<SongDto>? Songs, IReadOnlyList<LocalTrackDto>? Tracks);

public sealed record CompareItemDto(long SongId, string Status, string? LocalPath, string? MatchedTitle, double? Score);

public sealed record CompareResultDto(IReadOnlyList<CompareItemDto> Items);

public sealed record TransferOptionsDto(
    string? TargetDirectory,
    string? Quality,
    string? LyricsMode,
    bool? WriteCover,
    bool? OverwriteExisting,
    bool? SkipExisting,
    string? FileNameTemplate,
    int? MaxConcurrency);

public sealed record DownloadRequest(IReadOnlyList<SongDto>? Songs, TransferOptionsDto? Options);

public sealed record DownloadItemDto(
    long SongId,
    string SongName,
    bool Success,
    string? OutputPath,
    string? ErrorMessage,
    bool DecryptedFromNcm,
    bool Skipped);

public sealed record DownloadSummaryDto(
    int SuccessCount,
    int SkippedCount,
    int FailedCount,
    IReadOnlyList<DownloadItemDto> Results);

public sealed record SyncItemRequest(SongDto? Song, string? LocalPath);

public sealed record SyncRequestDto(IReadOnlyList<SyncItemRequest>? Items, TransferOptionsDto? Options);

public sealed record SyncItemDto(
    long SongId,
    string SongName,
    bool Success,
    string Source,
    string? OutputPath,
    string? Message);

public sealed record SyncSummaryDto(
    int SuccessCount,
    int FailedCount,
    int DownloadedCount,
    int DecryptedCount,
    int SkippedCount,
    IReadOnlyList<SyncItemDto> Results);

public sealed record NcmScanRequest(IReadOnlyList<string>? Directories);

public sealed record NcmScanFileDto(
    string Path,
    string? Title,
    string? Artist,
    string? Album,
    long DurationMs,
    string? Format,
    int Bitrate,
    string? CoverUrl);

public sealed record NcmScanResultDto(IReadOnlyList<NcmScanFileDto> Files);

public sealed record NcmMatchOptionsDto(
    string? TagSourcePriority,
    string? LyricsSourcePriority,
    bool? OverwriteExisting,
    bool? FetchLyrics,
    bool? PreferWordLyrics,
    double? MatchThreshold);

public sealed record NcmImportRequest(
    IReadOnlyList<string>? Files,
    string? OutputDirectory,
    bool? Overwrite,
    bool? AutoMatchOnline,
    NcmMatchOptionsDto? MatchOptions);

public sealed record NcmImportItemDto(
    string SourcePath,
    string? OutputPath,
    bool Success,
    string? Format,
    string? ErrorMessage,
    string? EnrichmentMessage);

public sealed record NcmImportResultDto(
    IReadOnlyList<NcmImportItemDto> Results,
    int SuccessCount,
    int FailedCount);

public sealed record TagInfoDto(
    string Path,
    string? Title,
    string? Artist,
    string? Album,
    string? AlbumArtist,
    string? Year,
    string? TrackNumber,
    string? Genre,
    string? Composer,
    string? Comment,
    long DurationMs,
    int Bitrate,
    string? Format,
    bool HasCover,
    string? CoverBase64,
    string? Lyrics,
    int LyricsLength);

public sealed record TagWriteDto(
    string? Title,
    string? Artist,
    string? Album,
    string? AlbumArtist,
    string? Year,
    string? TrackNumber,
    string? Genre,
    string? Composer,
    string? Comment,
    long? NeteaseId);

public sealed record TagReadRequest(string? Path);

public sealed record TagWriteRequest(string? Path, TagWriteDto? Tags, string? Lyrics, string? CoverBase64);

public sealed record OkDto(bool Ok);

public sealed record LyricsDto(string? Lrc, string? TranslatedLrc, string? WordLrc);

public sealed record LyricsMatchDto(
    OnlineMusicSource Source,
    string SongId,
    string Title,
    string Artist,
    string Album,
    long DurationMs,
    double Score,
    string? CoverUrl,
    LyricsDto Lyrics);

public sealed record LyricsSearchRequest(
    string? Title,
    string? Artist,
    string? Album,
    long DurationMs,
    string? Source,
    bool? PreferWordLyrics,
    double? MatchThreshold);

public sealed record LyricsSearchResultDto(IReadOnlyList<LyricsMatchDto> Matches);

public sealed record BatchLyricsScanRequest(string? Directory);

public sealed record BatchLyricsScanResultDto(IReadOnlyList<string> Files);

public sealed record BatchLyricsOptionsDto(
    string? TagSourcePriority,
    string? LyricsSourcePriority,
    bool? OverwriteExisting,
    bool? FetchLyrics,
    bool? PreferWordLyrics,
    double? MatchThreshold);

public sealed record BatchLyricsRunRequest(
    string? Directory,
    IReadOnlyList<string>? Files,
    BatchLyricsOptionsDto? Options);

public sealed record BatchLyricsItemDto(
    string Path,
    bool Success,
    string? SourceLabel,
    string? Message,
    string? MatchedTitle,
    string? MatchedArtist);

public sealed record BatchLyricsResultDto(IReadOnlyList<BatchLyricsItemDto> Items);

public sealed record FsEntryDto(string Name, string Path);

public sealed record FsFileDto(string Name, string Path, long Size);

public sealed record FsBrowseDto(
    string Path,
    string? Parent,
    IReadOnlyList<FsEntryDto> Directories,
    IReadOnlyList<FsFileDto> AudioFiles);

public sealed record AppSettingsDto(
    string DownloadDirectory,
    string LibraryDirectory,
    AudioQuality Quality,
    LyricsWriteMode LyricsMode,
    bool WriteCover,
    bool OverwriteExisting,
    bool SkipExisting,
    string FileNameTemplate,
    int MaxConcurrency,
    bool AutoMatchLocal,
    OnlineSourcePriority TagSourcePriority,
    OnlineSourcePriority LyricsSourcePriority,
    string[] NcmImportFolders,
    string NcmImportOutputDirectory,
    OnlineMusicSource LyricsEditorSearchSource,
    bool LyricsEditorPreferWordLyrics,
    double MatchThreshold,
    ThemeMode Theme,
    RenderFrameRateMode RenderFrameRate);

public sealed record AppSettingsUpdateDto(
    string? DownloadDirectory,
    string? LibraryDirectory,
    string? Quality,
    string? LyricsMode,
    bool? WriteCover,
    bool? OverwriteExisting,
    bool? SkipExisting,
    string? FileNameTemplate,
    int? MaxConcurrency,
    bool? AutoMatchLocal,
    string? TagSourcePriority,
    string? LyricsSourcePriority,
    string[]? NcmImportFolders,
    string? NcmImportOutputDirectory,
    string? LyricsEditorSearchSource,
    bool? LyricsEditorPreferWordLyrics,
    double? MatchThreshold,
    string? Theme,
    string? RenderFrameRate);
