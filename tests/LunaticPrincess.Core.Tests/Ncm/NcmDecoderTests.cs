using LunaticPrincess.Core.Ncm;
using Xunit;

namespace LunaticPrincess.Core.Tests.Ncm;

public sealed class NcmDecoderTests : IDisposable
{
    private readonly TempDirectory _temp = new("ncm-decoder");
    private readonly NcmDecoder _decoder = new();

    public void Dispose() => _temp.Dispose();

    [Fact]
    public void IsNcmFile_DetectsMagic_AndReturnsFalseForOtherFiles()
    {
        var ncmPath = _temp.File("track.ncm");
        File.WriteAllBytes(ncmPath, SyntheticNcmFile.Build(CreateMp3Audio()));

        var plainPath = _temp.File("plain.mp3");
        File.WriteAllBytes(plainPath, CreateMp3Audio(1024));

        Assert.True(_decoder.IsNcmFile(ncmPath));
        Assert.False(_decoder.IsNcmFile(plainPath));
        Assert.False(_decoder.IsNcmFile(_temp.File("missing.ncm")));
        Assert.False(_decoder.IsNcmFile(string.Empty));
    }

    [Fact]
    public async Task ReadInfoAsync_ParsesMetadataWithoutDecryptingAudio()
    {
        var path = _temp.File("metadata.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(CreateMp3Audio(1000)));

        var info = await _decoder.ReadInfoAsync(path);

        Assert.Equal(path, info.SourcePath);
        Assert.True(info.HasMetadata);
        var metadata = Assert.IsType<NcmMetadata>(info.Metadata);
        Assert.Equal("测试歌曲", metadata.Title);
        Assert.Equal("测试专辑", metadata.Album);
        Assert.Equal("测试歌手/第二歌手", metadata.Artists);
        Assert.Equal("mp3", metadata.Format);
        Assert.Equal(320000, metadata.Bitrate);
        Assert.Equal(215000, metadata.DurationMs);
        Assert.Equal(1234567890L, metadata.MusicId);
        Assert.Equal("别名", metadata.Alias);
    }

    [Fact]
    public async Task ReadInfoAsync_ThrowsForNonNcmAndMissingFiles()
    {
        var plainPath = _temp.File("not-ncm.bin");
        File.WriteAllBytes(plainPath, CreateMp3Audio(512));

        await Assert.ThrowsAsync<InvalidDataException>(() => _decoder.ReadInfoAsync(plainPath));
        await Assert.ThrowsAsync<FileNotFoundException>(() => _decoder.ReadInfoAsync(_temp.File("missing.ncm")));
        await Assert.ThrowsAsync<ArgumentException>(() => _decoder.ReadInfoAsync(" "));
    }

    [Fact]
    public async Task ReadInfoAsync_ThrowsInvalidDataForTruncatedFile()
    {
        var valid = SyntheticNcmFile.Build(CreateMp3Audio(2048));
        var path = _temp.File("truncated.ncm");
        File.WriteAllBytes(path, valid.AsSpan(0, 20).ToArray());

        await Assert.ThrowsAsync<InvalidDataException>(() => _decoder.ReadInfoAsync(path));
    }

    [Fact]
    public async Task DecryptAsync_Mp3_RoundTripsAudioMetadataAndCover()
    {
        var audio = CreateMp3Audio(5000);
        var cover = CreatePngBytes();
        var path = _temp.File("song.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(audio, cover, coverPadding: 8));

        var result = await _decoder.DecryptAsync(path, _temp.Path);

        Assert.True(result.Success, result.ErrorMessage);
        Assert.Equal(path, result.SourcePath);
        Assert.NotNull(result.OutputPath);
        Assert.EndsWith(".mp3", result.OutputPath, StringComparison.OrdinalIgnoreCase);
        Assert.Equal("mp3", result.Format);
        Assert.Equal(File.ReadAllBytes(result.OutputPath!), audio);
        Assert.Equal(cover, result.CoverData);
        Assert.NotNull(result.Metadata);
        Assert.Equal("测试歌曲", result.Metadata!.Title);
        Assert.True(File.Exists(result.OutputPath));
    }

    [Fact]
    public async Task DecryptAsync_Flac_DetectsFlacFormat()
    {
        var audio = CreateFlacAudio(9000);
        var path = _temp.File("lossless.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(audio));

        var result = await _decoder.DecryptAsync(path, _temp.Path);

        Assert.True(result.Success, result.ErrorMessage);
        Assert.Equal("flac", result.Format);
        Assert.EndsWith(".flac", result.OutputPath, StringComparison.OrdinalIgnoreCase);
        Assert.Equal(audio, File.ReadAllBytes(result.OutputPath!));
    }

    [Fact]
    public async Task DecryptAsync_LargeAudio_SurvivesChunkBoundaries()
    {
        var audio = CreateMp3Audio(32_768 * 2 + 517);
        var path = _temp.File("large.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(audio, cover: null));

        var result = await _decoder.DecryptAsync(path, _temp.Path);

        Assert.True(result.Success, result.ErrorMessage);
        Assert.Equal(audio, File.ReadAllBytes(result.OutputPath!));
    }

    [Fact]
    public async Task DecryptAsync_WithoutMetadata_StillProducesAudio()
    {
        var audio = CreateMp3Audio(3000);
        var path = _temp.File("no-meta.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(audio, includeMetadata: false));

        var result = await _decoder.DecryptAsync(path, _temp.Path);

        Assert.True(result.Success, result.ErrorMessage);
        Assert.Null(result.Metadata);
        Assert.Equal(audio, File.ReadAllBytes(result.OutputPath!));
    }

    [Fact]
    public async Task DecryptAsync_InvalidMetadata_DoesNotAbortAudio()
    {
        var audio = CreateMp3Audio(3000);
        var path = _temp.File("bad-meta.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(audio, metadataJson: "{not-valid-json"));

        var result = await _decoder.DecryptAsync(path, _temp.Path);

        Assert.True(result.Success, result.ErrorMessage);
        Assert.Null(result.Metadata);
        Assert.Equal(audio, File.ReadAllBytes(result.OutputPath!));
    }

    [Fact]
    public async Task DecryptAsync_ExistingTarget_RespectsOverwriteFlag()
    {
        var audio = CreateMp3Audio(4000);
        var path = _temp.File("existing.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(audio));

        var first = await _decoder.DecryptAsync(path, _temp.Path);
        Assert.True(first.Success);

        var sentinel = new byte[] { 1, 2, 3, 4 };
        File.WriteAllBytes(first.OutputPath!, sentinel);

        var skipped = await _decoder.DecryptAsync(path, _temp.Path, overwrite: false);
        Assert.False(skipped.Success);
        Assert.False(string.IsNullOrWhiteSpace(skipped.ErrorMessage));
        Assert.Equal(sentinel, File.ReadAllBytes(first.OutputPath!));

        var overwritten = await _decoder.DecryptAsync(path, _temp.Path, overwrite: true);
        Assert.True(overwritten.Success, overwritten.ErrorMessage);
        Assert.Equal(first.OutputPath, overwritten.OutputPath);
        Assert.Equal(audio, File.ReadAllBytes(overwritten.OutputPath!));
    }

    [Fact]
    public async Task DecryptAsync_HonorsCancellation_AndLeavesNoPartialFiles()
    {
        var path = _temp.File("cancel.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(CreateMp3Audio(100_000)));

        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => _decoder.DecryptAsync(path, _temp.Path, overwrite: true, cancellation.Token));

        Assert.Empty(Directory.GetFiles(_temp.Path, "*.mp3"));
        Assert.Empty(Directory.GetFiles(_temp.Path, "*.ncmtmp"));
    }

    [Fact]
    public async Task DecryptAsync_LeavesNoTemporaryFilesOnSuccessOrFailure()
    {
        var audio = CreateMp3Audio(6000);
        var path = _temp.File("clean.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(audio));

        var result = await _decoder.DecryptAsync(path, _temp.Path);
        Assert.True(result.Success);

        Assert.Empty(Directory.GetFiles(_temp.Path, "*.ncmtmp"));
        Assert.Single(Directory.GetFiles(_temp.Path, "*.mp3"));

        var badPath = _temp.File("broken.ncm");
        File.WriteAllBytes(badPath, new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D, 0x00 });
        var failure = await _decoder.DecryptAsync(badPath, _temp.Path);
        Assert.False(failure.Success);
        Assert.Empty(Directory.GetFiles(_temp.Path, "*.ncmtmp"));
    }

    [Fact]
    public async Task DecryptAsync_CreatesMissingOutputDirectory()
    {
        var path = _temp.File("nested.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(CreateMp3Audio(2048)));
        var outputDirectory = Path.Combine(_temp.Path, "a", "b", "c");

        var result = await _decoder.DecryptAsync(path, outputDirectory);

        Assert.True(result.Success, result.ErrorMessage);
        Assert.True(File.Exists(result.OutputPath));
        Assert.StartsWith(outputDirectory, result.OutputPath, StringComparison.OrdinalIgnoreCase);
    }

    [Fact]
    public async Task DecryptAsync_NonNcmFile_ReturnsFailureResult()
    {
        var path = _temp.File("fake.ncm");
        File.WriteAllBytes(path, CreateMp3Audio(2048));

        var result = await _decoder.DecryptAsync(path, _temp.Path);

        Assert.False(result.Success);
        Assert.Null(result.OutputPath);
        Assert.False(string.IsNullOrWhiteSpace(result.ErrorMessage));
        Assert.Empty(Directory.GetFiles(_temp.Path, "*.mp3"));
    }

    [Fact]
    public async Task DecryptAsync_TruncatedFile_ReturnsFailureResult()
    {
        var valid = SyntheticNcmFile.Build(CreateMp3Audio(2048));
        var path = _temp.File("short.ncm");
        File.WriteAllBytes(path, valid.AsSpan(0, 40).ToArray());

        var result = await _decoder.DecryptAsync(path, _temp.Path);

        Assert.False(result.Success);
        Assert.False(string.IsNullOrWhiteSpace(result.ErrorMessage));
    }

    [Fact]
    public async Task ReadInfoAsync_DoesNotDecryptAudio()
    {
        // 音频区完全无效也不影响只读文件头/元数据。
        var path = _temp.File("header-only.ncm");
        File.WriteAllBytes(path, SyntheticNcmFile.Build(new byte[4096], cover: null));

        var info = await _decoder.ReadInfoAsync(path);

        Assert.NotNull(info.Metadata);
        Assert.Equal("测试歌曲", info.Metadata!.Title);
    }

    private static byte[] CreateMp3Audio(int length = 40_000)
    {
        var audio = new byte[Math.Max(3, length)];
        "ID3"u8.CopyTo(audio);
        for (var i = 3; i < audio.Length; i++)
        {
            audio[i] = (byte)(i * 31 + 7);
        }

        return audio;
    }

    private static byte[] CreateFlacAudio(int length = 8192)
    {
        var audio = new byte[Math.Max(4, length)];
        "fLaC"u8.CopyTo(audio);
        for (var i = 4; i < audio.Length; i++)
        {
            audio[i] = (byte)(i * 17 + 11);
        }

        return audio;
    }

    private static byte[] CreatePngBytes()
    {
        var bytes = new byte[128];
        ReadOnlySpan<byte> pngMagic = stackalloc byte[] { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A };
        pngMagic.CopyTo(bytes);
        for (var i = 8; i < bytes.Length; i++)
        {
            bytes[i] = (byte)(i * 13 + 5);
        }

        return bytes;
    }
}
