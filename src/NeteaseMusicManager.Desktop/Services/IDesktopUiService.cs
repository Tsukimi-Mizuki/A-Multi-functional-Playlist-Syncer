using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Settings;
using NeteaseMusicManager.Desktop.ViewModels;

namespace NeteaseMusicManager.Desktop.Services;

/// <summary>View 层能力（弹窗、文件夹选择、打开资源管理器），由主窗口实现，便于 ViewModel 保持可测试。</summary>
public interface IDesktopUiService
{
    Task<NeteaseSession?> ShowQrLoginAsync(QrLoginViewModel viewModel);

    Task<AppSettings?> ShowSettingsAsync(SettingsViewModel viewModel);

    Task ShowNcmImportAsync(NcmImportViewModel viewModel);

    Task<bool> ConfirmAsync(string title, string message, string okText = "确定", string cancelText = "取消");

    Task ShowMessageAsync(string title, string message, string okText = "知道了");

    Task<IReadOnlyList<string>?> PickFoldersAsync(string title, bool allowMultiple);

    Task<string?> PickFolderAsync(string title);

    void OpenFolder(string path);
}
