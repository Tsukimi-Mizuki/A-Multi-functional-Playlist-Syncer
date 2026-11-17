/// 把毫秒时长格式化成 `m:ss`；无效（<= 0）时显示 `--:--`。
String formatDuration(int ms) {
  if (ms <= 0) return '--:--';
  final totalSeconds = (ms / 1000).round();
  final m = totalSeconds ~/ 60;
  final s = totalSeconds % 60;
  return '$m:${s.toString().padLeft(2, '0')}';
}

/// 毫秒时间戳 -> 本地时间 `HH:mm:ss`。
String formatClock(int timestampMs) {
  final t = DateTime.fromMillisecondsSinceEpoch(timestampMs);
  String two(int v) => v.toString().padLeft(2, '0');
  return '${two(t.hour)}:${two(t.minute)}:${two(t.second)}';
}
