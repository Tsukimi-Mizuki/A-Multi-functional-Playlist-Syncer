using System.Net.Http;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Core.Utilities;

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
        UnauthorizedAccessException or IOException when ErrorText.IsKnownIoError(exception) => ErrorText.Describe(exception),
        DirectoryNotFoundException => "目录不存在，请先在设置中配置正确的路径",
        FileNotFoundException => "文件不存在或已被移动",
        IOException io => $"文件读写失败：{io.Message}",
        _ => string.IsNullOrWhiteSpace(exception.Message) ? exception.GetType().Name : exception.Message,
    };

    /// <summary>
    /// 接口明确返回「未登录」（业务 code=301）。HTTP 层的 401/302/5xx、限流（406）、风控（-460）、超时与网络错误
    /// 都不算——它们可能是暂时性的，不能据此清除登录态。
    /// </summary>
    public static bool IsAuthenticationError(Exception exception) =>
        exception is NeteaseApiException { IsNotLoggedIn: true };
}
