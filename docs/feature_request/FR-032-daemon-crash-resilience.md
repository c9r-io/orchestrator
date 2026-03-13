# FR-032: Daemon 进程崩溃韧性与 Worker 存活保障

## 状态

Proposed

## 优先级

P1 — 影响 self-bootstrap / self-evolution 长周期任务的可靠执行

## 背景

### 问题发现

2026-03-13 执行 `follow-logs-callback-execution.md` 测试计划期间，daemon 进程在 Cycle 2 的 `qa_testing` 阶段静默退出，导致：

1. 两个正在运行的 `qa_testing` agent 进程被孤立
2. 对应 task items（order 103、104）永久停留在 `running` 状态
3. 整个 self-bootstrap 任务无法正常完成

### 崩溃现场

- **最后日志**：`2026-03-13T05:31:32Z` — worker 2 claimed child task `cecdf664`，该 task 因 `workflow not found: qa_only in project 'cs-qa'` 立即失败
- **无崩溃日志**：daemon 无 panic trace、无 shutdown event、无 SIGTERM 记录，进程静默消失
- **macOS crash report**：`/Library/Logs/DiagnosticReports/` 无当次 crash 记录

### 代码分析

`crates/daemon/src/main.rs` 中的 worker 循环（L445-541）存在以下脆弱点：

1. **Worker panic 导致 worker 退出**（L515-518）：`catch_unwind` 捕获 panic 后，worker 执行 `break` 退出循环，**永久失去一个并发槽位**。没有 worker 重生机制。

```rust
Err(panic) => {
    error!(worker = worker_num, %task_id, "task panicked");
    drop(panic);
    break; // worker 永久退出
}
```

2. **RestartRequestedError 触发全局 shutdown**（L507-511）：一个 worker 遇到 restart 请求后，调用 `request_shutdown()` 终止所有 worker，包括正在处理其他任务的 worker。

3. **无 worker 健康监控**：main 函数在 gRPC server 完成后等待 worker handles（L351-364），但无周期性检查 worker 是否存活。如果所有 worker 都因 panic `break` 退出，gRPC server 仍在运行但无法处理任何任务。

4. **子任务失败路径**：`qa_testing` 步骤通过 CLI 创建子任务，daemon worker 认领子任务后执行 `run_task_loop`。当子任务 workflow 不存在时，`run_task_loop` 返回 `Err`（非 panic），worker 继续循环（L513: `error!` + 不 break）。此路径本身不应导致崩溃，但可能存在 panic 路径尚未被代码分析覆盖。

### 可能的崩溃原因

由于无 crash dump，以下为按可能性排序的推测：

1. **OOM Kill**：MacBook Air 16GB 内存，同时运行多个 claude agent 进程 + daemon + cargo 编译，系统可能 SIGKILL 了 daemon
2. **未捕获的 panic**：虽然 `run_task_loop` 有 `catch_unwind`，但 worker loop 本身的其他 await 点（如 `claim_next_pending_task`、`emit_daemon_event`）如果 panic 则不受保护
3. **Tokio runtime 异常**：所有 worker task 退出后，runtime 可能决定终止

## 需求

### 核心需求

**N1. Worker 自动重生**：当 worker 因 panic 退出后，daemon 应在短暂延迟（如 2s）后自动启动替代 worker，维持 `--workers N` 的并发度。

**N2. Worker 健康监控**：daemon 应周期性检查存活 worker 数量，当低于配置值时发出告警事件并触发重生。

**N3. Worker loop panic 全覆盖**：worker 的整个 loop body（不仅是 `run_task_loop` 调用）应被 `catch_unwind` 保护，确保单次迭代 panic 不会终止 worker。

### 辅助需求

**A1. Daemon 崩溃日志**：注册 `panic hook` 将 panic 信息写入 `data/daemon_crash.log`（在 stdout 可能不可用时仍可追溯）。

**A2. 崩溃恢复事件**：daemon 启动时检测 `data/daemon.pid` 中记录的 PID 是否仍存活，若不存活说明上次是非正常退出，emit `daemon_crash_recovered` 事件。

**A3. Worker 存活指标**：在 `runtime_snapshot` 中新增 `total_worker_restarts` 计数器，记录 worker 重生次数。

### 非目标

- 不改变 `self_restart` 的 exec() 热重载机制
- 不引入外部进程监控工具（如 systemd、launchd）
- 不变更 gRPC API 接口

## 涉及文件

| 文件 | 变更类型 |
|------|---------|
| `crates/daemon/src/main.rs` | Worker loop 重构：panic 重生、loop body 全覆盖 catch_unwind |
| `core/src/runtime.rs` | 新增 `total_worker_restarts` 计数器 |
| `crates/daemon/src/lifecycle.rs` | 启动时检测 stale PID、emit crash_recovered 事件 |

## 验收标准

1. Worker panic 后 2s 内自动重生，`runtime_snapshot` 中 `live_workers` 恢复到配置值
2. `total_worker_restarts` 计数器正确递增
3. daemon 启动时若检测到上次非正常退出，events 表出现 `daemon_crash_recovered` 事件
4. panic hook 将 crash 信息写入 `data/daemon_crash.log`
5. 回归：正常 shutdown（SIGTERM）行为不变，worker 仍能优雅退出
6. `cargo test --workspace --lib` 通过

## 参考

- 测试计划：`docs/plan/follow-logs-callback-execution.md`
- Worker 循环：`crates/daemon/src/main.rs:445-541`
- Shutdown 逻辑：`crates/daemon/src/main.rs:320-388`
- Runtime state：`core/src/runtime.rs`
