# Design Doc 58: Agent Subprocess Daemon PID Guard

## 背景

FR-046 揭示了 self-bootstrap echo-command-test 期间的两层防线同时失效问题：

1. **CEL prehook `self_referential_safe` filter**：QA 文档 `58-control-plane-security.md` 缺少 `self_referential_safe: false` 标记，导致 prehook 放行
2. **`guard_daemon_pid_kill`**：仅在 `ShellRunnerExecutor::spawn()` 层面拦截顶层命令字符串，对 agent (Claude Code) 进程内部的 Bash tool 调用零可见性

结果：agent 在执行 QA 场景时通过内部 Bash tool 执行 `kill <daemon_pid>`，成功杀死了管理自身任务的 daemon。

## 设计方案

### 子任务 A: QA 文档 frontmatter 补全

扫描 `docs/qa/orchestrator/` 下所有涉及 daemon 进程管理（kill、pkill、killall、daemon.pid）的文档，补全 `self_referential_safe: false` YAML frontmatter。

补全文档清单：
- `87-self-referential-daemon-pid-guard.md`
- `86-orphaned-running-items-recovery.md`
- `85-daemon-crash-resilience.md`
- `60-daemon-lifecycle-runtime-metrics.md`
- `96-self-restart-socket-continuity.md`
- `91-daemon-crash-resilience.md`

### 子任务 B: Agent 子进程层 CLAUDE.md + Hooks 注入

在 `phase_runner/setup.rs` 的 self-referential 分支中，agent 进程启动前向 workspace 注入两层防护：

**Layer 1: `.claude/CLAUDE.md` 软防护**
- 写入 daemon PID 和安全规则指令
- 利用 Claude Code 的 CLAUDE.md 自动加载机制
- 提供意图层面的指导（LLM instruction following）

**Layer 2: `.claude/settings.local.json` hooks 硬防护**
- 注入 `PreToolUse` hook，匹配 `Bash` tool
- Hook 脚本 `.claude/daemon-pid-guard.sh` 读取 stdin JSON，提取 command 字段
- 使用 shell case 模式匹配 kill 命令是否指向 daemon PID 或 orchestratord
- 匹配则输出 `{"continue":false,"stopReason":"..."}` 并 exit 2 阻断执行

### 实现位置

- `core/src/scheduler/phase_runner/agent_guard.rs`：注入逻辑（CLAUDE.md、hook 脚本、settings）
- `core/src/scheduler/phase_runner/setup.rs:86-96`：调用点
- 幂等设计：重复注入时更新 PID、去重 hook 条目
- 容错设计：注入失败仅 `tracing::warn`，不阻断步骤执行

## 三层防线架构

| 层级 | 机制 | 拦截点 | 防护强度 |
|------|------|--------|----------|
| L1 | CEL prehook `self_referential_safe` | 步骤调度前 | 硬（跳过整个步骤） |
| L2 | `guard_daemon_pid_kill` | 进程 spawn 时 | 硬（检查顶层命令） |
| L3 | Claude Code PreToolUse hook | agent 内部 Bash 调用时 | 硬（拦截子命令） |
| L3' | CLAUDE.md 安全指令 | agent LLM 推理时 | 软（依赖指令遵从） |

## 清理策略

- Worktree 隔离模式：注入文件随 worktree 自动清理
- 共享 workspace：文件持续存在，对 self-referential 模式无害
- 无需显式清理逻辑
