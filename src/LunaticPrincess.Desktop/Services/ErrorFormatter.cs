using System.Net.Http;
using LunaticPrincess.Core.Netease;

namespace LunaticPrincess.Desktop.Services;

/// <summary>把底层异常转换成适合直接展示给用户的中文提示（不丢失原始异常，调用方仍可记录日志）。</summary>
public static class ErrorFormatter
{
    public static string ToFriendlyMessage(Exception exception) => exception switch
    {
        TaskCanceledException => "请求超时，请稍后重试",
        OperationCanceledException => "操作已取消",
        NeteaseApiException api => api.Code is { } code
            ? $"网易云接口错误（{code}）：{api.Message}"
            : $"网易云接口错误：{api.Message}",
        HttpRequestException => "网络连接失败，请检查网络后重试",
        UnauthorizedAccessException => "没有访问权限，请检查目录权限或以管理员身份重试",
        DirectoryNotFoundException => "目录不存在，请先在设置中配置正确的路径",
        FileNotFoundException => "文件不存在或已被移动",
        IOException io => $"文件读写失败：{io.Message}",
        _ => string.IsNullOrWhiteSpace(exception.Message) ? exception.GetType().Name : exception.Message,
    };

    /// <summary>登录态失效（网易云返回需要重新登录的错误码）。</summary>
    public static bool IsAuthenticationError(Exception exception) =>
        exception is NeteaseApiException { Code: 301 or -2 or 401 or 302 };
}
