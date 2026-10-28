using System.Threading.Channels;
using LunaticPrincess.Server.Contracts;

namespace LunaticPrincess.Server.Infrastructure;

/// <summary>内存环形日志缓冲（最多 1000 条），并支持 /api/logs 的 SSE 订阅者推送。</summary>
public sealed class ServerLog
{
    private const int Capacity = 1000;
    private readonly object _gate = new();
    private readonly LinkedList<LogEntryDto> _entries = new();
    private readonly List<Channel<LogEntryDto>> _subscribers = new();

    public void Write(string level, string message)
    {
        var entry = new LogEntryDto(DateTimeOffset.Now.ToString("O"), level, message);
        var targets = new List<Channel<LogEntryDto>>();
        lock (_gate)
        {
            _entries.AddLast(entry);
            while (_entries.Count > Capacity)
            {
                _entries.RemoveFirst();
            }

            targets.AddRange(_subscribers);
        }

        foreach (var channel in targets)
        {
            channel.Writer.TryWrite(entry);
        }
    }

    public void Info(string message) => Write("info", message);

    public void Warn(string message) => Write("warn", message);

    public void Error(string message) => Write("error", message);

    public IReadOnlyList<LogEntryDto> Snapshot()
    {
        lock (_gate)
        {
            return _entries.ToArray();
        }
    }

    public LogSubscription Subscribe()
    {
        var channel = Channel.CreateBounded<LogEntryDto>(new BoundedChannelOptions(1024)
        {
            FullMode = BoundedChannelFullMode.DropOldest,
            SingleReader = true,
            SingleWriter = false,
        });
        lock (_gate)
        {
            _subscribers.Add(channel);
        }

        return new LogSubscription(this, channel);
    }

    private void Unsubscribe(Channel<LogEntryDto> channel)
    {
        lock (_gate)
        {
            _subscribers.Remove(channel);
        }

        channel.Writer.TryComplete();
    }

    public sealed class LogSubscription : IDisposable
    {
        private readonly ServerLog _owner;
        private readonly Channel<LogEntryDto> _channel;

        internal LogSubscription(ServerLog owner, Channel<LogEntryDto> channel)
        {
            _owner = owner;
            _channel = channel;
        }

        public ChannelReader<LogEntryDto> Reader => _channel.Reader;

        public void Dispose() => _owner.Unsubscribe(_channel);
    }
}
