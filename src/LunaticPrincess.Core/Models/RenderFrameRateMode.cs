namespace LunaticPrincess.Core.Models;

/// <summary>界面渲染帧率上限。</summary>
public enum RenderFrameRateMode
{
    /// <summary>跟随显示器垂直同步刷新率。</summary>
    VSync = 0,
    Fps30 = 30,
    Fps60 = 60,
    Fps90 = 90,
    Fps120 = 120,
    Fps144 = 144,
    Fps165 = 165,
    Fps240 = 240,
}
