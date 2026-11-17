import 'dart:async';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_acrylic/flutter_acrylic.dart' as acrylic;
import 'package:lunatic_princess/app.dart';
import 'package:lunatic_princess/src/rust/api/app.dart' as rust;
import 'package:lunatic_princess/src/rust/frb_generated.dart';
import 'package:window_manager/window_manager.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  await acrylic.Window.initialize();
  // 隐藏系统画的最小化 / 最大化 / 关闭按钮：标题栏隐藏后它们仍会画出来，但点击落在
  // 自绘标题栏的拖动区域上，按不动；改用 WindowButtons 里自绘的按钮。
  await acrylic.Window.hideWindowControls();

  final window = WindowManager.instance.getCurrent()!;
  window.title = 'Lunatic Princess';
  window.titleBarStyle = TitleBarStyle.hidden; // 自绘标题栏
  window.setSize(const Size(1280, 800).toNative(), false);
  window.minimumSize = const Size(960, 600).toNative();
  window.center();

  // 关闭流程：第一次关闭请求被拦下，先让 Rust 侧把登录态落盘（否则可能丢掉
  // 刚轮换的 Cookie），最多等 3 秒，再放行第二次 close()。
  var closing = false;
  window.addListener((event) async {
    if (event is! WindowCloseRequestedEvent || closing) return;
    event.request.cancel();
    closing = true;
    try {
      await rust.shutdown().timeout(const Duration(seconds: 3));
    } catch (_) {}
    window.close();
  });

  runApp(const LunaticPrincessApp());
  window.show();
  window.focus();
}
