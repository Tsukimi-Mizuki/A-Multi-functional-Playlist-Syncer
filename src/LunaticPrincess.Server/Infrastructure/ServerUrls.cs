using System.Globalization;

namespace LunaticPrincess.Server.Infrastructure;

/// <summary>解析 --urls / ASPNETCORE_URLS，只保留端口并强制绑定 127.0.0.1。</summary>
public static class ServerUrls
{
    public const string DefaultBaseUrl = "http://127.0.0.1:18400";

    public static string Resolve(string[] args)
    {
        var raw = ReadUrlsArgument(args) ?? Environment.GetEnvironmentVariable("ASPNETCORE_URLS");
        if (string.IsNullOrWhiteSpace(raw))
        {
            return DefaultBaseUrl;
        }

        var urls = new List<string>();
        foreach (var part in raw.Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            if (!Uri.TryCreate(part, UriKind.Absolute, out var uri) || uri.Port <= 0)
            {
                continue;
            }

            var url = "http://127.0.0.1:" + uri.Port.ToString(CultureInfo.InvariantCulture);
            if (!urls.Contains(url, StringComparer.Ordinal))
            {
                urls.Add(url);
            }
        }

        return urls.Count == 0 ? DefaultBaseUrl : string.Join(";", urls);
    }

    public static string FirstBaseUrl(string urls) => urls.Split(';')[0].TrimEnd('/');

    private static string? ReadUrlsArgument(string[] args)
    {
        for (var i = 0; i < args.Length; i++)
        {
            var arg = args[i];
            if (arg.StartsWith("--urls=", StringComparison.OrdinalIgnoreCase))
            {
                return arg["--urls=".Length..];
            }

            if (string.Equals(arg, "--urls", StringComparison.OrdinalIgnoreCase) && i + 1 < args.Length)
            {
                return args[i + 1];
            }
        }

        return null;
    }
}
