namespace NeteaseMusicManager.Core.Tests.Tags;

/// <summary>生成 TagLibSharp 可解析的最小合法 MP3（MPEG-1 Layer III，128kbps/44.1kHz 静音帧）。</summary>
internal static class Mp3Fixture
{
    private const int Bitrate = 128_000;
    private const int SampleRate = 44_100;

    internal static byte[] BuildMinimalMp3(int frameCount = 24)
    {
        if (frameCount <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(frameCount));
        }

        var frame = BuildFrame();
        var data = new byte[frame.Length * frameCount];
        for (var i = 0; i < frameCount; i++)
        {
            frame.CopyTo(data, i * frame.Length);
        }

        return data;
    }

    /// <summary>1x1 透明 PNG（合法文件头，TagLib 只保存原始字节）。</summary>
    internal static byte[] PngCover() => Convert.FromHexString(
        "89504E470D0A1A0A" +
        "0000000D49484452" +
        "0000000100000001" +
        "08060000001F15C489" +
        "0000000A49444154" +
        "789C6300010000050001" +
        "0D0A2DB4" +
        "0000000049454E44AE426082");

    private static byte[] BuildFrame()
    {
        // MPEG-1 Layer III: 0xFFFB header, bitrate index 9 (128kbps), samplerate index 0 (44.1kHz)。
        var frameLength = 144 * Bitrate / SampleRate; // 417
        var frame = new byte[frameLength];
        frame[0] = 0xFF;
        frame[1] = 0xFB;
        frame[2] = 0x90;
        frame[3] = 0x64;
        return frame;
    }
}
