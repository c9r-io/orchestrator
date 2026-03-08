# C/S 模式下 db/init/apply 的执行路径不一致

## 问题描述

在 C/S 架构下，`orchestrator db reset`、`orchestrator init`、`orchestrator apply` 存在两条执行路径：

1. **通过 daemon RPC**（新 CLI `target/release/orchestrator`）：CLI 发送 gRPC 请求给 daemon，daemon 执行操作并原子更新内存中的 `ActiveConfig`。
2. **直接操作 SQLite**（旧单体 CLI `core/target/release/agent-orchestrator`）：绕过 daemon，直接写入数据库。daemon 的内存状态不会更新。

## 实际遇到的场景

self-evolution 执行过程中：

1. 旧 daemon（用旧二进制启动）正在运行
2. 修复了 project-only config 验证 bug 后重新构建
3. 使用旧 CLI symlink（指向 `core/target/release/`）执行 `db reset` + `init` + `apply`
4. 旧 CLI 直接写入 SQLite，旧 daemon 的内存 config 未更新
5. `task create` 使用新 CLI 发给旧 daemon，旧 daemon 仍用旧代码验证，报错

## 根因

- 执行模板文档中的启动步骤没有明确区分 CLI 二进制版本
- `db reset` / `init` 在新 CLI 中是否走 RPC 尚不确定——如果它们仍然直接操作 SQLite（因为这些是 bootstrap 操作，daemon 可能还没启动），则存在 daemon 内存状态与磁盘状态不一致的窗口

## 建议修复

1. **文档层面**（已完成）：执行模板中明确使用新 CLI 二进制路径，并说明 apply 通过 RPC 热加载不需要重启
2. **代码层面**：确认 `db reset` / `init` 在 C/S 模式下的行为：
   - 如果 daemon 未运行，直接操作 SQLite 是合理的
   - 如果 daemon 已运行，应通过 RPC 通知 daemon reload，或至少给出警告
3. **CLI 层面**：当 daemon 正在运行时，`db reset` 和 `init` 应检测并警告用户（类似 k8s 的 `kubectl` 不会让你直接操作 etcd）

## 严重性

中。不影响功能正确性，但会导致操作者困惑和浪费调试时间。
