# FR-019: 修复 libc 类型编译错误

**优先级**: P0
**状态**: Proposed
**目标**: 恢复全平台构建

## 背景与目标

项目在特定平台（macOS / Linux musl / cross-compile 目标）上存在 `libc` crate 类型不匹配或缺失的编译错误，导致 CI 或开发者本地构建失败。这直接阻断了全平台交付能力，属于 P0 级别的构建可用性问题。

目标：

- 定位并修复所有因 `libc` 类型引发的编译错误，确保 `cargo build --workspace` 在所有目标平台上通过。
- 统一 `libc` crate 的版本与 feature flag 声明，消除平台条件编译中的遗漏分支。
- CI 矩阵覆盖 `x86_64-unknown-linux-gnu`、`x86_64-unknown-linux-musl`、`aarch64-unknown-linux-gnu`、`x86_64-apple-darwin`、`aarch64-apple-darwin` 五个目标。

非目标：

- 引入新的 FFI 绑定或 native 依赖。
- 重构现有平台抽象层（仅修复编译错误，不改变架构）。

## 问题分析

需排查以下常见根因：

1. `libc` crate 版本升级后部分类型签名变更（如 `libc::c_int` → `core::ffi::c_int`）。
2. `#[cfg(target_os = "...")]` 条件编译遗漏某些平台分支。
3. `Cargo.toml` 中 `libc` 的 feature flag（如 `extra_traits`）未在所有 workspace member 中一致声明。
4. cross-compile 时 `libc` 与 `nix` / `mio` 等 crate 的版本不兼容。

## 实施方案

### 第一步：诊断

- 在 CI 中对所有目标平台执行 `cargo check --workspace --target <triple>`，收集完整错误列表。
- 按错误类型分类：类型不匹配、缺失符号、feature 缺失。

### 第二步：修复

- 统一 workspace 级 `libc` 版本（`[workspace.dependencies]`）。
- 补全缺失的 `#[cfg]` 分支，使用 `cfg_if` 简化多平台条件编译。
- 必要时引入类型别名适配层，隔离平台差异。

### 第三步：验证

- 本地 cross-compile 验证（使用 `cross` 工具）。
- CI 矩阵全量通过。

## CLI / API 影响

无。本 FR 为纯构建修复，不涉及用户可见接口变更。

## 关键设计决策与权衡

### 统一 workspace 级依赖版本 vs 各 crate 独立声明

选择 workspace 级统一管理，避免版本分裂导致的隐性不兼容。代价是某些 crate 可能引入不必要的 feature，但构建一致性优先。

### 修复范围最小化

仅修复编译错误，不借机重构平台抽象层。降低回归风险，缩短修复周期。

## 风险与缓解

风险：修复某一平台的编译错误可能引入其他平台的回归。
缓解：CI 矩阵覆盖所有目标平台，修复后全量验证。

风险：`libc` 版本升级可能与其他 native crate 冲突。
缓解：使用 `cargo tree -d -i libc` 检查依赖冲突，必要时 pin 版本。

## 验收标准

- `cargo build --workspace` 在所有 5 个目标平台上通过。
- `cargo test --workspace` 无因 `libc` 类型引发的测试失败。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- CI 矩阵全平台绿色。
- 无新增 `#[allow(unused)]` 或 `#[allow(clippy::*)]` suppress。
