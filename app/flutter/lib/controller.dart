import 'dart:async';
import 'dart:convert';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge.dart';
import 'package:lunatic_princess/log_merge.dart';
import 'package:lunatic_princess/src/rust/api/app.dart';
import 'package:lunatic_princess/src/rust/api/download.dart';
import 'package:lunatic_princess/src/rust/api/library.dart';
import 'package:lunatic_princess/src/rust/api/library.dart' as bridge;
import 'package:lunatic_princess/src/rust/api/login.dart';
import 'package:lunatic_princess/src/rust/api/playlists.dart';
import 'package:lunatic_princess/src/rust/api/settings.dart';
import 'package:lunatic_princess/src/rust/api/settings.dart' as bridge;

/// 窗口材质：云母 / 亚克力 / 无（纯色背景）。
///
/// 名字与 `settings.json` 里的 `windowMaterial` 一致（`lp-core` 按枚举成员名序列化）。
enum WindowMaterial {
  mica('云母（Mica）', 'Mica'),
  acrylic('亚克力（Acrylic）', 'Acrylic'),
  none('无', 'None');

  const WindowMaterial(this.label, this.serializedName);

  final String label;

  /// 写进设置文件时的值。
  final String serializedName;

  static WindowMaterial fromName(String name) => switch (name.trim().toLowerCase()) {
        'acrylic' => WindowMaterial.acrylic,
        'none' => WindowMaterial.none,
        _ => WindowMaterial.mica,
      };
}

/// 主题设置（与 `lp-core` 的 `ThemeMode` 同名）。
const _kThemeModeNames = {
  ThemeMode.system: 'System',
  ThemeMode.light: 'Light',
  ThemeMode.dark: 'Dark',
};

ThemeMode _themeModeFromName(String name) {
  final lower = name.trim().toLowerCase();
  if (lower == 'light') return ThemeMode.light;
  if (lower == 'dark') return ThemeMode.dark;
  return ThemeMode.system;
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

  // ---- 设置 ----
  SettingsDto? settings;
  bool settingsLoading = false;
  bool settingsSaving = false;
  String? settingsError;

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

  // ---- 本地曲库 ----
  /// 当前歌单与本地曲库的对比结果（`matches` 与 `detail.songs` 按顺序一一对应）。
  MatchOutcomeDto? match;
  String? matchError;
  /// 扫描 / 对比的进度（`total` 为 0 表示还在统计文件数）。
  ScanProgressDto? scanProgress;
  /// 是否正在扫描或对比。
  bool libraryBusy = false;
  /// 上一次扫描的摘要（设置页与歌单页展示用）。
  LibrarySummaryDto? librarySummary;
  /// 一次性提示（取消、完成之类），界面显示一次后由 [clearNotice] 清掉。
  String? notice;
  int _matchSeq = 0;

  // ---- 下载 / 同步 ----
  /// 正在进行的任务：类别、歌曲总数与要处理的歌曲 ID。
  TransferStartedDto? transferStarted;
  /// 最近一条逐首歌进度。
  TransferProgressDto? transferProgress;
  /// 最近一次任务的汇总（成功后保留到用户关掉提示）。
  TransferSummaryDto? transferSummary;
  /// 任务无法开始 / 无法完成的中文原因。
  String? transferError;
  /// 是否有下载 / 同步任务在运行。
  bool transferBusy = false;
  /// 曲目表格里选中的行下标（同一首歌在歌单里可能出现两次，所以按行算）。
  final Set<int> selectedRows = <int>{};
  /// Shift 连选的起点行。
  int? selectionAnchor;

  // ---- 日志 ----
  List<LogLine> logs = const [];

  // ---- 外观（由设置推出来，界面只改设置）----
  WindowMaterial material = WindowMaterial.mica;
  ThemeMode themeMode = ThemeMode.system;

  StreamSubscription<LogEntryDto>? _logSub;
  StreamSubscription<AccountDto>? _accountSub;
  StreamSubscription<ScanProgressDto>? _progressSub;
  StreamSubscription<TransferProgressDto>? _transferProgressSub;
  StreamSubscription<TransferStartedDto>? _transferStartedSub;
  bool _disposed = false;

  /// 启动序列：loadApp -> 设置 -> 订阅事件流 -> 补齐日志 -> 校验登录 -> 拉歌单。
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

    await loadSettings();

    // 先订阅再补齐历史，避免两者之间漏日志；重叠部分靠 id 去重。
    _logSub = watchLogs().listen(
      (e) => _mergeLogs([e]),
      onError: (Object _) {},
    );
    _accountSub = watchAccount().listen(_setAccount, onError: (Object _) {});
    _progressSub = bridge.watchLibraryProgress().listen(
      (p) {
        scanProgress = p;
        _notify();
      },
      onError: (Object _) {},
    );
    _transferStartedSub = watchTransferStarted().listen(
      (started) {
        transferStarted = started;
        _notify();
      },
      onError: (Object _) {},
    );
    _transferProgressSub = watchTransferProgress().listen(
      (progress) {
        transferProgress = progress;
        _notify();
      },
      onError: (Object _) {},
    );
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

  // ---- 设置 ----

  /// 读一次设置，并把主题与窗口材质应用上去。
  Future<void> loadSettings() async {
    settingsLoading = true;
    settingsError = null;
    _notify();
    try {
      final loaded = await bridge.loadSettings();
      settings = loaded;
      _applyAppearance(loaded.settings);
    } catch (e) {
      settingsError = e.toString();
    }
    settingsLoading = false;
    _notify();
  }

  /// 提交改动过的设置键；成功时用返回的设置覆盖本地状态。
  ///
  /// `patch` 只放改动的键（`lp-core` 会把未出现的键保持原值，旧外观键也不会被写回）。
  Future<bool> saveSettings(Map<String, Object?> patch) async {
    if (patch.isEmpty) return true;
    settingsSaving = true;
    settingsError = null;
    _notify();
    var ok = false;
    try {
      final saved = await bridge.saveSettings(patch: jsonEncode(patch));
      settings = saved;
      _applyAppearance(saved.settings);
      ok = true;
    } catch (e) {
      settingsError = e.toString();
    }
    settingsSaving = false;
    _notify();
    return ok;
  }

  void _applyAppearance(AppSettingsDto s) {
    material = WindowMaterial.fromName(s.windowMaterial);
    themeMode = _themeModeFromName(s.theme);
  }

  Future<void> setMaterial(WindowMaterial m) async {
    if (m == material) return;
    material = m;
    _notify();
    await saveSettings({'windowMaterial': m.serializedName});
  }

  Future<void> setThemeMode(ThemeMode m) async {
    if (m == themeMode) return;
    themeMode = m;
    _notify();
    await saveSettings({'theme': _kThemeModeNames[m] ?? 'System'});
  }

  void clearNotice() {
    if (notice == null) return;
    notice = null;
    _notify();
  }

  // ---- 本地曲库 ----

  /// 扫描设置里的曲库目录。
  Future<void> scanLibrary() async {
    if (libraryBusy) return;
    libraryBusy = true;
    scanProgress = null;
    matchError = null;
    _notify();
    try {
      final result = await bridge.libraryScan();
      final summary = result.summary;
      if (summary != null) {
        librarySummary = summary;
        notice = '曲库扫描完成：${summary.totalFiles} 个文件'
            '${summary.ncmFiles > 0 ? '，其中 ${summary.ncmFiles} 个 NCM 待解密' : ''}';
      } else if (result.error?.kind == 'cancelled') {
        notice = '已取消扫描';
      } else {
        matchError = result.error?.message ?? '扫描失败';
      }
    } catch (e) {
      matchError = e.toString();
    }
    libraryBusy = false;
    scanProgress = null;
    _notify();
  }

  /// 把当前歌单与本地曲库对比；`rescan` 为真时丢弃旧的扫描结果重新扫描。
  Future<void> matchLocal({bool rescan = false}) async {
    final d = detail;
    if (d == null || libraryBusy) return;
    final seq = ++_matchSeq;
    libraryBusy = true;
    scanProgress = null;
    matchError = null;
    _notify();
    try {
      final result = await bridge.libraryMatch(playlistId: d.id, rescan: rescan);
      if (seq != _matchSeq) return;
      final outcome = result.outcome;
      if (outcome != null) {
        match = outcome;
        librarySummary = LibrarySummaryDto(
          directory: outcome.libraryDirectory,
          totalFiles: outcome.totalFiles,
          ncmFiles: outcome.ncmFiles,
          skippedDirectories: 0,
        );
      } else if (result.error?.kind == 'cancelled') {
        notice = '已取消对比';
      } else {
        matchError = result.error?.message ?? '对比失败';
      }
    } catch (e) {
      if (seq == _matchSeq) matchError = e.toString();
    }
    if (seq != _matchSeq) return;
    libraryBusy = false;
    scanProgress = null;
    _notify();
  }

  /// 取消正在进行的扫描或对比。
  Future<void> cancelLibrary() async {
    try {
      await bridge.libraryCancel();
    } catch (e) {
      matchError = e.toString();
      _notify();
    }
  }

  // ---- 账号与歌单 ----

  void _setAccount(AccountDto a) {
    final was = account.loggedIn;
    account = a;
    if (!a.loggedIn) {
      // 退出 / 登录失效：清空歌单，并离开已不可见的歌单页。
      _playlistsSeq++;
      _detailSeq++;
      _matchSeq++;
      playlists = const [];
      playlistsLoading = false;
      playlistsError = null;
      detail = null;
      detailLoading = false;
      detailError = null;
      match = null;
      matchError = null;
      libraryBusy = false;
      selectedRows.clear();
      selectionAnchor = null;
      transferStarted = null;
      transferProgress = null;
      transferSummary = null;
      transferError = null;
      transferBusy = false;
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
    match = null;
    matchError = null;
    selectedRows.clear();
    selectionAnchor = null;
    transferStarted = null;
    transferProgress = null;
    transferSummary = null;
    transferError = null;
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
    // 设置了「打开歌单后自动对比」时就顺手对比一次（使用已有的扫描结果，不重复扫盘）。
    if (seq == _detailSeq &&
        detail != null &&
        (settings?.settings.autoMatchLocal ?? false)) {
      unawaited(matchLocal());
    }
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

  // ---- 下载 / 同步 ----

  /// 选中 / 取消选中一行：Ctrl 切换、Shift 连选（与 Tauri 版表格规则一致）。
  void selectRow(int index, {bool control = false, bool shift = false}) {
    if (shift && selectionAnchor != null) {
      final start = selectionAnchor! < index ? selectionAnchor! : index;
      final end = selectionAnchor! < index ? index : selectionAnchor!;
      if (!control) selectedRows.clear();
      selectedRows.addAll([for (var i = start; i <= end; i++) i]);
    } else if (control) {
      if (!selectedRows.remove(index)) selectedRows.add(index);
      selectionAnchor = index;
    } else {
      selectedRows
        ..clear()
        ..add(index);
      selectionAnchor = index;
    }
    _notify();
  }

  /// 清除多选。
  void clearSelection() {
    if (selectedRows.isEmpty) return;
    selectedRows.clear();
    selectionAnchor = null;
    _notify();
  }

  /// 选中行的歌曲 ID，按行号顺序。
  List<int> selectedTrackIds() {
    final songs = detail?.songs ?? const <SongDto>[];
    final rows = selectedRows.where((row) => row < songs.length).toList()..sort();
    return [for (final row in rows) songs[row].id];
  }

  /// 计算下载缺失 / 一键同步确认框里的计划。
  ///
  /// 去重与判定都在 lp-core 里，与真正执行任务时用的是同一条规则。
  Future<TransferPlanResultDto?> requestTransferPlan() async {
    final d = detail;
    if (d == null) return null;
    try {
      return await transferPlan(playlistId: d.id);
    } catch (e) {
      transferError = e.toString();
      _notify();
      return null;
    }
  }

  /// 跑一次下载 / 同步任务，并把结果收进界面状态。
  Future<void> _runTransfer(Future<TransferResultDto> Function() start) async {
    if (transferBusy) return;
    transferBusy = true;
    transferError = null;
    transferSummary = null;
    transferProgress = null;
    transferStarted = null;
    _notify();
    try {
      final result = await start();
      final summary = result.summary;
      if (summary == null) {
        final error = result.error;
        if (error != null && error.kind == 'cancelled') {
          notice = '已取消任务';
        } else {
          transferError = error?.message ?? '任务失败';
        }
      } else {
        transferSummary = summary;
        if (summary.cancelled) notice = '任务已取消';
        // 任务结束后 lp-core 会重新扫描曲库，这里再对比一次让状态列跟上。
        if (summary.libraryRefreshed) unawaited(matchLocal());
      }
    } catch (e) {
      transferError = e.toString();
    }
    transferBusy = false;
    transferProgress = null;
    _notify();
  }

  /// 下载选中的歌曲。
  Future<void> downloadSelectedTracks() async {
    final d = detail;
    final ids = selectedTrackIds();
    if (d == null || ids.isEmpty) return;
    await _runTransfer(
      () => downloadTracks(playlistId: d.id, trackIds: Int64List.fromList(ids)),
    );
  }

  /// 下载本地缺失的歌曲；trackIds 是确认框里仍然勾选的歌曲。
  Future<void> downloadMissingTracks({List<int>? trackIds}) async {
    final d = detail;
    if (d == null) return;
    await _runTransfer(
      () => downloadMissing(
        playlistId: d.id,
        trackIds: trackIds == null ? null : Int64List.fromList(trackIds),
      ),
    );
  }

  /// 一键同步整个歌单。
  Future<void> syncCurrentPlaylist() async {
    final d = detail;
    if (d == null) return;
    await _runTransfer(() => syncPlaylist(playlistId: d.id));
  }

  /// 取消正在进行的下载 / 同步。
  Future<void> cancelTransfer() async {
    try {
      await downloadCancel();
    } catch (e) {
      transferError = e.toString();
      _notify();
    }
  }

  /// 关掉结果提示。
  void clearTransferResult() {
    if (transferSummary == null && transferError == null) return;
    transferSummary = null;
    transferError = null;
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
    _progressSub?.cancel();
    _transferProgressSub?.cancel();
    _transferStartedSub?.cancel();
    super.dispose();
  }
}
