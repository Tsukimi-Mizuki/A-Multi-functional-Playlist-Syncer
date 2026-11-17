import 'dart:async';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/log_merge.dart';
import 'package:lunatic_princess/src/rust/api/app.dart';
import 'package:lunatic_princess/src/rust/api/login.dart';
import 'package:lunatic_princess/src/rust/api/playlists.dart';

/// 窗口材质：云母 / 亚克力 / 无（纯色背景）。
enum WindowMaterial {
  mica('云母（Mica）'),
  acrylic('亚克力（Acrylic）'),
  none('无');

  const WindowMaterial(this.label);
  final String label;
}

/// 导航选中项的键：概览 / 日志 / 设置固定，歌单用 `pl:<id>`。
const kKeyOverview = 'overview';
const kKeyLog = 'log';
const kKeySettings = 'settings';
String playlistKey(int id) => 'pl:$id';

/// 全局状态（原型阶段不引入其它状态管理库）。
class AppController extends ChangeNotifier {
  // ---- 启动状态 ----
  AppStateDto? appState;
  String? startupError;
  bool starting = true;

  // ---- 账号 ----
  AccountDto account = const AccountDto(loggedIn: false, userId: 0, vipType: 0);
  ValidationDto? validation;

  // ---- 歌单 ----
  List<PlaylistDto> playlists = const [];
  bool playlistsLoading = false;
  String? playlistsError;

  String selectedKey = kKeyOverview;
  PlaylistDetailDto? detail;
  bool detailLoading = false;
  String? detailError;
  int _detailSeq = 0;
  int _playlistsSeq = 0;

  // ---- 日志 ----
  List<LogLine> logs = const [];

  // ---- 外观（原型阶段不持久化）----
  WindowMaterial material = WindowMaterial.mica;
  ThemeMode themeMode = ThemeMode.system;

  StreamSubscription<LogEntryDto>? _logSub;
  StreamSubscription<AccountDto>? _accountSub;
  bool _disposed = false;

  /// 启动序列：loadApp -> 订阅事件流 -> 补齐日志 -> 校验登录 -> 拉歌单。
  Future<void> start() async {
    try {
      appState = await loadApp();
    } catch (e) {
      startupError = e.toString();
      starting = false;
      _notify();
      return;
    }
    account = appState!.account;
    _notify();

    // 先订阅再补齐历史，避免两者之间漏日志；重叠部分靠 id 去重。
    _logSub = watchLogs().listen(
      (e) => _mergeLogs([e]),
      onError: (Object _) {},
    );
    _accountSub = watchAccount().listen(_setAccount, onError: (Object _) {});
    try {
      _mergeLogs(await recentLogs());
    } catch (_) {}

    try {
      validation = await validateLogin();
      _setAccount(validation!.account);
    } catch (e) {
      validation = ValidationDto(
        status: 'unverified',
        message: e.toString(),
        account: account,
      );
    }
    starting = false;
    _notify();
    if (account.loggedIn && playlists.isEmpty && !playlistsLoading) {
      unawaited(loadPlaylists());
    }
  }

  void _setAccount(AccountDto a) {
    final was = account.loggedIn;
    account = a;
    if (!a.loggedIn) {
      // 退出 / 登录失效：清空歌单，并离开已不可见的歌单页。
      _playlistsSeq++;
      _detailSeq++;
      playlists = const [];
      playlistsLoading = false;
      playlistsError = null;
      detail = null;
      detailLoading = false;
      detailError = null;
      if (selectedKey.startsWith('pl:')) selectedKey = kKeyOverview;
    } else if (!was) {
      unawaited(loadPlaylists());
    }
    _notify();
  }

  Future<void> loadPlaylists() async {
    final seq = ++_playlistsSeq;
    playlistsLoading = true;
    playlistsError = null;
    _notify();
    try {
      final list = await listPlaylists();
      if (seq != _playlistsSeq) return;
      playlists = list;
    } catch (e) {
      if (seq != _playlistsSeq) return;
      playlistsError = e.toString();
    }
    playlistsLoading = false;
    // 选中的歌单已不在列表里（被删除 / 取消收藏）时回到概览。
    if (selectedKey.startsWith('pl:') &&
        !playlists.any((p) => playlistKey(p.id) == selectedKey)) {
      selectedKey = kKeyOverview;
      detail = null;
    }
    _notify();
  }

  void select(String key) {
    if (key == selectedKey) return;
    selectedKey = key;
    if (key.startsWith('pl:')) {
      final id = int.tryParse(key.substring(3));
      if (id != null) unawaited(loadDetail(id));
    }
    _notify();
  }

  Future<void> loadDetail(int id) async {
    final seq = ++_detailSeq;
    detail = null;
    detailLoading = true;
    detailError = null;
    _notify();
    try {
      final d = await playlistTracks(id: id);
      if (seq != _detailSeq) return;
      detail = d;
    } catch (e) {
      if (seq != _detailSeq) return;
      detailError = e.toString();
    }
    detailLoading = false;
    _notify();
  }

  /// 退出登录；账号流会随后推送「未登录」，这里也直接清理一次以免等待。
  Future<void> doLogout() async {
    try {
      await logout();
    } catch (e) {
      playlistsError = e.toString();
      _notify();
      return;
    }
    _setAccount(const AccountDto(loggedIn: false, userId: 0, vipType: 0));
  }

  void setMaterial(WindowMaterial m) {
    if (m == material) return;
    material = m;
    _notify();
  }

  void setThemeMode(ThemeMode m) {
    if (m == themeMode) return;
    themeMode = m;
    _notify();
  }

  void _mergeLogs(Iterable<LogEntryDto> entries) {
    logs = mergeLogs(
      logs,
      entries.map((e) => LogLine(e.id, e.level, e.message, e.timestampMs)),
    );
    _notify();
  }

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _logSub?.cancel();
    _accountSub?.cancel();
    super.dispose();
  }
}
