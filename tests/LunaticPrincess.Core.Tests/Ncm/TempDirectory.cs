namespace LunaticPrincess.Core.Tests;

/// <summary>测试用临时目录，Dispose 时递归删除。</summary>
internal sealed class TempDirectory : IDisposable
{
    public TempDirectory(string? prefix = null)
    {
        Path = System.IO.Path.Combine(
            System.IO.Path.GetTempPath(),
            "nmm-core-tests",
            $"{prefix ?? "tmp"}-{Guid.NewGuid():N}");
        Directory.CreateDirectory(Path);
    }

    public string Path { get; }

    public string File(string fileName) => System.IO.Path.Combine(Path, fileName);

    public string Dir(string directoryName)
    {
        var path = System.IO.Path.Combine(Path, directoryName);
        Directory.CreateDirectory(path);
        return path;
    }

    public void Dispose()
    {
        try
        {
            if (Directory.Exists(Path))
            {
                Directory.Delete(Path, recursive: true);
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            // 测试清理失败不影响断言。
        }
    }
}
