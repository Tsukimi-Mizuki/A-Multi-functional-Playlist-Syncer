namespace LunaticPrincess.Core.Models;

/// <summary>歌词写入标签的方式。</summary>
public enum LyricsWriteMode
{
    None = 0,
    Line = 1,
    Word = 2,
}

public enum QrLoginPhase
{
    Idle,
    Creating,
    Waiting,
    Scanned,
    Confirmed,
    Expired,
    Failed,
}
