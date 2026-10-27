using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Tags;

namespace LunaticPrincess.Core.Tests.Library;

internal sealed class TempWorkspaceForLibrary : IDisposable
{
    public TempWorkspaceForLibrary()
    {
        Root = Path.Combine(Path.GetTempPath(), "nmm-lib-tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Root);
    }

    public string Root { get; }

    public string CreateFile(string relativePath, byte[]? content = null)
    {
        var fullPath = Path.Combine(Root, relativePath);
        var directory = Path.GetDirectoryName(fullPath);
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory);
        }

        File.WriteAllBytes(fullPath, content ?? new byte[] { 0x49, 0x44, 0x33 });
        return fullPath;
    }

    public string CreateDirectory(string relativePath)
    {
        var fullPath = Path.Combine(Root, relativePath);
        Directory.CreateDirectory(fullPath);
        return fullPath;
    }

    public void Dispose()
    {
        try
        {
            if (Directory.Exists(Root))
            {
                Directory.Delete(Root, recursive: true);
            }
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
        }
    }
}

internal sealed class ImmediateProgressForLibrary<T> : IProgress<T>
{
    private readonly object _gate = new();
    private readonly List<T> _reports = new();

    public IReadOnlyList<T> Snapshot()
    {
        lock (_gate)
        {
            return _reports.ToArray();
        }
    }

    public void Report(T value)
    {
        lock (_gate)
        {
            _reports.Add(value);
        }
    }
}

internal sealed class FakeAudioTagServiceForLibrary : IAudioTagService
{
    public Dictionary<string, AudioTagInfo?> Entries { get; } = new(StringComparer.OrdinalIgnoreCase);
    public Dictionary<string, Exception> ReadFailures { get; } = new(StringComparer.OrdinalIgnoreCase);
    public List<string> ReadCalls { get; } = new();

    public Task<AudioTagInfo?> ReadAsync(string audioPath, CancellationToken cancellationToken = default)
    {
        ReadCalls.Add(audioPath);
        cancellationToken.ThrowIfCancellationRequested();
        if (ReadFailures.TryGetValue(audioPath, out var failure))
        {
            throw failure;
        }

        return Task.FromResult(Entries.TryGetValue(audioPath, out var info) ? info : null);
    }

    public Task WriteAsync(string audioPath, AudioTagData tags, CancellationToken cancellationToken = default) =>
        throw new NotSupportedException("扫描测试不需要写标签。");
}

internal sealed class FakeNcmDecoderForLibrary : INcmDecoder
{
    public Dictionary<string, NcmFileInfo> Infos { get; } = new(StringComparer.OrdinalIgnoreCase);
    public Dictionary<string, Exception> ReadFailures { get; } = new(StringComparer.OrdinalIgnoreCase);
    public List<string> ReadInfoCalls { get; } = new();
    public Func<string, bool> IsNcmFileHandler { get; set; } =
        path => string.Equals(Path.GetExtension(path), ".ncm", StringComparison.OrdinalIgnoreCase);

    public bool IsNcmFile(string path) => IsNcmFileHandler(path);

    public Task<NcmFileInfo> ReadInfoAsync(string ncmPath, CancellationToken cancellationToken = default)
    {
        ReadInfoCalls.Add(ncmPath);
        cancellationToken.ThrowIfCancellationRequested();
        if (ReadFailures.TryGetValue(ncmPath, out var failure))
        {
            throw failure;
        }

        return Task.FromResult(Infos.TryGetValue(ncmPath, out var info) ? info : new NcmFileInfo(ncmPath, null));
    }

    public Task<NcmDecryptResult> DecryptAsync(
        string ncmPath,
        string outputDirectory,
        bool overwrite = false,
        CancellationToken cancellationToken = default) =>
        throw new NotSupportedException("扫描测试不需要解密。");
}
