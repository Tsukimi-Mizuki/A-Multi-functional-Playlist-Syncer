namespace LunaticPrincess.Server.Infrastructure;

/// <summary>收集 .ncm 文件：支持传入目录（递归）、单个文件，去重并排序。</summary>
public static class NcmScanner
{
    public static IReadOnlyList<string> Collect(IEnumerable<string>? paths)
    {
        var result = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        if (paths is null)
        {
            return result;
        }

        foreach (var raw in paths)
        {
            if (string.IsNullOrWhiteSpace(raw))
            {
                continue;
            }

            var path = raw.Trim();
            if (File.Exists(path))
            {
                if (IsNcm(path) && seen.Add(Path.GetFullPath(path)))
                {
                    result.Add(Path.GetFullPath(path));
                }

                continue;
            }

            if (!Directory.Exists(path))
            {
                continue;
            }

            try
            {
                foreach (var file in Directory.EnumerateFiles(path, "*.ncm", SearchOption.AllDirectories))
                {
                    if (seen.Add(file))
                    {
                        result.Add(file);
                    }
                }
            }
            catch (UnauthorizedAccessException)
            {
            }
            catch (IOException)
            {
            }
        }

        result.Sort(StringComparer.OrdinalIgnoreCase);
        return result;
    }

    private static bool IsNcm(string path)
        => string.Equals(Path.GetExtension(path), ".ncm", StringComparison.OrdinalIgnoreCase);
}
