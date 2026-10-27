using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using LunaticPrincess.Desktop.Services;
using LunaticPrincess.Desktop.ViewModels;
using LunaticPrincess.Desktop.Views;

namespace LunaticPrincess.Desktop;

public partial class App : Application
{
    private AppServices? _services;
    private MainWindowViewModel? _mainViewModel;

    public override void Initialize() => AvaloniaXamlLoader.Load(this);

    public override void OnFrameworkInitializationCompleted()
    {
        if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
        {
            desktop.ShutdownMode = ShutdownMode.OnMainWindowClose;

            try
            {
                _services = AppServices.Create();
                var uiService = new DesktopUiService();
                _mainViewModel = new MainWindowViewModel(_services, uiService);
                var mainWindow = new MainWindow(_mainViewModel);
                uiService.Owner = mainWindow;
                desktop.MainWindow = mainWindow;
                mainWindow.Opened += async (_, _) =>
                {
                    try
                    {
                        await _mainViewModel.InitializeAsync();
                        var startupLyricsFile = Program.GetStartupLyricsFile();
                        if (!string.IsNullOrWhiteSpace(startupLyricsFile))
                        {
                            await _mainViewModel.OpenLyricsEditorForFileAsync(startupLyricsFile);
                        }
                    }
                    catch (Exception ex)
                    {
                        await new MessageDialog(
                                "初始化失败",
                                ErrorFormatter.ToFriendlyMessage(ex),
                                "知道了")
                            .ShowDialog(mainWindow);
                    }
                };
                desktop.Exit += (_, _) =>
                {
                    _mainViewModel?.Dispose();
                    _services?.Dispose();
                };
            }
            catch (Exception ex)
            {
                var errorDialog = new MessageDialog(
                    "启动失败",
                    "应用初始化失败：" + ErrorFormatter.ToFriendlyMessage(ex),
                    "退出");
                errorDialog.WindowStartupLocation = WindowStartupLocation.CenterScreen;
                desktop.MainWindow = errorDialog;
            }
        }

        base.OnFrameworkInitializationCompleted();
    }
}
