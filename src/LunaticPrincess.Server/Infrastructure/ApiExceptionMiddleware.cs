using System.Text.Json;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Server.Contracts;

namespace LunaticPrincess.Server.Infrastructure;

/// <summary>统一把异常翻译成 { "error": { "message": ..., "code"?: ... } }。</summary>
public sealed class ApiExceptionMiddleware
{
    private readonly RequestDelegate _next;
    private readonly ServerLog _log;

    public ApiExceptionMiddleware(RequestDelegate next, ServerLog log)
    {
        _next = next;
        _log = log;
    }

    public async Task InvokeAsync(HttpContext context)
    {
        try
        {
            await _next(context).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (context.RequestAborted.IsCancellationRequested)
        {
            // 客户端主动断开（SSE、长轮询等），不再写响应。
        }
        catch (NeteaseApiException ex)
        {
            _log.Error("网易云接口失败：" + ex.Message);
            await WriteErrorAsync(context, StatusCodes.Status502BadGateway, ex.Message, "netease_error").ConfigureAwait(false);
        }
        catch (ApiException ex)
        {
            if (ex.StatusCode >= 500)
            {
                _log.Error(ex.Message);
            }

            await WriteErrorAsync(context, ex.StatusCode, ex.Message, ex.Code).ConfigureAwait(false);
        }
        catch (ArgumentException ex)
        {
            await WriteErrorAsync(context, StatusCodes.Status400BadRequest, ex.Message, "invalid_request").ConfigureAwait(false);
        }
        catch (BadHttpRequestException ex)
        {
            await WriteErrorAsync(context, ex.StatusCode, ex.Message, "bad_request").ConfigureAwait(false);
        }
        catch (Exception ex)
        {
            _log.Error("未处理异常：" + ex);
            await WriteErrorAsync(context, StatusCodes.Status500InternalServerError, ex.Message, "internal_error").ConfigureAwait(false);
        }
    }

    private static async Task WriteErrorAsync(HttpContext context, int statusCode, string message, string? code)
    {
        if (context.Response.HasStarted)
        {
            return;
        }

        context.Response.Clear();
        context.Response.StatusCode = statusCode;
        context.Response.ContentType = "application/json; charset=utf-8";
        var payload = JsonSerializer.Serialize(new ErrorResponse(new ErrorBody(message, code)), JsonDefaults.Options);
        await context.Response.WriteAsync(payload).ConfigureAwait(false);
    }
}
