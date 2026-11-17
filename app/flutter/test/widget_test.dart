import 'package:flutter_test/flutter_test.dart';
import 'package:lunatic_princess/format.dart';
import 'package:lunatic_princess/log_merge.dart';

LogLine line(int id, [String msg = 'm']) =>
    LogLine(BigInt.from(id), 'info', msg, 0);

void main() {
  group('formatDuration', () {
    test('formats m:ss', () {
      expect(formatDuration(215000), '3:35');
      expect(formatDuration(61000), '1:01');
      expect(formatDuration(5000), '0:05');
    });
    test('invalid shows placeholder', () {
      expect(formatDuration(0), '--:--');
      expect(formatDuration(-1), '--:--');
    });
  });

  group('mergeLogs', () {
    test('dedupes by id and sorts', () {
      final merged = mergeLogs([line(1), line(3)], [line(2), line(3, 'dup')]);
      expect(merged.map((l) => l.id.toInt()), [1, 2, 3]);
      expect(merged[2].message, 'm');
    });
    test('keeps only the last limit entries', () {
      final merged = mergeLogs([], [
        for (var i = 1; i <= 10; i++) line(i),
      ], limit: 4);
      expect(merged.map((l) => l.id.toInt()), [7, 8, 9, 10]);
    });
  });
}
