using Avalonia.Media.Imaging;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Netease;
using NeteaseMusicManager.Core.Storage;
using NeteaseMusicManager.Desktop.Services;
using QRCoder;

namespace NeteaseMusicManager.Desktop.ViewModels;

/// <summary>
/// 扫码登录对话框的 ViewModel：取二维码 → 每 2 秒轮询 → 成功后保存登录态并通知窗口关闭。
/// 所有异常都在对话框内部消化为可重试的错误提示，不会让主程序崩溃。
/// </summary>
public sealed partial class QrLoginViewModel : ObservableObject, IDisposable
{
    private static readonly TimeSpan PollInterval = TimeSpan.FromSeconds(2);

    private readonly INeteaseApiClient _api;
    private readonly ISessionStore _sessionStore;
    private CancellationTokenSource? _cts;
    private int _generation;
    private bool _disposed;

    public QrLoginViewModel(INeteaseApiClient api, ISessionStore sessionStore)
    {
        _api = api;
        _sessionStore = sessionStore;
    }

    /// <summary>扫码成功（携带已保存的登录态）或窗口被取消（null）时触发。</summary>
    public event Action<NeteaseSession?>? Completed;

    [ObservableProperty]
    private Bitmap? _qrImage;

    [ObservableProperty]
    private string _statusText = "正在获取二维码…";

    [ObservableProperty]
    private string _statusKind = "waiting";

    [ObservableProperty]
    private string _hintText = "使用网易云音乐 App 扫码登录";

    [ObservableProperty]
    private bool _canRefresh;

    [ObservableProperty]
    private bool _isCreating;

    public bool IsQrReady => QrImage is not null;

    partial void OnQrImageChanged(Bitmap? value) => OnPropertyChanged(nameof(IsQrReady));

    /// <summary>窗口打开后调用一次。</summary>
    public Task StartAsync() => CreateQrAsync();

    [RelayCommand]
    private Task RefreshAsync() => CreateQrAsync();

    private async Task CreateQrAsync()
    {
        if (_disposed)
        {
            return;
        }

        var generation = ++_generation;
        CancelCurrent();
        var cts = new CancellationTokenSource();
        _cts = cts;

        IsCreating = true;
        CanRefresh = false;
        StatusKind = "waiting";
        StatusText = "正在获取二维码…";
        HintText = "正在向网易云请求登录二维码";
        QrImage = null;

        try
        {
            var start = await _api.CreateQrLoginAsync(cts.Token).ConfigureAwait(true);
            if (generation != _generation || cts.IsCancellationRequested)
            {
                return;
            }

            QrImage = RenderQrCode(start.QrContent);
            IsCreating = false;
            StatusKind = "waiting";
            StatusText = "等待扫码";
            HintText = "打开网易云音乐 App，扫描上方二维码";
            _ = PollLoopAsync(start.Key, generation, cts.Token);
        }
        catch (OperationCanceledException)
        {
            // 刷新/关闭窗口造成的取消，忽略。
        }
        catch (Exception ex)
        {
            if (generation != _generation)
            {
                return;
            }

            IsCreating = false;
            CanRefresh = true;
            StatusKind = "error";
            StatusText = "获取二维码失败";
            HintText = ErrorFormatter.ToFriendlyMessage(ex);
        }
    }

    private async Task PollLoopAsync(string key, int generation, CancellationToken cancellationToken)
    {
        var transientFailures = 0;

        try
        {
            while (!cancellationToken.IsCancellationRequested && generation == _generation)
            {
                await Task.Delay(PollInterval, cancellationToken).ConfigureAwait(true);
                if (generation != _generation)
                {
                    return;
                }

                QrLoginStatus status;
                try
                {
                    status = await _api.PollQrLoginAsync(key, cancellationToken).ConfigureAwait(true);
                    transientFailures = 0;
                }
                catch (OperationCanceledException)
                {
                    return;
                }
                catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException)
                {
                    if (++transientFailures >= 3)
                    {
                        ReportError(generation, "查询扫码状态失败，请点击刷新重试", ex);
                        return;
                    }

                    continue;
                }
                catch (Exception ex)
                {
                    ReportError(generation, "扫码登录失败", ex);
                    return;
                }

                switch (status.State)
                {
                    case QrLoginState.Waiting:
                        if (StatusKind != "waiting")
                        {
                            StatusKind = "waiting";
                            StatusText = "等待扫码";
                        }

                        break;

                    case QrLoginState.Scanned:
                        StatusKind = "scanned";
                        StatusText = "已扫码，请在手机端确认";
                        HintText = "请在手机上点击「确认登录」，完成后会自动进入主界面";
                        break;

                    case QrLoginState.Confirmed:
                    {
                        var session = status.Session;
                        if (session is not { IsLoggedIn: true })
                        {
                            ReportError(generation, "登录会话无效，请刷新二维码重试", null);
                            return;
                        }

                        StatusKind = "success";
                        StatusText = "登录成功";
                        HintText = "正在同步账号信息…";

                        try
                        {
                            await _sessionStore.SaveAsync(session, CancellationToken.None).ConfigureAwait(true);
                            _api.SetSession(session);
                        }
                        catch (Exception ex)
                        {
                            ReportError(generation, "保存登录状态失败", ex);
                            return;
                        }

                        IsCreating = false;
                        await Task.Delay(450, CancellationToken.None).ConfigureAwait(true);
                        Complete(session);
                        return;
                    }

                    case QrLoginState.Expired:
                        StatusKind = "error";
                        StatusText = "二维码已过期";
                        HintText = "请点击「刷新二维码」重新获取";
                        CanRefresh = true;
                        return;

                    case QrLoginState.Failed:
                    default:
                        ReportError(generation, string.IsNullOrWhiteSpace(status.Message) ? "登录失败，请重试" : status.Message, null);
                        return;
                }
            }
        }
        catch (OperationCanceledException)
        {
            // 正常取消。
        }
        catch (Exception ex)
        {
            ReportError(generation, "扫码登录失败", ex);
        }
    }

    private void ReportError(int generation, string message, Exception? exception)
    {
        if (generation != _generation || _disposed)
        {
            return;
        }

        IsCreating = false;
        CanRefresh = true;
        StatusKind = "error";
        StatusText = message;
        HintText = exception is null ? "请点击「刷新二维码」重试" : ErrorFormatter.ToFriendlyMessage(exception);
    }

    private void Complete(NeteaseSession session)
    {
        if (_disposed)
        {
            return;
        }

        Completed?.Invoke(session);
    }

    public void Cancel()
    {
        _generation++;
        CancelCurrent();
    }

    private void CancelCurrent()
    {
        var cts = _cts;
        _cts = null;
        if (cts is null)
        {
            return;
        }

        try
        {
            cts.Cancel();
        }
        catch (ObjectDisposedException)
        {
            // 已取消。
        }

        cts.Dispose();
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        CancelCurrent();
        Completed = null;
    }

    private static Bitmap? RenderQrCode(string content)
    {
        if (string.IsNullOrWhiteSpace(content))
        {
            return null;
        }

        using var generator = new QRCodeGenerator();
        using var data = generator.CreateQrCode(content, QRCodeGenerator.ECCLevel.Q);
        var png = new PngByteQRCode(data).GetGraphic(20);
        return new Bitmap(new MemoryStream(png));
    }
}
