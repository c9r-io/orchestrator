---
self_referential_safe: false
---
# Agent Subprocess Daemon PID Guard

**Module**: orchestrator
**Scope**: Verify that agent (Claude Code) subprocesses cannot kill the orchestrator daemon via internal Bash tool calls
**Scenarios**: 4

---

## Scenario 1: CLAUDE.md injection in self-referential mode

**前置条件**
- Workspace 配置 `self_referential: true`
- Daemon 正在运行，PID 已知

**步骤**
1. 触发 agent 步骤执行（或直接调用 `inject_agent_daemon_guard()`）
2. 检查 workspace 下 `.claude/CLAUDE.md` 文件

**预期结果**
- `.claude/CLAUDE.md` 存在
- 包含 `<!-- orchestrator-daemon-guard -->` 标记
- 包含当前 daemon PID
- 包含 "NEVER run `kill`" 等安全指令

---

## Scenario 2: PreToolUse hook injection and settings merge

**前置条件**
- Workspace 配置 `self_referential: true`
- `.claude/settings.local.json` 可能已有 permissions 配置

**步骤**
1. 触发 agent 步骤执行
2. 检查 `.claude/settings.local.json`
3. 检查 `.claude/daemon-pid-guard.sh`

**预期结果**
- `settings.local.json` 包含 `hooks.PreToolUse` 数组
- 数组中有 `matcher: "Bash"` 条目指向 `daemon-pid-guard.sh`
- 原有 permissions 配置保留不变
- `daemon-pid-guard.sh` 存在且可执行（mode 0755）

---

## Scenario 3: Hook script blocks kill command

**前置条件**
- `.claude/daemon-pid-guard.sh` 已注入，daemon PID 为 12345

**步骤**
1. 模拟 Claude Code PreToolUse 输入：
   ```bash
   echo '{"tool_name":"Bash","tool_input":{"command":"kill 12345"}}' | .claude/daemon-pid-guard.sh
   ```
2. 检查 exit code 和 stdout

**预期结果**
- Exit code: 2
- stdout 包含 `"continue":false`
- stdout 包含 `stopReason` 描述

**反向验证**
1. 输入安全命令：
   ```bash
   echo '{"tool_name":"Bash","tool_input":{"command":"echo hello"}}' | .claude/daemon-pid-guard.sh
   ```
2. Exit code: 0, stdout 包含 `"continue":true`

---

## Scenario 4: Idempotent re-injection updates PID

**前置条件**
- 已注入一次 guard（PID 111）

**步骤**
1. 再次注入 guard（PID 222）
2. 检查 `.claude/CLAUDE.md`
3. 检查 `.claude/settings.local.json`

**预期结果**
- CLAUDE.md 仅包含一个 guard block（2 个 marker）
- Guard block 显示 PID 222，不含 PID 111
- settings.local.json 仅包含一个 daemon-pid-guard hook 条目
