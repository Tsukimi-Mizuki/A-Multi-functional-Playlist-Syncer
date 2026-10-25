using NeteaseMusicManager.Core.Download;
using NeteaseMusicManager.Core.Ncm;
using Xunit;

namespace NeteaseMusicManager.Core.Tests.Download;

public sealed class NcmImportServiceTests
{
    private static byte[] PngBytes(byte marker) =>
        new byte[] { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, marker };

    private static byte[] NcmContent() => new byte[] { 0x43, 0x54, 0x45, 0x4E, 0x46, 0x44, 0x41, 0x4D, 1, 2, 3 };

    private static void AssertNoTempFiles(string directory)
    {
        foreach (var file in Directory.EnumerateFiles(directory, "*", SearchOption.AllDirectories))
        {
            var name = Path.GetFileName(file);
            Assert.DoesNotContain(".tmp", name, StringComparison.OrdinalIgnoreCase);
            Assert.DoesNotContain(".tagtmp", name, StringComparison.OrdinalIgnoreCase);
            Assert.False(name.StartsWith(".", StringComparison.Ordinal), $"遗留临时文件: {file}");
        }
    }

    [Fact]
    public async Task ImportAsync_DecryptsAndWritesNcmMetadataAndCover()
    {
        using var workspace = new TempWorkspaceForDownload();
        var ncmPath = workspace.WriteFile("Lucky.ncm", NcmContent());
        var cover = PngBytes(5);
        var decoder = new FakeNcmDecoderForDownload
        {
            OutputFormat = "flac",
            CoverData = cover,
            Metadata = new NcmMetadata("Lucky", "Jason Mraz/Colbie Caillat", "We Sing", "flac", 999_000, 245_000, 777),
        };
        var tags = new RecordingTagServiceForDownload();
        var progress = new ImmediateProgressForDownload<NcmImportProgress>();
        var service = new NcmImportService(decoder, tags);

        var results = await service.ImportAsync(new[] { ncmPath }, workspace.Root, overwrite: false, progress);

        var result = Assert.Single(results);
        Assert.True(result.Success);
        Assert.NotNull(result.OutputPath);
        Assert.True(File.Exists(result.OutputPath));
        Assert.EndsWith(".flac", result.OutputPath, StringComparison.OrdinalIgnoreCase);
        Assert.Equal(777, result.Metadata?.MusicId);

        var write = Assert.Single(tags.Writes);
        Assert.Equal("Lucky", write.Tags.Title);
        Assert.Equal(new[] { "Jason Mraz", "Colbie Caillat" }, write.Tags.Artists);
        Assert.Equal("We Sing", write.Tags.Album);
        Assert.Equal(777L, write.Tags.NeteaseId);
        Assert.Equal(cover, write.Tags.CoverData);
        Assert.Equal("image/png", write.Tags.CoverMimeType);
        Assert.Equal(1, progress.Snapshot()[^1].Completed);
        Assert.Equal(1, progress.Snapshot()[^1].Total);
        AssertNoTempFiles(workspace.Root);
    }

    [Fact]
    public async Task ImportAsync_OverwriteFlag_IsForwardedToDecoder()
    {
        using var workspace = new TempWorkspaceForDownload();
        var ncmPath = workspace.WriteFile("song.ncm", NcmContent());
        var decoder = new FakeNcmDecoderForDownload();
        var service = new NcmImportService(decoder, new RecordingTagServiceForDownload());

        var first = await service.ImportAsync(new[] { ncmPath }, workspace.Root, overwrite: false);
        var second = await service.ImportAsync(new[] { ncmPath }, workspace.Root, overwrite: true);

        Assert.True(first[0].Success);
        Assert.True(second[0].Success);
        Assert.Equal(new[] { false, true }, decoder.DecryptOverwriteFlags);
    }

    [Fact]
    public async Task ImportAsync_PropagatesDecoderFailure()
    {
        using var workspace = new TempWorkspaceForDownload();
        var ncmPath = workspace.WriteFile("song.ncm", NcmContent());
        var decoder = new FakeNcmDecoderForDownload
        {
            DecryptHandler = (source, _, _, _) => Task.FromResult(
                new NcmDecryptResult(source, null, false, ErrorMessage: "目标文件已存在")),
        };
        var tags = new RecordingTagServiceForDownload();
        var service = new NcmImportService(decoder, tags);

        var results = await service.ImportAsync(new[] { ncmPath }, workspace.Root);

        var result = Assert.Single(results);
        Assert.False(result.Success);
        Assert.Equal("目标文件已存在", result.ErrorMessage);
        Assert.Empty(tags.Writes);
    }

    [Fact]
    public async Task ImportAsync_RejectsNonNcmAndMissingFiles()
    {
        using var workspace = new TempWorkspaceForDownload();
        var notNcm = workspace.WriteFile("not.ncm", new byte[] { 1, 2, 3 });
        var missing = Path.Combine(workspace.Root, "missing.ncm");
        var decoder = new FakeNcmDecoderForDownload { IsNcmFileResult = false };
        var service = new NcmImportService(decoder, new RecordingTagServiceForDownload());

        var results = await service.ImportAsync(new[] { notNcm, missing }, workspace.Root);

        Assert.Equal(2, results.Count);
        Assert.False(results[0].Success);
        Assert.Contains("NCM", results[0].ErrorMessage);
        Assert.False(results[1].Success);
        Assert.Contains("不存在", results[1].ErrorMessage);
        Assert.Empty(decoder.DecryptCalls);
    }

    [Fact]
    public async Task ImportAsync_TagWriteFailure_ReportsFailureButKeepsDecryptedAudio()
    {
        using var workspace = new TempWorkspaceForDownload();
        var ncmPath = workspace.WriteFile("song.ncm", NcmContent());
        var decoder = new FakeNcmDecoderForDownload();
        var tags = new RecordingTagServiceForDownload { ThrowOnWrite = true };
        var service = new NcmImportService(decoder, tags);

        var results = await service.ImportAsync(new[] { ncmPath }, workspace.Root);

        var result = Assert.Single(results);
        Assert.False(result.Success);
        Assert.Contains("标签写入失败", result.ErrorMessage);
        Assert.NotNull(result.OutputPath);
        Assert.True(File.Exists(result.OutputPath));
        AssertNoTempFiles(workspace.Root);
    }

    [Fact]
    public async Task ImportAsync_DeduplicatesPaths_AndReportsProgress()
    {
        using var workspace = new TempWorkspaceForDownload();
        var ncmPath = workspace.WriteFile("song.ncm", NcmContent());
        var decoder = new FakeNcmDecoderForDownload();
        var tags = new RecordingTagServiceForDownload();
        var progress = new ImmediateProgressForDownload<NcmImportProgress>();
        var service = new NcmImportService(decoder, tags);

        var results = await service.ImportAsync(new[] { ncmPath, ncmPath }, workspace.Root, progress: progress);

        Assert.Single(results);
        Assert.Single(decoder.DecryptCalls);
        Assert.Single(tags.Writes);
        var reports = progress.Snapshot();
        Assert.Equal(1, reports[^1].Completed);
        Assert.Equal(1, reports[^1].Total);
    }

    [Fact]
    public async Task ImportAsync_ContinuesAfterOneFailure()
    {
        using var workspace = new TempWorkspaceForDownload();
        var good = workspace.WriteFile("good.ncm", NcmContent());
        var bad = workspace.WriteFile("bad.ncm", NcmContent());
        var decoder = new FakeNcmDecoderForDownload
        {
            DecryptHandler = (source, outputDirectory, _, cancellationToken) =>
            {
                if (source.EndsWith("bad.ncm", StringComparison.OrdinalIgnoreCase))
                {
                    return Task.FromResult(new NcmDecryptResult(source, null, false, ErrorMessage: "解密失败"));
                }

                return new FakeNcmDecoderForDownload().DecryptAsync(source, outputDirectory, true, cancellationToken);
            },
        };
        var service = new NcmImportService(decoder, new RecordingTagServiceForDownload());

        var results = await service.ImportAsync(new[] { bad, good }, workspace.Root);

        Assert.Equal(2, results.Count);
        Assert.False(results[0].Success);
        Assert.True(results[1].Success);
    }

    [Fact]
    public async Task ImportAsync_HonoursCancellation()
    {
        using var workspace = new TempWorkspaceForDownload();
        var ncmPath = workspace.WriteFile("song.ncm", NcmContent());
        using var cts = new CancellationTokenSource();
        cts.Cancel();
        var service = new NcmImportService(new FakeNcmDecoderForDownload(), new RecordingTagServiceForDownload());

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => service.ImportAsync(new[] { ncmPath }, workspace.Root, cancellationToken: cts.Token));
    }

    [Fact]
    public async Task ImportAsync_EmptyInput_ReturnsEmptyList()
    {
        using var workspace = new TempWorkspaceForDownload();
        var service = new NcmImportService(new FakeNcmDecoderForDownload(), new RecordingTagServiceForDownload());

        var results = await service.ImportAsync(Array.Empty<string>(), workspace.Root);

        Assert.Empty(results);
    }
}
