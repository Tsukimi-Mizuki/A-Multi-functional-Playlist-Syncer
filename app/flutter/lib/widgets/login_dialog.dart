import 'dart:async';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:lunatic_princess/src/rust/api/login.dart';

Future<void> showLoginDialog(BuildContext context) {
  return showDialog<void>(
    context: context,
    builder: (_) => const _LoginDialog(),
  );
}

class _LoginDialog extends StatefulWidget {
  const _LoginDialog();

  @override
  State<_LoginDialog> createState() => _LoginDialogState();
}

class _LoginDialogState extends State<_LoginDialog> {
  QrStartDto? _qr;
  String _status = '正在获取二维码…';
  bool _expired = false;
  bool _failed = false;
  Timer? _timer;
  bool _polling = false;
  int _transientErrors = 0;
  // 每次刷新二维码递增，用来丢弃旧二维码的在途轮询结果。
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    unawaited(_refresh());
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  Future<void> _refresh() async {
    final gen = ++_generation;
    _timer?.cancel();
    setState(() {
      _qr = null;
      _expired = false;
      _failed = false;
      _transientErrors = 0;
      _status = '正在获取二维码…';
    });
    try {
      final qr = await qrStart();
      if (!mounted || gen != _generation) return;
      setState(() {
        _qr = qr;
        _status = '请用网易云音乐 App 扫码';
      });
      _timer = Timer.periodic(const Duration(seconds: 2), (_) => _poll(gen));
    } catch (e) {
      if (!mounted || gen != _generation) return;
      setState(() {
        _failed = true;
        _status = '获取二维码失败：$e';
      });
    }
  }

  Future<void> _poll(int gen) async {
    final qr = _qr;
    if (_polling || qr == null || gen != _generation) return;
    _polling = true;
    try {
      final r = await qrPoll(key: qr.key);
      if (!mounted || gen != _generation) return;
      switch (r.state) {
        case 'waiting':
          _transientErrors = 0;
          setState(() => _status = '请用网易云音乐 App 扫码');
        case 'scanned':
          _transientErrors = 0;
          setState(() => _status = '已扫码，请在手机上确认');
        case 'expired':
          _timer?.cancel();
          setState(() {
            _expired = true;
            _status = '二维码已过期';
          });
        case 'success':
          _timer?.cancel();
          Navigator.of(context).pop();
        default: // failed / error
          if (r.transient && ++_transientErrors <= 3) return;
          _timer?.cancel();
          setState(() {
            _failed = true;
            _status = r.message ?? '登录失败';
          });
      }
    } catch (e) {
      if (!mounted || gen != _generation) return;
      if (++_transientErrors <= 3) return;
      _timer?.cancel();
      setState(() {
        _failed = true;
        _status = '登录失败：$e';
      });
    } finally {
      _polling = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final qr = _qr;
    return ContentDialog(
      constraints: const BoxConstraints(maxWidth: 380),
      title: const Text('扫码登录'),
      // 二维码与状态文字在对话框里水平居中（Column 自己只有内容那么宽）。
      content: Center(
        heightFactor: 1,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            SizedBox(
              width: 220,
              height: 220,
              child: qr == null
                  ? Center(
                      child: _failed
                          ? const WindowsIcon(
                              WindowsIcons.error_badge,
                              size: 40,
                            )
                          : const ProgressRing(),
                    )
                  : Opacity(
                      opacity: _expired ? 0.25 : 1,
                      // 二维码自带白底，深色主题下也能被扫描。
                      child: SvgPicture.string(qr.svg, width: 220, height: 220),
                    ),
            ),
            const SizedBox(height: 16),
            Text(_status, textAlign: TextAlign.center),
          ],
        ),
      ),
      actions: [
        Button(onPressed: _refresh, child: const Text('刷新二维码')),
        Button(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('取消'),
        ),
      ],
    );
  }
}
