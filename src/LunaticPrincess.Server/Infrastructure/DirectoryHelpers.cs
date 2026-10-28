namespace LunaticPrincess.Server.Infrastructure;

public static class DirectoryHelpers
{
    /// <summary>创建目录，失败时转成 400 的 ApiException，避免裸 IOException 变成 500。</summary>
    public static void EnsureDirectory(string? path, string label)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            throw new ApiException(StatusCodes.Status400BadRequest, label + "不能为空", "invalid_request");
        }

        try
        {
            Directory.CreateDirectory(path);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or NotSupportedException or ArgumentException)
        {
            throw new ApiException(
                StatusCodes.Status400BadRequest,
                "无法创建" + label + "：" + path + "（" + ex.Message + "）",
                "invalid_directory");
        }
    }
}
