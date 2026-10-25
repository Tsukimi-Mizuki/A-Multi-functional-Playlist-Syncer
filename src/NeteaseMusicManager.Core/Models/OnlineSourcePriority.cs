namespace NeteaseMusicManager.Core.Models;

/// <summary>在线标签/歌词来源优先级。</summary>
public enum OnlineSourcePriority
{
    NeteaseFirst = 0,
    QQMusicFirst = 1,
    NeteaseOnly = 2,
    QQMusicOnly = 3,
}
