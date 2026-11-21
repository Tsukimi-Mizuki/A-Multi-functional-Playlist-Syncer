import 'dart:io';

import 'package:file_selector/file_selector.dart';

/// 系统目录 / 文件对话框。Windows 上走系统的 IFileOpenDialog，和 Tauri 版的观感一致。
///
/// 取消和对话框本身出错（没有权限、路径已失效等）都返回 null：调用方保持原状即可，
/// 不值得为一次取消弹错误框。

const XTypeGroup _coverTypes = XTypeGroup(
  label: '图片',
  extensions: <String>['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp'],
);

/// 选一个目录。initialDirectory 不存在时忽略，免得对话框打不开。
Future<String?> pickDirectory({
  String? initialDirectory,
  String confirmButtonText = '选择文件夹',
}) async {
  try {
    return await getDirectoryPath(
      initialDirectory: _usableDirectory(initialDirectory),
      confirmButtonText: confirmButtonText,
    );
  } catch (_) {
    return null;
  }
}

/// 选一张封面图片。大小与文件头的校验交给 lp-core 的 read_cover_file。
Future<XFile?> pickCoverImage({String? initialDirectory}) async {
  try {
    return await openFile(
      acceptedTypeGroups: const <XTypeGroup>[_coverTypes],
      initialDirectory: _usableDirectory(initialDirectory),
      confirmButtonText: '选择封面',
    );
  } catch (_) {
    return null;
  }
}

String? _usableDirectory(String? path) {
  final trimmed = path?.trim() ?? '';
  if (trimmed.isEmpty) return null;
  return Directory(trimmed).existsSync() ? trimmed : null;
}
