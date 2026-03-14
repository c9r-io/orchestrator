# FR-041: Self-Restart 后 Socket 连接断裂导致后续步骤不可达

**Priority**: P1
**Status**: Proposed
**Date**: 2026-03-14
**Discovered**: self-bootstrap 监控测试 (follow-logs-callback-refactor)

## 问题描述

self_restart 步骤通过 `exec()` 热重载 daemon binary 后，Unix domain socket (`data/orchestrator.sock`) 变得不可连接。
CLI 客户端（包括 orchestrator daemon 内部的子进程调用）收到 `Connection refused (os error 61)`，
导致 self_restart 之后的所有 CLI 交互失败。

### 观察到的现象

1. self_restart 在 Cycle 1 结束时执行（exit_code=101，restart=false，因为 build 失败了）
2. daemon PID 文件 (`data/daemon.pid`) 更新为新 PID `36747`，但该进程已不存在
3. 旧 daemon 进程 PID `24255` 仍在运行，但 socket 无法接受连接
4. 重新编译 CLI 后仍然无法连接
5. 需要手动 kill + restart daemon 才能恢复

```
$ ./target/release/orchestrator task trace <task_id>
Error: failed to connect to daemon at data/orchestrator.sock
Caused by: Connection refused (os error 61)

$ cat data/daemon.pid
36747           # ← 进程不存在

$ ps -p 24255
PID COMMAND
24255 ./target/release/orchestratord --foreground --workers 2  # ← 实际进程
```

### 影响

- Cycle 2 的 qa_testing 完成后，daemon 被 QA agent 杀死（FR-040）
- 手动重启 daemon 后，resume task 检测到 `max_cycles_enforced`，无法继续执行 ticket_fix
- 最终 3 个 item 标记为 `unresolved`，2 张 ticket 未修复

## 根因分析

1. **exec() 语义与 PID 文件不一致**：self_restart 写入新 PID 到 `data/daemon.pid`，但如果 exec() 失败（如 build 失败），PID 文件指向的进程已不存在。旧进程继续运行但持有旧 socket。

2. **Socket 生命周期与进程解耦**：daemon 进程通过 exec() 重载后，需要重新 bind socket，但旧 socket 文件残留在文件系统上且不再监听。

3. **无自愈机制**：没有 socket 健康检查或自动重建 socket 的逻辑。CLI 一旦连接失败就只能报错。

## 建议方案

### 方案 A: exec() 失败时清理 PID 文件（短期，推荐）

如果 self_restart 的 build 阶段失败（exit_code != 0），不更新 PID 文件，不触发 exec()，保持原 daemon 继续运行。当前实现在 build 失败时仍然写了 PID 文件（导致 PID 指向不存在的进程）。

### 方案 B: Daemon 启动时 socket 自检与重建（中期）

在 daemon 启动和 exec() 重载后增加 socket 自检：
1. 启动时尝试 connect 自身 socket，如果残留的旧 socket 无法连接则删除重建
2. exec() 重载后立即重新 bind socket 并发出 ready 信号
3. CLI 连接失败时增加重试逻辑（最多 3 次，间隔 1s）

### 方案 C: 使用 TCP 端口替代 Unix Socket（长期备选）

Unix socket 的生命周期与进程绑定，exec() 后 socket FD 可能无效。使用 TCP 端口可以更好地支持 exec() 热重载场景，但引入网络暴露面。

## 验收标准

1. self_restart build 失败时，daemon 继续正常运行，CLI 可以连接
2. self_restart exec() 成功时，新 daemon 在 3 秒内可接受 CLI 连接
3. PID 文件始终指向可用的 daemon 进程
4. `orchestrator task resume` 在 daemon 可用时能正确恢复中断的任务
