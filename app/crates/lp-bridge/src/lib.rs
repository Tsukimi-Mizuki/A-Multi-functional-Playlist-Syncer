//! Lunatic Princess 的 Flutter 桥接层：把 `lp-core` 的能力经 flutter_rust_bridge 交给 Dart。
//!
//! 与 Tauri 外壳（`src-tauri`）同一个角色：只做组合根、参数转换与事件推送，业务逻辑一律在 `lp-core`。
//! [`api`] 下的公开函数会被代码生成器扫描，生成 `app/flutter/lib/src/rust/` 里的 Dart 绑定；
//! [`state`] 是不对 Dart 公开的内部状态。

pub mod api;
// 生成的代码每次 `flutter_rust_bridge_codegen generate` 都会整份重写，不按本仓库的 rustfmt 风格排版。
#[rustfmt::skip]
mod frb_generated;
mod state;
