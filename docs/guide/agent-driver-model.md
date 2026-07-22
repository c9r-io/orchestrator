# Agent Driver Model / Agent Driver 使用指南

Agent Driver lets an Agent declare **which provider protocol it speaks** without putting provider CLI flags into workflow YAML. Workflows ask for behavior—multi-turn conversation, tools, approval events, session attachment, or workspace access—and Orchestrator rejects incompatible combinations before a task starts.

Use a driver when you want provider-neutral workflow definitions, structured tool/usage events, or safe provider session attachment. Keep `spec.command` when a script or existing one-shot Agent already works and does not need those features.

## Quick Start

### Generic shell

Shell is the compatibility driver. It still uses your command but makes driver ownership explicit:

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: shell-checker
spec:
  capabilities: [check]
  command: "./scripts/check.sh '{prompt}'"
  driver:
    provider: shell
    transport: cli
    shell:
      requirePromptPlaceholder: true
```

Shell is one-shot: it cannot satisfy multi-turn, hosted-tool, provider-session, or permission-event requirements.

### Claude CLI

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: claude-coder
spec:
  capabilities: [implement]
  driver:
    provider: claude
    transport: cli
    options:
      model: sonnet
      maxTurns: 8
      budgetCapUsd: 1.0
      permissionMode: ask
      allowedTools: [mcp__orch]
      timeoutSecs: 1800
    claude:
      thinkingBudgetTokens: 2048
```

Do not add a `command` to a Claude or Codex driver. The provider adapter owns command construction and safely delivers the prompt.

### Codex CLI

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: codex-reviewer
spec:
  capabilities: [review]
  driver:
    provider: codex
    transport: cli
    options:
      model: gpt-5-codex
      permissionMode: governed
      cwd: .
    codex:
      reasoningEffort: high
```

The current Codex CLI adapter is one-shot and does not host Orchestrator MCP tools or emit governed permission requests. Session attachment is supported for a later step during the same daemon lifetime.

## Declare What The Workflow Needs

Put requirements on the step, not the Agent name:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: governed-implementation
spec:
  steps:
    - id: implement
      type: implement
      required_capability: implement
      behavior:
        side_effect_class: workspace_only
        driverRequirements:
          multiTurn: true
          toolHosting: stdio
          sessionResume: true
          permissionEvents: true
          workspaceAccess: write
  loop:
    mode: once
```

Every enabled explicit driver that can satisfy `implement` is checked during apply. This prevents a healthy preferred Agent from hiding an incompatible fallback Agent.

| Requirement | Meaning |
|---|---|
| `multiTurn` | The workflow can send another user turn to the live provider session |
| `toolHosting` | `none`, `stdio`, or future authenticated `http` Orchestrator tools |
| `sessionResume` | A later step can attach to provider context |
| `permissionEvents` | Provider permission requests enter Attention for a human decision |
| `workspaceAccess` | `none`, `read`, or `write`; defaults to `write` for fail-closed safety |

Non-idempotent external steps additionally require guaranteed cancellation. CLI drivers satisfy this through process-group termination. SDK descriptors are not executable in this release and cannot mutate a workspace.

## Portable Options

| Field | Purpose |
|---|---|
| `model` | Provider model selector |
| `maxTurns` | Maximum turns for providers that support live input |
| `budgetCapUsd` | Provider cost ceiling when supported |
| `permissionMode` | `governed`, `ask`, or `deny` |
| `allowedTools` | Allowlisted Orchestrator tools |
| `cwd` | Workspace-relative working directory; absolute paths and `..` are rejected |
| `env` | Non-secret driver-local environment additions |
| `timeoutSecs` | Driver step timeout used when the workflow does not override it |

Put secrets in `SecretStore`-backed Agent `env`, not `driver.options.env`.

## Apply And Diagnose

Start with a dry run:

```bash
orchestrator apply --dry-run --project my-project -f agents.yaml
```

Driver errors have stable codes and a field path, for example:

```text
Diagnostic [driver_tool_hosting_required] at spec.steps[].behavior.driverRequirements: ...
```

Common fixes:

| Code | Fix |
|---|---|
| `driver_multi_turn_required` | Use Claude CLI or remove multi-turn semantics |
| `driver_tool_hosting_required` | Select a driver with the requested transport |
| `driver_permission_events_required` | Use a permission-event-capable driver or remove the approval gate |
| `driver_workspace_sandbox_required` | Use a sandboxable CLI driver |
| `driver_guaranteed_cancel_required` | Use a guaranteed-cancel driver or make the external operation idempotent |
| `driver_transport_unavailable` | Change `transport` to `cli` |

## Sessions, Permissions, And Events

- Provider session tokens are internal opaque values. They do not appear in CLI output, task DTOs, gRPC, logs, audit, or event payloads.
- Session attachment is available across steps only while the same daemon remains alive. After daemon restart, resume from an Orchestrator handoff/checkpoint and start a new provider session.
- A driver emits a permission request; it never approves it. The request becomes an Attention item, and the existing RBAC/audit path owns the decision.
- Tool calls, tool results, usage, assistant text, and terminal outcome appear in the task event timeline as normalized `driver_*` events.

## Unsafe Raw Arguments

Avoid `rawArgs`. If a provider feature is important and stable, add a typed driver option instead.

The escape hatch requires all of the following:

```yaml
driver:
  provider: codex
  transport: cli
  rawArgs: ["--experimental-feature"]
  unsafeRawArgs: true
```

- daemon started in unsafe mode;
- Admin authorization;
- canonical Action Audit context;
- explicit `unsafeRawArgs: true`.

The apply records action `agent.driver.raw_args.apply`. Never place a token or credential in `rawArgs`.

## Migration And Rollback

Migrate one capability pool at a time:

1. Add a new driver Agent with a temporary capability such as `implement_driver`.
2. Dry-run the workflow requirements.
3. Run a deterministic pilot and compare terminal state, evidence, sandbox behavior, and cost.
4. Move the production capability to the driver Agent.
5. Keep the legacy Agent disabled or on a fallback capability for one release window.

Rollback by restoring the workflow capability to the legacy command Agent and reapplying the old manifest. There is no driver database migration to reverse.

The complete runnable example is `fixtures/manifests/bundles/agent-driver-fixture.yaml`; the verification entry point is `scripts/qa/test-agent-driver-abstraction.sh`.

---

# 中文指南

Agent Driver 的作用，是让 Agent 声明“自己使用哪一种供应商协议”，而不是把 Claude/Codex 的命令行参数散落在 workflow YAML 中。Workflow 只声明需要的能力；如果 Agent 不支持，`apply` 会直接拒绝，任务不会进入运行态。

已有的一次性脚本可以继续使用 `spec.command`。当你需要结构化工具事件、多轮输入、权限审批或供应商 session 接力时，再迁移到 driver。

## 最小配置

通用 shell：

```yaml
spec:
  capabilities: [check]
  command: "./scripts/check.sh '{prompt}'"
  driver:
    provider: shell
    transport: cli
```

Claude：

```yaml
spec:
  capabilities: [implement]
  driver:
    provider: claude
    transport: cli
    options:
      model: sonnet
      maxTurns: 8
      budgetCapUsd: 1.0
      permissionMode: ask
      allowedTools: [mcp__orch]
      timeoutSecs: 1800
    claude:
      thinkingBudgetTokens: 2048
```

Codex：

```yaml
spec:
  capabilities: [review]
  driver:
    provider: codex
    transport: cli
    options:
      model: gpt-5-codex
      permissionMode: governed
    codex:
      reasoningEffort: high
```

Claude/Codex driver 不要再填写 `command`；命令构造、prompt 传递和 session 参数由 provider adapter 负责。

## Workflow 如何声明要求

```yaml
behavior:
  side_effect_class: workspace_only
  driverRequirements:
    multiTurn: true
    toolHosting: stdio
    sessionResume: true
    permissionEvents: true
    workspaceAccess: write
```

- `multiTurn`：同一个供应商 session 可以继续发送下一轮用户消息；
- `toolHosting`：是否需要 Orchestrator 托管工具；
- `sessionResume`：后续 step 是否需要接回供应商上下文；
- `permissionEvents`：供应商权限请求是否必须进入 Attention；
- `workspaceAccess`：`none/read/write`，默认是更保守的 `write`。

只要某个启用状态的候选 Agent 显式配置了 driver，它就必须满足这些要求。这样备用 Agent 不会等到故障切换时才暴露不兼容。

## 安全边界

所有 CLI driver 都继续走 Orchestrator 的统一进程路径，包括 runner policy、Daemon PID 防护、Seatbelt/Linux namespace、rlimit、环境变量白名单、脱敏和进程组终止。SDK 目前只是未来接口描述：不能执行，也不能承载 workspace 修改。

供应商 session token 是 runner 内部的不透明值：不会进入 gRPC、DTO、日志、Action Audit 或事件正文。同一 daemon 生命周期内可以跨 step 接力；daemon 重启后，应从 Orchestrator handoff/checkpoint 开一个新的供应商 session。

Claude 的 MCP 配置写在每次 run 独立的 `{run_artifacts}/driver/mcp.json`，Unix 权限为 `0600`，并发任务不会共享路径。

## 排错

先执行：

```bash
orchestrator apply --dry-run --project my-project -f agents.yaml
```

常见错误：

- `driver_multi_turn_required`：改用 Claude CLI，或取消多轮要求；
- `driver_tool_hosting_required`：选择支持所需工具传输的 driver；
- `driver_permission_events_required`：使用能产生权限事件的 driver；
- `driver_workspace_sandbox_required`：workspace 操作必须使用可沙箱化 CLI driver；
- `driver_guaranteed_cancel_required`：非幂等外部操作必须能保证取消；
- `driver_transport_unavailable`：当前版本改用 `transport: cli`。

## `rawArgs` 逃生口

不要把 `rawArgs` 当常规配置。它必须同时满足：`unsafeRawArgs: true`、daemon unsafe mode、Admin 权限和 canonical Action Audit。成功 apply 会记录 `agent.driver.raw_args.apply`。任何 token、credential、session ID 都不能放进 `rawArgs`。

## 建议迁移方式

1. 用临时 capability 新增 driver Agent；
2. 对 workflow 做 dry-run；
3. 运行确定性 pilot，对比完成状态、退出码、事件、沙箱和成本；
4. 再把生产 capability 切到 driver Agent；
5. 保留旧 command Agent 一个发布周期，作为显式回退。

可运行示例见 `fixtures/manifests/bundles/agent-driver-fixture.yaml`；完整验证执行 `scripts/qa/test-agent-driver-abstraction.sh`。
