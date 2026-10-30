using System.Security.Principal;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// 提权状态检测。
/// Windows 的 UIPI 会阻止从资源管理器（普通权限）向管理员权限进程拖放文件，
/// 且没有任何客户端开关可以放行，因此在界面上提前说明原因并给出替代操作。
/// </summary>
public static class ElevationHelper
{
    /// <summary>拖放被系统阻断时给用户看的说明。</summary>
    public const string DragDropBlockedHint =
        "当前以管理员身份运行，Windows 会阻止从资源管理器拖入文件；请改用界面上的按钮选择文件，或以普通权限重新启动。";

    private static readonly Lazy<bool> Elevated = new(Detect);

    /// <summary>当前进程是否以管理员身份运行（非 Windows 或检测失败时返回 false）。</summary>
    public static bool IsElevated => Elevated.Value;

    private static bool Detect()
    {
        if (!OperatingSystem.IsWindows())
        {
            return false;
        }

        try
        {
            using var identity = WindowsIdentity.GetCurrent();
            return new WindowsPrincipal(identity).IsInRole(WindowsBuiltInRole.Administrator);
        }
        catch (Exception)
        {
            // 拿不到令牌信息时按非提权处理，避免影响正常启动。
            return false;
        }
    }
}
