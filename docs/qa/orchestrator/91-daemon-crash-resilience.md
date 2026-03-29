# QA: Daemon 进程崩溃韧性与 Worker 存活保障 (FR-032)

验证 daemon worker 崩溃恢复、健康监控、crash 日志与启动恢复机制是否正确工作。

## 前置条件

- orchestrator 已编译并可运行
- 项目根目录为 `{source_tree}`

## 场景 1: Worker Panic 自动恢复（代码审查）

**步骤:**
1. 检查 `crates/daemon/src/main.rs` 中 `worker_loop()` 函数
2. 确认 `worker_iteration()` 调用被 `AssertUnwindSafe(...).catch_unwind()` 包裹

**预期结果:**
- [ ] panic 发生时 worker 不执行 `break`，而是 `continue` 继续循环
- [ ] panic 恢复前调用 `record_worker_restart()` 递增计数器
- [ ] panic 恢复后 sleep 2s 防止紧密 panic 循环
- [ ] 发出 `worker_panic_recovered` 事件

## 场景 2: Worker Supervisor 健康检查与重生（代码审查）

**步骤:**
1. 检查 `crates/daemon/src/main.rs` 中 `worker_supervisor()` 函数

**预期结果:**
- [ ] supervisor 每 30s 检查所有 worker handle 是否 `is_finished()`
- [ ] 检测到已退出的 worker 后，sleep 2s 再重新 spawn
- [ ] 重生的 worker 使用相同的 state、shutdown receiver、restart sender
- [ ] 当 `live_workers < configured_workers` 时发出警告日志

## 场景 3: Panic Hook 写入 Crash 日志（代码审查）

**步骤:**
1. 检查 `crates/daemon/src/main.rs` 中 panic hook 安装代码

**预期结果:**
- [ ] `std::panic::set_hook()` 在 daemon 启动早期安装
- [ ] panic 信息追加写入 `data/daemon_crash.log`（非覆盖）
- [ ] 日志格式包含 epoch 时间戳：`[epoch=...] {panic_info}`
- [ ] 写入后仍调用 default hook，不吞掉标准 panic 输出

## 场景 4: 启动时 Stale PID 检测与 Crash Recovery 事件（代码审查）

**步骤:**
1. 检查 `crates/daemon/src/lifecycle.rs` 中 `detect_stale_pid()` 函数
2. 检查 `crates/daemon/src/main.rs` 中 daemon 启动序列

**预期结果:**
- [ ] `detect_stale_pid()` 读取 `data/daemon.pid`，用 `nix::sys::signal::kill(pid, None)` 检查进程是否存活
- [ ] PID 文件存在但进程已死 → 返回 `true`（stale）
- [ ] stale PID 检测到时发出 `daemon_crash_recovered` 事件
- [ ] 新 PID 随后写入 `data/daemon.pid`

## 场景 5: Runtime Snapshot 包含 Worker 重启计数器

**步骤:**
1. 检查 `core/src/runtime.rs` 中 `DaemonRuntimeSnapshot` 结构体

**预期结果:**
- [ ] 包含 `total_worker_restarts: u64` 字段
- [ ] `record_worker_restart()` 使用 `fetch_add(1, SeqCst)` 原子递增
- [ ] `snapshot()` 方法正确读取并返回该计数器

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | 2026-03-29 | Worker panic 恢复、supervisor 重生、crash 日志、stale PID 检测、runtime snapshot 计数器 |

