namespace LunaticPrincess.Core.Utilities;

/// <summary>
/// 音频文件类型判定（歌词编辑器拖放载入、文件对话框过滤共用）。
/// 与下载用的 <c>MediaFileHelper</c> 分开：这里只关心“能不能当作可编辑音频读入”。
/// </summary>
public static class AudioFileTypes
{
    /// <summary>歌词编辑器接受的扩展名（不含 .ncm：NCM 需先解密）。</summary>
    public static IReadOnlyList<string> SupportedExtensions { get; } =
    [
        ".mp3", ".flac", ".m4a", ".mp4", ".ogg", ".opus", ".wav", ".aac", ".wma",
    ];

    /// <summary>给用户看的一行格式说明。</summary>
    public const string ExtensionHint = "MP3 / FLAC / M4A / MP4 / OGG / OPUS / WAV / AAC / WMA";

    private static readonly HashSet<string> ExtensionSet =
        new(SupportedExtensions, StringComparer.OrdinalIgnoreCase);

    public const string EmptyTargetMessage = "没有识别到拖入的文件路径。";
    public const string NoAudioInFolderMessage = "该文件夹里没有可编辑的音频文件。";

    /// <summary>扩展名是否可编辑（只看扩展名，不读文件内容）。</summary>
    public static bool IsSupported(string? path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return false;
        }

        var extension = Path.GetExtension(path.Trim());
        return extension.Length > 0 && ExtensionSet.Contains(extension);
    }

    /// <summary>
    /// 把拖放目标解析成可载入的音频文件：文件直接返回；文件夹返回其中按名称排序的第一个受支持文件。
    /// </summary>
    /// <param name="target">拖入的路径（文件或文件夹），可以为空。</param>
    /// <param name="path">解析出的音频文件完整路径。</param>
    /// <param name="error">解析失败时给用户看的中文提示。</param>
    public static bool TryResolve(string? target, out string? path, out string? error)
    {
        path = null;
        error = null;
        if (string.IsNullOrWhiteSpace(target))
        {
            error = EmptyTargetMessage;
            return false;
        }

        var candidate = target.Trim();
        if (File.Exists(candidate))
        {
            if (!IsSupported(candidate))
            {
                var extension = Path.GetExtension(candidate);
                error = extension.Length == 0
                    ? $"暂不支持无扩展名的文件，请拖入 {ExtensionHint} 格式的音频文件。"
                    : $"暂不支持「{extension}」格式，请拖入 {ExtensionHint} 格式的音频文件。";
                return false;
            }

            path = candidate;
            return true;
        }

        if (Directory.Exists(candidate))
        {
            var first = ListSupportedFiles(candidate).FirstOrDefault();
            if (first is null)
            {
                error = NoAudioInFolderMessage;
                return false;
            }

            path = first;
            return true;
        }

        error = $"找不到「{candidate}」。";
        return false;
    }

    /// <summary>列出目录中可编辑的音频文件（只看当前层级，按文件名排序）。</summary>
    public static IReadOnlyList<string> ListSupportedFiles(string? directory)
    {
        if (string.IsNullOrWhiteSpace(directory) || !Directory.Exists(directory))
        {
            return Array.Empty<string>();
        }

        try
        {
            return Directory
                .EnumerateFiles(directory)
                .Where(IsSupported)
                .OrderBy(file => Path.GetFileName(file), StringComparer.OrdinalIgnoreCase)
                .ToArray();
        }
        catch (IOException)
        {
            return Array.Empty<string>();
        }
        catch (UnauthorizedAccessException)
        {
            return Array.Empty<string>();
        }
    }
}
