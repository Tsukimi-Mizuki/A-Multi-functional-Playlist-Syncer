namespace LunaticPrincess.Core.Settings;

/// <summary>界面主题：液态玻璃的深色/浅色两套配色，或跟随系统。</summary>
public enum ThemeMode
{
    /// <summary>深色液态玻璃（默认）。</summary>
    Dark = 0,

    /// <summary>浅色液态玻璃。</summary>
    Light = 1,

    /// <summary>跟随系统当前的应用主题。</summary>
    System = 2,
}
