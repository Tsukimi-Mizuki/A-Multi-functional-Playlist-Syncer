using Microsoft.Extensions.Logging;

namespace LunaticPrincess.Server.Infrastructure;

/// <summary>把 ILogger 输出汇入 ServerLog，供 /api/logs 推送；框架自身的信息级日志不转发，避免刷屏。</summary>
public sealed class ServerLogProvider : ILoggerProvider
{
    private readonly ServerLog _log;

    public ServerLogProvider(ServerLog log) => _log = log;

    public ILogger CreateLogger(string categoryName) => new ServerLogLogger(_log, categoryName);

    public void Dispose()
    {
    }

    private sealed class ServerLogLogger : ILogger
    {
        private readonly ServerLog _log;
        private readonly bool _frameworkCategory;

        public ServerLogLogger(ServerLog log, string categoryName)
        {
            _log = log;
            _frameworkCategory = categoryName.StartsWith("Microsoft", StringComparison.Ordinal)
                || categoryName.StartsWith("System.Net.Http", StringComparison.Ordinal);
        }

        public IDisposable? BeginScope<TState>(TState state) where TState : notnull => null;

        public bool IsEnabled(LogLevel logLevel)
            => logLevel != LogLevel.None && (!_frameworkCategory || logLevel >= LogLevel.Warning);

        public void Log<TState>(LogLevel logLevel, EventId eventId, TState state, Exception? exception,
            Func<TState, Exception?, string> formatter)
        {
            if (!IsEnabled(logLevel))
            {
                return;
            }

            var message = formatter(state, exception);
            if (exception is not null)
            {
                message = message + " | " + exception.Message;
            }

            var level = logLevel switch
            {
                LogLevel.Warning => "warn",
                LogLevel.Error => "error",
                LogLevel.Critical => "error",
                _ => "info",
            };
            _log.Write(level, message);
        }
    }
}
