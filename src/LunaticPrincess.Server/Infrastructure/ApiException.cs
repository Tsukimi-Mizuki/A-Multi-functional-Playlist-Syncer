namespace LunaticPrincess.Server.Infrastructure;

/// <summary>带 HTTP 状态码的业务异常，由 ApiExceptionMiddleware 统一转成 { error: { message, code } }。</summary>
public sealed class ApiException : Exception
{
    public ApiException(int statusCode, string message, string? code = null) : base(message)
    {
        StatusCode = statusCode;
        Code = code;
    }

    public int StatusCode { get; }

    public string? Code { get; }
}
