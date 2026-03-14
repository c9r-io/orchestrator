# FR-040: QA Agent 子进程绕过 Daemon PID Guard 杀死 Daemon

**Priority**: P1
**Status**: Proposed
**Date**: 2026-03-14
**Discovered**: self-bootstrap 监控测试 (follow-logs-callback-refactor)

## 问题描述

FR-034 实现的 Daemon PID Guard (`guard_daemon_pid_kill`) 仅在 `spawn.rs` 层对**顶层命令字符串**进行模式匹配。
当 QA testing 步骤的顶层命令是 `claude -p "..."` 时，Guard 不会拦截，
但 Claude agent 内部会自主执行 `pkill -9 -f orchestratord`（例如 53-client-server-architecture 的 Scenario 1: Daemon Startup and Shutdown）。

这导致 QA agent 在 self-bootstrap 流程中成功杀死了正在执行任务的 daemon 进程，产生连锁故障：
1. 所有正在运行的步骤被中断（task_paused: app_shutdown）
2. ticket_fix、align_tests、doc_governance 等后续步骤未执行
3. 任务最终以 `failed` 状态结束，3 个 item 标记为 `unresolved`

### 实际事件证据

```
# QA tester (53-client-server-architecture) 的行为：
TOOL Bash: pkill -f orchestratord   → 未被拦截，daemon 仍存活
TOOL Bash: kill 36747               → 未被拦截
TOOL Bash: pkill -9 -f orchestratord → 成功杀死 daemon

# 无 daemon_pid_kill_blocked 事件记录
```

## 根因分析

1. **Guard 只检查 spawn 入口的命令字符串**：`spawn.rs:69-73` 对 `extra_env["ORCHESTRATOR_DAEMON_PID"]` 存在时调用 `guard_daemon_pid_kill(command, daemon_pid)`，但 `command` 是 `claude -p "..."` 而非 agent 内部执行的子命令。

2. **QA doc frontmatter 过滤未生效**：`53-client-server-architecture.md` 标记了 `self_referential_safe: false`，但 qa_doc_gen 的 regression_targets 仍然选中了它，prehook 层未因此跳过。

3. **Agent 沙箱模式不匹配**：QA testing 使用 `host` execution profile（非 sandbox），没有进程级限制。

## 建议方案

### 方案 A: Prehook 层 frontmatter 过滤（短期，推荐）

在 qa_testing 步骤的 prehook 中增加对 item 的 `self_referential_safe` frontmatter 检查：
- 如果当前 workspace 是 self-referential 且 QA doc 标记了 `self_referential_safe: false`，则 skip 该 item
- 不影响非 self-referential 场景下的全量 QA 测试

### 方案 B: Agent 提示注入 PID 保护指令（中期）

在 self-referential 场景下，向 QA testing agent 的 system prompt 注入禁止杀 daemon 的约束：
```
CRITICAL: This test is running inside the orchestrator daemon.
Do NOT execute any command that kills, stops, or restarts the orchestratord process.
Daemon PID: {daemon_pid}
```
依赖 LLM 的遵循性，不保证 100% 有效。

### 方案 C: 进程级沙箱隔离（长期）

将 QA testing 的 execution profile 从 `host` 切换到 sandbox 模式，在 macOS seatbelt 或 Linux seccomp 层面限制 `kill` 系统调用对 daemon PID 的操作。

## 验收标准

1. 在 self-bootstrap 流程中，QA agent 不能杀死 daemon 进程
2. 非 self-referential 场景不受影响
3. 被拦截/跳过的 QA item 产出 `step_skipped` 事件而非 false failure ticket
