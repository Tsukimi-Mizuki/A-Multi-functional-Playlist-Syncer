using System.Collections.Concurrent;
using Avalonia.Media.Imaging;

namespace NeteaseMusicManager.Desktop.Services;

/// <summary>
/// 远程封面/头像加载与内存缓存。纯装饰性资源：任何失败都静默返回 null，不向 UI 抛异常，
/// 也不会阻塞调用方所在的 UI 线程（解码在后台线程完成，Bitmap 由 UI 绑定持有）。
/// </summary>
public sealed class CoverImageService
{
    private const int MaxParallelRequests = 4;

    private readonly HttpClient _http;
    private readonly ConcurrentDictionary<string, Task<Bitmap?>> _cache = new(StringComparer.Ordinal);
    private readonly SemaphoreSlim _gate = new(MaxParallelRequests, MaxParallelRequests);

    public CoverImageService(HttpClient http) => _http = http;

    public Task<Bitmap?> LoadAsync(string? url)
    {
        if (string.IsNullOrWhiteSpace(url))
        {
            return Task.FromResult<Bitmap?>(null);
        }

        return _cache.GetOrAdd(url, LoadCoreAsync);
    }

    private async Task<Bitmap?> LoadCoreAsync(string url)
    {
        await _gate.WaitAsync().ConfigureAwait(false);
        try
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, url);
            request.Headers.TryAddWithoutValidation(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36");
            request.Headers.TryAddWithoutValidation("Referer", "https://music.163.com/");
            using var response = await _http
                .SendAsync(request, HttpCompletionOption.ResponseHeadersRead)
                .ConfigureAwait(false);
            if (!response.IsSuccessStatusCode)
            {
                return null;
            }

            var bytes = await response.Content.ReadAsByteArrayAsync().ConfigureAwait(false);
            if (bytes.Length == 0 || bytes.Length > 20 * 1024 * 1024)
            {
                return null;
            }

            return new Bitmap(new MemoryStream(bytes));
        }
        catch (Exception)
        {
            // 封面只是装饰，网络/解码失败不影响功能。
            return null;
        }
        finally
        {
            _gate.Release();
        }
    }
}
