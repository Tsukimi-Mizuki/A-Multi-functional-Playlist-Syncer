/// 日志条目的最小抽象，方便不依赖 Rust 桥做单测。
class LogLine {
  const LogLine(this.id, this.level, this.message, this.timestampMs);

  final BigInt id;
  final String level;
  final String message;
  final int timestampMs;
}

/// 把 [incoming] 并入 [current]：按 id 去重（先到先得）、按 id 升序、只留最后 [limit] 条。
///
/// 启动时 `recentLogs()` 与 `watchLogs()` 会有重叠，所以必须按 id 合并。
List<LogLine> mergeLogs(
  List<LogLine> current,
  Iterable<LogLine> incoming, {
  int limit = 400,
}) {
  final byId = <BigInt, LogLine>{for (final l in current) l.id: l};
  for (final l in incoming) {
    byId.putIfAbsent(l.id, () => l);
  }
  final merged = byId.values.toList()..sort((a, b) => a.id.compareTo(b.id));
  if (merged.length > limit) {
    return merged.sublist(merged.length - limit);
  }
  return merged;
}
