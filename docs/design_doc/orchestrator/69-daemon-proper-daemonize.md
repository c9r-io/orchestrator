# 69 — orchestratord 真正 Daemon 化

> FR-057 | P1 | 2026-03-16

## 背景

`orchestratord` 的 `--foreground` 标志被解析但从未使用，daemon 始终以前台进程运行。关闭终端后 shell 发送 SIGHUP 导致 daemon 退出，不符合长驻后台服务的预期。

## 设计决策

### 1. Double-Fork Daemonization

采用标准 Unix double-fork 模式（`crates/daemon/src/daemonize.rs`）：

```
orchestratord
  ├─ fork() → 父进程打印 PID 后 exit(0)
  │    └─ setsid() → 成为新 session leader
  │         └─ fork() → 脱离 session leader
  │              ├─ stdin → /dev/null
  │              ├─ stdout/stderr → data/daemon.log (append)
  │              └─ 进入主循环
```

选择 double-fork 而非 single-fork 是为了防止进程意外重新获取控制终端。

### 2. `--foreground` 模式

带 `--foreground` 时跳过 fork/setsid，日志输出到 stdout（保留 ANSI 颜色）。适用于：
- Docker 容器（需要前台进程）
- systemd `Type=simple`
- 开发调试

### 3. SIGHUP 处理

`lifecycle.rs` 的 `shutdown_signal()` 改为循环模式：
- SIGHUP → 记录日志，继续运行
- SIGINT/SIGTERM → 触发优雅关闭

### 4. 日志重定向

Daemon 模式下 stdout/stderr 通过 `dup2()` 重定向到 `data/daemon.log`，并禁用 tracing 的 ANSI 输出。现有 `data/daemon_crash.log`（panic hook）保持不变。

### 5. CLI daemon stop/status

新增 `orchestrator daemon stop` 和 `orchestrator daemon status` 子命令（`crates/cli/src/commands/daemon.rs`）：
- **本地操作**：直接读取 PID 文件和发送信号，不依赖 gRPC 连接
- `stop`：读取 `data/daemon.pid` → 发送 SIGTERM → 等待 30s 超时
- `status`：读取 PID 文件 → 检查进程存活 → 输出状态

## 涉及模块

| 模块 | 改动 |
|------|------|
| `crates/daemon/src/daemonize.rs` | 新文件 — double-fork + stdio 重定向 |
| `crates/daemon/src/main.rs` | 根据 `--foreground` 决定是否 daemonize；ANSI 开关 |
| `crates/daemon/src/lifecycle.rs` | SIGHUP 循环忽略 |
| `crates/daemon/Cargo.toml` | nix features 增加 `process`, `fs` |
| `crates/cli/src/cli.rs` | 新增 `Daemon(DaemonCommands)` |
| `crates/cli/src/commands/daemon.rs` | 新文件 — stop/status 实现 |
| `crates/cli/src/commands/mod.rs` | daemon 模块注册 |
| `crates/cli/src/main.rs` | Daemon 命令在 gRPC 连接前分发 |

## 向后兼容

| 场景 | 影响 |
|------|------|
| `orchestratord --foreground` | 行为不变 |
| `orchestratord`（无 flag） | **行为变更**：从前台变为后台 |
| Docker / systemd | 容器 entrypoint 和 systemd unit 应使用 `--foreground` |
| Kubernetes | Pod entrypoint 已使用 `--foreground`，不受影响 |
