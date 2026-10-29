using System.Security;
using System.Text.RegularExpressions;

namespace LunaticPrincess.Core.Utilities;

/// <summary>
/// 把底层异常翻译成用户能直接读懂的中文提示。
/// Windows 的 <see cref="UnauthorizedAccessException"/> 原始信息是
/// "Access to the path 'D:\\music\\x.part' is denied."，直接展示既没有中文也不告诉用户该怎么办，
/// 这里统一换成「哪个路径 + 出了什么问题 + 怎么解决」的表述。
/// 无法识别的异常仍然回退到原始 Message，保证不吞掉信息。
/// </summary>
public static class ErrorText
{
    // Win32 / HRESULT 错误码（多数 .NET IO 异常会把它放在 HResult 里）。
    private const int AccessDeniedHResult = unchecked((int)0x80070005); // ERROR_ACCESS_DENIED
    private const int SharingViolationHResult = unchecked((int)0x80070020); // ERROR_SHARING_VIOLATION
    private const int DiskFullHResult = unchecked((int)0x80070070); // ERROR_DISK_FULL
    private const int PathTooLongHResult = unchecked((int)0x800700CE); // ERROR_FILENAME_EXCED_RANGE

    private const string WritePermissionHint = "请检查该文件夹的权限，或以管理员身份运行本程序后重试。";
    private const string ReadPermissionHint = "请检查该文件或文件夹的权限，或以管理员身份运行本程序后重试。";

    /// <summary>从异常信息里提取被拒绝访问的路径（支持英文与简繁中文的系统提示）。</summary>
    private static readonly Regex QuotedPathPattern = new(
        @"(?:'|\u201C|\u300C)(?<path>[A-Za-z]:[^'\u201D\u300D\r\n]+)(?:'|\u201D|\u300D)",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    /// <summary>是否为权限类错误（无写入权限、ACL 拒绝、安全异常等）。</summary>
    public static bool IsPermissionError(Exception exception) =>
        exception is UnauthorizedAccessException or SecurityException ||
        exception.HResult == AccessDeniedHResult ||
        HasAnyText(exception.Message,
            "is denied",
            "access is denied",
            "拒绝访问",
            "拒絕存取",
            "存取遭拒",
            "访问被拒绝",
            "存取被拒");

    /// <summary>是否为磁盘空间不足。</summary>
    public static bool IsDiskFull(Exception exception) =>
        exception.HResult == DiskFullHResult ||
        HasAnyText(exception.Message, "not enough space", "disk full", "磁盘空间不足", "磁碟空間不足", "空間不足");

    /// <summary>是否为文件被其他程序占用。</summary>
    public static bool IsFileInUse(Exception exception) =>
        exception.HResult == SharingViolationHResult ||
        HasAnyText(exception.Message,
            "being used by another process",
            "另一个程序正在使用",
            "正在使用此文件",
            "被另一个进程使用",
            "另一個程序正在使用");

    /// <summary>是否为路径过长。</summary>
    public static bool IsPathTooLong(Exception exception) =>
        exception is PathTooLongException || exception.HResult == PathTooLongHResult;

    /// <summary>是否属于本类能给出明确中文说明的 IO 错误。</summary>
    public static bool IsKnownIoError(Exception exception) =>
        IsPermissionError(exception) || IsDiskFull(exception) || IsFileInUse(exception) || IsPathTooLong(exception);

    /// <summary>
    /// 写入场景的提示：优先使用调用方已知的目标目录（比从异常信息里猜路径更准确）。
    /// </summary>
    public static string DescribeWrite(Exception exception, string? targetDirectory = null)
    {
        if (!IsPermissionError(exception))
        {
            return Describe(exception);
        }

        var path = NullIfWhite(targetDirectory) ?? TryGetPath(exception);
        return path is null
            ? $"无法写入目标文件夹：没有写入权限，{WritePermissionHint}"
            : $"无法写入「{path}」：没有写入权限，{WritePermissionHint}";
    }

    /// <summary>把异常转成中文提示；无法识别时回退到原始 Message。</summary>
    public static string Describe(Exception exception)
    {
        if (exception is null)
        {
            return string.Empty;
        }

        var path = TryGetPath(exception);
        if (IsPermissionError(exception))
        {
            return path is null
                ? $"权限不足：无法访问目标文件，{ReadPermissionHint}"
                : $"无法访问「{path}」：权限不足，{ReadPermissionHint}";
        }

        if (IsDiskFull(exception))
        {
            return path is null
                ? "磁盘空间不足，请清理磁盘后重试。"
                : $"磁盘空间不足，无法写入「{path}」，请清理该磁盘后重试。";
        }

        if (IsFileInUse(exception))
        {
            return path is null
                ? "文件正被其他程序占用，请关闭占用它的程序后重试。"
                : $"文件「{path}」正被其他程序占用，请关闭占用它的程序后重试。";
        }

        if (IsPathTooLong(exception))
        {
            return "文件路径过长，请把文件放到更短的目录，或缩短文件名后重试。";
        }

        return string.IsNullOrWhiteSpace(exception.Message) ? exception.GetType().Name : exception.Message;
    }

    /// <summary>尽量从异常信息里取出路径；取不到返回 null。</summary>
    public static string? TryGetPath(Exception? exception)
    {
        var message = exception?.Message;
        if (string.IsNullOrWhiteSpace(message))
        {
            return null;
        }

        var match = QuotedPathPattern.Match(message);
        if (match.Success)
        {
            return match.Groups["path"].Value.Trim();
        }

        // 有些本地化提示把路径放在「」之外或用双引号；再退一步找带盘符的片段。
        var index = message.IndexOf(":\\", StringComparison.Ordinal);
        if (index > 0)
        {
            var start = index - 1;
            while (start > 0 && char.IsLetter(message[start - 1]))
            {
                start--;
            }

            var end = start;
            while (end < message.Length && !char.IsWhiteSpace(message[end]) && message[end] is not ('\'' or '\u201D' or '\u300D' or '"'))
            {
                end++;
            }

            if (end - start > 3)
            {
                return message[start..end].Trim();
            }
        }

        return null;
    }

    private static bool HasAnyText(string? message, params string[] needles)
    {
        if (string.IsNullOrWhiteSpace(message))
        {
            return false;
        }

        foreach (var needle in needles)
        {
            if (message.Contains(needle, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }

        return false;
    }

    private static string? NullIfWhite(string? value) => string.IsNullOrWhiteSpace(value) ? null : value.Trim();
}
