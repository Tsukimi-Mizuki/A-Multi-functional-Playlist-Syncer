using System.Text.Json;
using LunaticPrincess.Core;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Server.Contracts;
using LunaticPrincess.Server.Endpoints;
using LunaticPrincess.Server.Infrastructure;
using Microsoft.Extensions.Logging;

// 必须在任何 AppPaths 访问之前确定数据目录（AppPaths 只在首次访问时解析一次）。
AppPaths.EnsureDataDirectory();

var builder = WebApplication.CreateBuilder(args);
var urls = ServerUrls.Resolve(args);
builder.WebHost.UseUrls(urls);

var serverLog = new ServerLog();
builder.Logging.ClearProviders();
builder.Logging.AddProvider(new ServerLogProvider(serverLog));
builder.Logging.AddFilter("Microsoft", LogLevel.Warning);
builder.Logging.AddFilter("System", LogLevel.Warning);
// 把框架日志全部导向 stderr，stdout 只保留 LPP_SERVER_READY 一行供 Tauri 解析。
builder.Logging.AddConsole(options => options.LogToStandardErrorThreshold = LogLevel.Trace);

builder.Services.ConfigureHttpJsonOptions(options => JsonDefaults.Configure(options.SerializerOptions));
builder.Services.AddCors(options => options.AddDefaultPolicy(policy => policy.AllowAnyOrigin().AllowAnyHeader().AllowAnyMethod()));
builder.Services.AddSingleton(serverLog);
builder.Services.AddSingleton(provider => ServerServices.Create(provider.GetRequiredService<ServerLog>()));
builder.Services.AddSingleton<JobManager>();

var app = builder.Build();
var services = app.Services.GetRequiredService<ServerServices>();
var jobs = app.Services.GetRequiredService<JobManager>();
var baseUrl = ServerUrls.FirstBaseUrl(urls);

app.UseCors();
// 兜底：请求体解析失败等由框架直接返回的空 4xx/5xx，也统一成 { error: { message, code } }。
app.UseStatusCodePages(async statusContext =>
{
    var http = statusContext.HttpContext;
    var response = http.Response;
    if (response.HasStarted || response.ContentType is not null)
    {
        return;
    }

    var (message, code) = response.StatusCode switch
    {
        StatusCodes.Status400BadRequest => ("请求无效：请求体格式错误或缺少必要参数", "bad_request"),
        StatusCodes.Status404NotFound => ("接口不存在：" + http.Request.Path, "not_found"),
        StatusCodes.Status405MethodNotAllowed => ("不支持的请求方法：" + http.Request.Method, "method_not_allowed"),
        StatusCodes.Status415UnsupportedMediaType => ("不支持的请求内容类型：" + http.Request.ContentType, "unsupported_media_type"),
        _ => ("请求失败（HTTP " + response.StatusCode + "）", "http_error"),
    };

    var payload = JsonSerializer.Serialize(new ErrorResponse(new ErrorBody(message, code)), JsonDefaults.Options);
    response.ContentType = "application/json; charset=utf-8";
    await response.WriteAsync(payload).ConfigureAwait(false);
});
app.UseMiddleware<ApiExceptionMiddleware>();

serverLog.Info("数据目录：" + AppPaths.DataDirectory);
serverLog.Info("配置文件：" + AppPaths.SettingsFilePath);

HealthEndpoints.Map(app);
SessionEndpoints.Map(app, services);
SettingsEndpoints.Map(app, services);
PlaylistEndpoints.Map(app, services);
LibraryEndpoints.Map(app, services, jobs);
TransferEndpoints.Map(app, services, jobs);
NcmEndpoints.Map(app, services, jobs);
TagEndpoints.Map(app, services);
BatchLyricsEndpoints.Map(app, services, jobs);
FileEndpoints.Map(app, services);
JobEndpoints.Map(app, services, jobs);
LogEndpoints.Map(app, services);

app.MapFallback((HttpContext context) => Results.Json(
    new ErrorResponse(new ErrorBody("接口不存在：" + context.Request.Path, "not_found")),
    JsonDefaults.Options,
    statusCode: StatusCodes.Status404NotFound));

// 启动时从 session.dat 恢复登录态，避免首个请求才发现未登录。
try
{
    var session = await services.SessionStore.LoadAsync().ConfigureAwait(false);
    if (session is { IsLoggedIn: true })
    {
        services.Api.SetSession(session);
        serverLog.Info("已恢复网易云登录态：" + session.DisplayName);
    }
    else
    {
        serverLog.Info("尚无网易云登录态，等待扫码登录");
    }
}
catch (Exception ex)
{
    serverLog.Warn("恢复登录态失败：" + ex.Message);
}

app.Lifetime.ApplicationStarted.Register(() =>
{
    Console.Out.WriteLine("LPP_SERVER_READY " + baseUrl);
    Console.Out.Flush();
});

try
{
    await app.RunAsync().ConfigureAwait(false);
}
catch (Exception ex)
{
    serverLog.Error("服务启动失败：" + ex);
    Console.Error.WriteLine(ex);
    Environment.ExitCode = 1;
}
