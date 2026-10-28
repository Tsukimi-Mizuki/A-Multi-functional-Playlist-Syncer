using System.Collections.Concurrent;
using System.Diagnostics.CodeAnalysis;
using System.Text.Json;
using System.Threading.Channels;
using LunaticPrincess.Core.Netease;
using LunaticPrincess.Server.Contracts;

namespace LunaticPrincess.Server.Infrastructure;

public enum JobState
{
    Running,
    Done,
    Failed,
    Canceled,
}

/// <summary>任务进度快照，对应契约里的 ProgressDto。</summary>
public sealed class JobProgress
{
    public int Completed { get; init; }

    public int Total { get; init; }

    public string Stage { get; init; } = "";

    public string Item { get; init; } = "";

    public string? Message { get; init; }

    public double? Percent { get; init; }

    public ProgressDto ToDto() => new(Completed, Total, Stage, Item, Message, Percent);
}

public sealed record JobEventFrame(string Payload, bool Terminal);

public sealed record JobDonePayload(object? Result);

public sealed class JobSubscription : IDisposable
{
    private readonly JobRecord _owner;
    private readonly Channel<JobEventFrame>? _channel;

    internal JobSubscription(JobRecord owner, Channel<JobEventFrame>? channel, IReadOnlyList<JobEventFrame> history, bool terminal)
    {
        _owner = owner;
        _channel = channel;
        History = history;
        Terminal = terminal;
    }

    public IReadOnlyList<JobEventFrame> History { get; }

    public bool Terminal { get; }

    public ChannelReader<JobEventFrame>? Reader => _channel?.Reader;

    public void Dispose() => _owner.Unsubscribe(_channel);
}

/// <summary>单个后台任务的状态、进度、结果与 SSE 事件流。</summary>
public sealed class JobRecord : IDisposable
{
    private const int HistoryLimit = 2000;
    private readonly object _gate = new();
    private readonly List<JobEventFrame> _history = new();
    private readonly List<Channel<JobEventFrame>> _subscribers = new();

    internal JobRecord(string id) => Id = id;

    public string Id { get; }

    public JobState State { get; private set; } = JobState.Running;

    public JobProgress? Progress { get; private set; }

    public object? Result { get; private set; }

    public string? Error { get; private set; }

    public CancellationTokenSource Cancellation { get; } = new();

    public bool IsTerminal => State != JobState.Running;

    public void Report(JobProgress progress)
        => Publish(new JobEventFrame(Frame("progress", progress), false), () => Progress = progress);

    internal void Complete(object? result)
        => Publish(new JobEventFrame(Frame("done", new JobDonePayload(result)), true), () =>
        {
            State = JobState.Done;
            Result = result;
        });

    internal void Fail(string message, string? code)
        => Publish(new JobEventFrame(Frame("error", new ErrorBody(message, code)), true), () =>
        {
            State = JobState.Failed;
            Error = message;
        });

    internal void Canceled(string message)
        => Publish(new JobEventFrame(Frame("error", new ErrorBody(message, "canceled")), true), () =>
        {
            State = JobState.Canceled;
            Error = message;
        });

    public void RequestCancel()
    {
        if (IsTerminal)
        {
            return;
        }

        try
        {
            Cancellation.Cancel();
        }
        catch (ObjectDisposedException)
        {
        }
    }

    public JobSubscription Subscribe()
    {
        lock (_gate)
        {
            if (IsTerminal)
            {
                return new JobSubscription(this, null, _history.ToArray(), true);
            }

            var channel = Channel.CreateBounded<JobEventFrame>(new BoundedChannelOptions(4096)
            {
                FullMode = BoundedChannelFullMode.DropOldest,
                SingleReader = true,
                SingleWriter = false,
            });
            _subscribers.Add(channel);
            return new JobSubscription(this, channel, _history.ToArray(), false);
        }
    }

    internal void Unsubscribe(Channel<JobEventFrame>? channel)
    {
        if (channel is null)
        {
            return;
        }

        lock (_gate)
        {
            _subscribers.Remove(channel);
        }

        channel.Writer.TryComplete();
    }

    public void Dispose() => Cancellation.Dispose();

    private void Publish(JobEventFrame frame, Action update)
    {
        List<Channel<JobEventFrame>> subscribers;
        lock (_gate)
        {
            if (IsTerminal)
            {
                return;
            }

            update();
            _history.Add(frame);
            if (_history.Count > HistoryLimit)
            {
                _history.RemoveRange(0, _history.Count - HistoryLimit);
            }

            subscribers = new List<Channel<JobEventFrame>>(_subscribers);
            if (frame.Terminal)
            {
                _subscribers.Clear();
            }
        }

        foreach (var subscriber in subscribers)
        {
            subscriber.Writer.TryWrite(frame);
            if (frame.Terminal)
            {
                subscriber.Writer.TryComplete();
            }
        }
    }

    private static string Frame(string eventName, object payload)
        => "event: " + eventName + "\ndata: " + JsonSerializer.Serialize(payload, payload.GetType(), JsonDefaults.Options) + "\n\n";
}

/// <summary>后台任务注册表：创建、查询、取消，任务结束 10 分钟后清理。</summary>
public sealed class JobManager : IDisposable
{
    public static readonly TimeSpan Retention = TimeSpan.FromMinutes(10);

    private readonly ConcurrentDictionary<string, JobRecord> _jobs = new(StringComparer.Ordinal);
    private readonly ServerLog _log;

    public JobManager(ServerLog log) => _log = log;

    public JobRecord Start(string kind, Func<JobRecord, CancellationToken, Task<object?>> work)
    {
        var job = new JobRecord("job-" + Guid.NewGuid().ToString("n")[..12]);
        _jobs[job.Id] = job;
        _log.Info("后台任务已创建：" + kind + "（" + job.Id + "）");
        _ = RunAsync(job, kind, work);
        return job;
    }

    public bool TryGet(string id, [NotNullWhen(true)] out JobRecord? job) => _jobs.TryGetValue(id, out job);

    public JobStateDto Snapshot(JobRecord job) => new(job.Id, ToWire(job.State), job.Progress?.ToDto(), job.Result, job.Error);

    public static string ToWire(JobState state) => state switch
    {
        JobState.Running => "running",
        JobState.Done => "done",
        JobState.Failed => "failed",
        JobState.Canceled => "canceled",
        _ => "running",
    };

    public void Dispose()
    {
        foreach (var job in _jobs.Values)
        {
            job.RequestCancel();
            job.Dispose();
        }

        _jobs.Clear();
    }

    private async Task RunAsync(JobRecord job, string kind, Func<JobRecord, CancellationToken, Task<object?>> work)
    {
        try
        {
            var result = await work(job, job.Cancellation.Token).ConfigureAwait(false);
            job.Complete(result);
            _log.Info("后台任务完成：" + kind + "（" + job.Id + "）");
        }
        catch (OperationCanceledException) when (job.Cancellation.IsCancellationRequested)
        {
            job.Canceled("任务已取消");
            _log.Warn("后台任务已取消：" + kind + "（" + job.Id + "）");
        }
        catch (NeteaseApiException ex)
        {
            job.Fail(ex.Message, "netease_error");
            _log.Error("后台任务失败：" + kind + "（" + job.Id + "）：" + ex.Message);
        }
        catch (Exception ex)
        {
            job.Fail(ex.Message, "job_failed");
            _log.Error("后台任务失败：" + kind + "（" + job.Id + "）：" + ex.Message);
        }
        finally
        {
            _ = CleanupAsync(job);
        }
    }

    private async Task CleanupAsync(JobRecord job)
    {
        try
        {
            await Task.Delay(Retention).ConfigureAwait(false);
        }
        catch
        {
            // ignore
        }

        _jobs.TryRemove(job.Id, out _);
        job.Dispose();
    }
}

/// <summary>把 IProgress&lt;JobProgress&gt; 直接转给 JobRecord（同步、有序）。</summary>
public sealed class JobProgressSink : IProgress<JobProgress>
{
    private readonly JobRecord _job;

    public JobProgressSink(JobRecord job) => _job = job;

    public void Report(JobProgress value) => _job.Report(value);
}
