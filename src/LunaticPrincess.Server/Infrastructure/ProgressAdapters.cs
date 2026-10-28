using System.Globalization;
using LunaticPrincess.Core.Download;
using LunaticPrincess.Core.Library;
using LunaticPrincess.Core.Ncm;
using LunaticPrincess.Core.Sync;

namespace LunaticPrincess.Server.Infrastructure;

/// <summary>把 Core 各服务的进度类型映射成统一的 JobProgress。</summary>
public static class ProgressAdapters
{
    public static IProgress<LibraryScanProgress> Library(IProgress<JobProgress> target)
        => new DelegateProgress<LibraryScanProgress>(p => target.Report(new JobProgress
        {
            Completed = p.ScannedFiles,
            Total = p.TotalFiles,
            Stage = "正在扫描音乐库",
            Item = FileName(p.CurrentFile),
            Percent = Percent(p.ScannedFiles, p.TotalFiles),
        }));

    public static IProgress<DownloadProgress> Download(IProgress<JobProgress> target)
        => new DelegateProgress<DownloadProgress>(p => target.Report(new JobProgress
        {
            Completed = p.Completed,
            Total = p.Total,
            Stage = string.IsNullOrWhiteSpace(p.Stage) ? "下载中" : p.Stage,
            Item = p.SongName ?? "",
            Message = p.Message,
            Percent = p.Percent ?? Percent(p.Completed, p.Total),
        }));

    public static IProgress<SyncProgress> Sync(IProgress<JobProgress> target)
        => new DelegateProgress<SyncProgress>(p => target.Report(new JobProgress
        {
            Completed = p.Completed,
            Total = p.Total,
            Stage = string.IsNullOrWhiteSpace(p.Stage) ? "正在同步" : p.Stage,
            Item = p.SongName ?? "",
            Message = p.Message,
            Percent = p.Percent ?? Percent(p.Completed, p.Total),
        }));

    public static IProgress<NcmImportProgress> NcmImport(IProgress<JobProgress> target)
        => new DelegateProgress<NcmImportProgress>(p =>
        {
            var stage = string.IsNullOrWhiteSpace(p.Message) ? "正在解密 NCM…" : p.Message!;
            var name = FileName(p.CurrentFile);
            var total = Math.Max(p.Total, 1);
            var position = Math.Min(p.Completed + 1, total);
            var message = "正在处理 " + position.ToString(CultureInfo.InvariantCulture) + "/"
                + total.ToString(CultureInfo.InvariantCulture) + "：" + name + " · " + stage;
            target.Report(new JobProgress
            {
                Completed = p.Completed,
                Total = p.Total,
                Stage = stage,
                Item = name,
                Message = message,
                Percent = Percent(p.Completed, p.Total),
            });
        });

    private static double? Percent(int completed, int total)
        => total > 0 ? Math.Clamp(completed * 100.0 / total, 0, 100) : null;

    private static string FileName(string? path)
        => string.IsNullOrWhiteSpace(path) ? "" : Path.GetFileName(path);
}

/// <summary>同步转发的 IProgress 实现，避免 Progress&lt;T&gt; 在无同步上下文时乱序。</summary>
public sealed class DelegateProgress<T> : IProgress<T>
{
    private readonly Action<T> _report;

    public DelegateProgress(Action<T> report) => _report = report;

    public void Report(T value) => _report(value);
}
