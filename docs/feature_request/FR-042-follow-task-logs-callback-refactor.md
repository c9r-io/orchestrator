# FR-042: follow_task_logs 流式回调重构 — gRPC TaskFollow 端点从空流变为真实日志流

**Priority**: P1
**Status**: Proposed
**Date**: 2026-03-14
**Discovered**: 代码审查 + self-bootstrap 课题验证

## 问题描述

`follow_task_logs_stream()`（`core/src/service/task.rs:139-158`）接收 `send_fn` 回调参数但完全忽略它（参数名 `_send_fn`），直接委托给 `follow_task_logs()`，后者通过 `print!()`/`eprint!()` 将日志写入 stdout/stderr（`core/src/scheduler/query/log_stream.rs:280-282`）。

这导致 gRPC `TaskFollow` 端点（`crates/daemon/src/server/task.rs:341-374`）虽然搭建了 streaming channel 并传入了 `send_fn`，但日志实际走了 daemon 进程的 stdout 而非 channel。远程客户端通过 gRPC `TaskFollow` 只能收到一个空流。

### 当前行为

| 维度 | 现状 |
|------|------|
| `_send_fn` 参数 | 被忽略（下划线前缀） |
| 日志输出目标 | daemon 进程的 stdout/stderr |
| gRPC `TaskFollow` 流 | 空流（无数据） |
| CLI `task logs --follow` | 正常输出到终端（因为共享 stdout） |
| 代码标记 | `task.rs:154` 有 `TODO: Phase 3` 注释 |

### 影响范围

- gRPC 远程客户端无法实时观察任务日志
- `orchestrator task watch` 的日志面板在 C/S 模式下不可用
- 集成测试（`crates/integration-tests/src/lib.rs:423-453`）设置了相同的 callback 但同样被忽略

## 涉及文件

| 文件 | 当前问题 | 预期变更 |
|------|---------|---------|
| `core/src/scheduler/query/log_stream.rs` | `follow_task_logs` 无 callback 参数；`follow_one_stream` 直接 `print!/eprint!` | 增加 `output_fn` callback 参数；通过 callback 输出 |
| `core/src/service/task.rs` | `_send_fn` 被忽略；TODO 注释 | 正确传递 `send_fn`；删除 TODO |
| `crates/daemon/src/server/task.rs` | callback 被下层忽略 | 无变更（已正确传入 `send_fn`） |
| `crates/integration-tests/src/lib.rs` | 同上 | 无变更 |

## 建议方案

### 核心重构

1. **`follow_task_logs` 签名变更**

```rust
// Before
pub async fn follow_task_logs(state: &InnerState, task_id: &str) -> Result<()>

// After
pub async fn follow_task_logs<F>(
    state: &InnerState,
    task_id: &str,
    mut output_fn: F,
) -> Result<()>
where
    F: FnMut(String, bool) -> anyhow::Result<()>,  // (line, is_stderr)
```

使用同步 `FnMut` 而非异步回调，避免 `Arc<F>` + `Send` + `tokio::spawn` 的复杂性。`follow_task_logs` 的调用者本身就在 async 上下文中，可以在外层处理异步发送。

2. **`follow_one_stream` 改用 callback**

```rust
// Before
if stderr { eprint!("{text}"); } else { print!("{text}"); }

// After
output_fn(text, stderr)?;
```

3. **`follow_task_logs_stream` 正确接线**

```rust
// Before
pub async fn follow_task_logs_stream<F, Fut>(state, task_id, _send_fn: F)
    follow_task_logs(state, &resolved_id).await

// After
pub async fn follow_task_logs_stream<F, Fut>(state, task_id, send_fn: F)
    follow_task_logs(state, &resolved_id, |line, _is_stderr| {
        // 同步发送到 channel（使用 try_send 或 blocking_send）
        let _ = /* channel send */;
        Ok(())
    }).await
```

4. **CLI 本地 follow 保持现有行为**

所有直接调用 `follow_task_logs` 的 CLI 路径传入 stdout-writing callback：

```rust
follow_task_logs(state, task_id, |line, is_stderr| {
    if is_stderr { eprint!("{}", line); } else { print!("{}", line); }
    Ok(())
}).await
```

### 设计约束

1. 不引入新依赖
2. 使用同步 `FnMut` callback 而非异步——避免 self-bootstrap 实测中发现的 `Arc<F>` not `Send` 问题
3. 保持 `follow_one_stream` 的 redaction、phase 切换检测、task 终态检测逻辑不变
4. 不改 proto 定义（`TaskFollowRequest`/`TaskLogLine` 已存在）
5. 不改 `stream_task_logs_impl` 的历史日志查询行为

## Self-Bootstrap 验证结果

本课题已通过 self-bootstrap 流程验证（task `f8ceb89f`），主要发现：

1. **Plan 阶段**：architect 正确识别根因和方案，未偏题
2. **qa_doc_gen**：正确识别 3 个回归测试目标（02, 53, 65），未创建新 QA 文档
3. **Implement**：成功重构了 `follow_task_logs` 和 `follow_one_stream`，但在 `follow_task_logs_stream` 中使用了 `Arc<F>` + `tokio::spawn` 异步模式，导致 `Send` trait 约束问题
4. **self_test**：编译通过（Cycle 1 的 self_test 仅检查了 `cargo check`）
5. **qa_testing (Cycle 2)**：发现了 `task.rs` 的 `Arc<F>` not `Send` 编译问题，创建了 ticket
6. **align_tests**：修复了编译问题，但 ticket 未被清理（参见 pipeline ordering ticket）

**关键教训**：应使用同步 `FnMut` callback 而非 `async Fn` + `Arc`，这样 `follow_task_logs_stream` 只需在外层处理异步发送逻辑。

## 验收标准

1. `follow_task_logs` 接受 callback 参数并通过 callback 输出所有日志行
2. `follow_one_stream` 不再直接调用 `print!/eprint!`
3. `follow_task_logs_stream` 正确使用 `send_fn`，`_send_fn` 参数去掉下划线
4. `task.rs` 中 `TODO: Phase 3` 注释已删除
5. CLI `task logs --follow` 行为不变（仍输出到终端）
6. gRPC `TaskFollow` 端点能通过 channel 接收实时日志行
7. 新增至少 2 个单元测试覆盖 callback 模式
8. `cargo test --workspace --lib` 全部通过
9. `cargo clippy --workspace` 无新 warning

## Self-Bootstrap 适用性

本课题适合作为 self-bootstrap 课题再次执行，建议调整：
- 在执行计划中注明使用同步 `FnMut` 而非 `async Fn` + `Arc`
- 确保先修复 pipeline ordering ticket（`ticket_fix` prehook timing），否则同周期 ticket 仍无法被自动修复
