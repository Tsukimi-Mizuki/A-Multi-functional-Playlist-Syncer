namespace NeteaseMusicManager.Core.Models;

/// <summary>客户端外观样式。</summary>
public enum AppVisualStyle
{
    /// <summary>初始的实色深色界面。</summary>
    Classic = 0,

    /// <summary>磨砂玻璃：Mica/模糊背景 + 冷色雾面玻璃。</summary>
    FrostedGlass = 1,

    /// <summary>亚克力：Windows 亚克力模糊与半透明面板。</summary>
    Acrylic = 2,

    /// <summary>液态玻璃：彩色流动背景 + 高光玻璃面板。</summary>
    LiquidGlass = 3,
}
