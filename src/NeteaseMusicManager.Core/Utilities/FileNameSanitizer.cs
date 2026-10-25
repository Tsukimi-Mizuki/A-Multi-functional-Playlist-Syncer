using System.Text;

namespace NeteaseMusicManager.Core.Utilities;

/// <summary>生成 Windows 安全文件名，并支持 {artist}/{title}/{album}/{track}/{year}/{id} 占位符。</summary>
public static class FileNameSanitizer
{
    private static readonly char[] InvalidChars = Path.GetInvalidFileNameChars();

    public static string Sanitize(string? value, int maxLength = 120)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return "未知";
        }

        var builder = new StringBuilder(value.Length);
        foreach (var c in value)
        {
            if (Array.IndexOf(InvalidChars, c) >= 0 || char.IsControl(c))
            {
                builder.Append('_');
            }
            else
            {
                builder.Append(c);
            }
        }

        var result = builder.ToString().Trim().TrimEnd('.');
        if (result.Length == 0)
        {
            result = "未知";
        }

        return result.Length <= maxLength ? result : result[..maxLength].TrimEnd();
    }

    public static string BuildFileName(
        string template,
        string? title,
        string? artist,
        string? album,
        int trackNumber,
        int year,
        long songId,
        string extension)
    {
        var safeTemplate = string.IsNullOrWhiteSpace(template) ? "{artist} - {title}" : template;
        var fileName = safeTemplate
            .Replace("{title}", title ?? "未知歌曲", StringComparison.OrdinalIgnoreCase)
            .Replace("{artist}", artist ?? "未知歌手", StringComparison.OrdinalIgnoreCase)
            .Replace("{album}", album ?? "未知专辑", StringComparison.OrdinalIgnoreCase)
            .Replace("{track}", trackNumber > 0 ? trackNumber.ToString("00") : string.Empty, StringComparison.OrdinalIgnoreCase)
            .Replace("{year}", year > 0 ? year.ToString() : string.Empty, StringComparison.OrdinalIgnoreCase)
            .Replace("{id}", songId.ToString(), StringComparison.OrdinalIgnoreCase);

        fileName = Sanitize(fileName.Trim(), 180);
        var dot = extension.StartsWith('.') ? extension : "." + extension;
        return fileName.EndsWith(dot, StringComparison.OrdinalIgnoreCase) ? fileName : fileName + dot;
    }
}
