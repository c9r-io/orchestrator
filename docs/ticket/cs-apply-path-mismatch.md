# C/S 模式下 db reset 未同步 daemon 内存状态

## 状态：已修复

## 问题描述

在 C/S 架构下，`orchestrator db reset --include-config` 通过 RPC 清除了 SQLite 中的 config 数据，但 daemon 的内存 `RwLock<ActiveConfig>` 未被同步重置。在 `db reset` 和下一次 `apply` 之间，daemon 仍持有旧的 config 状态。

对比：`apply` 的实现 (`persist_config_and_reload`) 在写入 SQLite 后会通过 `write_active_config(state)` 原子更新内存。`db_reset` 缺少这一步。

## 实际遇到的场景

self-evolution 执行过程中：

1. 旧 daemon（用旧二进制启动）正在运行
2. 修复了 project-only config 验证 bug 后重新构建
3. 使用旧 CLI symlink（指向已废弃的 `core/target/release/agent-orchestrator`）执行操作
4. 旧单体二进制绕过 daemon 直接写入 SQLite，daemon 内存未更新
5. `task create` 发给旧 daemon，报错

## 根因

1. **代码层面**：`run_db_reset` 只清除 SQLite，未同步重置 daemon 内存中的 `ActiveConfig`
2. **操作层面**：使用了已废弃的旧单体二进制（`core/target/release/agent-orchestrator`），该二进制绕过 daemon RPC 直接操作数据库

## 修复内容

1. **代码修复**（已完成）：`run_db_reset` 在 `include_config` 时同步清空 daemon 内存中的 `ActiveConfig`、`active_config_error`、`active_config_notice`
   - 文件：`core/src/service/system.rs`
2. **旧二进制清理**（已完成）：`core` crate 已是纯 library crate（无 `main.rs`、无 `[[bin]]`），不再产生独立二进制。`core/target/release/agent-orchestrator` 为历史构建残留物，已标记为废弃
3. **文档对齐**（已完成）：执行模板和 skill 文档统一使用新 CLI 路径 `target/release/orchestrator`

## 严重性

低。正常操作流程中 `db reset --include-config` 后必然紧跟 `apply`，不一致窗口很短。
