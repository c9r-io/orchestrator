# Orchestrator - Agent Driver Abstraction

**Module**: Orchestrator Config / Runner / Scheduler  
**Status**: Released  
**Related Plan**: FR-116  
**Related QA**: `docs/qa/orchestrator/164-agent-driver-abstraction.md`, `docs/qa/orchestrator/166-codex-session-resume-conformance.md`
**Created**: 2026-07-22  
**Last Updated**: 2026-07-25

> **Post-release execution update (FR-126):** every production Agent now has an explicit typed driver. Historical command-only input remains accepted only at runtime compatibility ingress, where Apply emits `[legacy_agent_command_deprecated]` and persists `shell/cli`. The global streaming executor and provider-session compatibility bridge have been deleted. See [DD-138](138-agent-driver-execution-migration.md).

## Background

The first streaming runner proved that an agent CLI can exchange structured tool events with Orchestrator, but the global runner assembled Claude flags in control-plane code and still treated stdout as the terminal truth. That shape could not safely support multiple providers, apply-time capability checks, live permission requests, or provider-neutral session attachment.

FR-116 makes the provider protocol an Agent-owned `driver`. `RunnerConfig` and `ExecutionProfile` retain process policy, sandbox, environment filtering, resource limits, and process-group cancellation. A driver translates typed options and provider wire events; it does not own authorization, Attention, task lifecycle, or workspace policy.

## Goals

- Provide object-safe `AgentDriver` and `DriverSession` contracts with one event stream and one input channel.
- Implement `shell/cli`, `claude/cli`, and `codex/cli`; reserve SDK descriptors for fail-closed validation only.
- Reject driver/workflow capability mismatches during apply with stable structured diagnostics.
- Preserve the sandbox path for every CLI driver and require guaranteed cancellation for non-idempotent external work.
- Keep provider session material out of gRPC, DTOs, logs, audits, and normal event payloads.
- Consume normalized events directly so terminal results cannot disappear behind the 256 KiB log-read limit.
- Write Claude MCP configuration under a unique run-artifact directory with mode `0600`.

## Non-goals

- Removing `Agent.spec.command`; an explicit `shell/cli` driver still uses it, and command-only historical input is normalized at compatibility ingress.
- Migrating every historical manifest as part of FR-116 itself. FR-126 subsequently completed the production migration.
- Implementing in-process SDK execution or permitting SDK workspace mutation.
- Moving approval, RBAC, Action Audit, or Attention policy into a provider adapter.
- Persisting a provider token in the public command-run/session model. Cross-step attachment is daemon-memory scoped; daemon crash recovery restarts at an Orchestrator logical boundary.

## Resource Model

An Agent must define either `command` or `driver`; an explicit shell driver uses both. Provider and transport are orthogonal:

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: claude-coder
spec:
  capabilities: [implement]
  driver:
    provider: claude       # shell | claude | codex
    transport: cli         # cli | sdk (descriptor only)
    options:
      model: sonnet
      maxTurns: 8
      budgetCapUsd: 1.0
      permissionMode: ask  # governed | ask | deny
      allowedTools: [mcp__orch]
      cwd: .
      timeoutSecs: 1800
    claude:
      thinkingBudgetTokens: 2048
```

Workflow steps declare the semantics they need, not a provider:

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

`rawArgs` is the only untyped escape hatch. It requires `unsafeRawArgs: true`, daemon unsafe mode, Admin authorization, and a canonical `agent.driver.raw_args.apply` audit action. It is intentionally noisy in exported configuration.

## Runtime Contracts

`orchestrator-runner::driver` owns:

- `AgentDriver`: stable identity, static capabilities, and `start`;
- `DriverSession`: single-consumer `events`, `send`, process-group `cancel`, opaque `session_ref`, and PID bookkeeping;
- `DriverEvent`: started, assistant text, tool use/result, permission request, usage, and terminal outcome;
- `DriverInput`: user message, tool result, permission decision, and interrupt;
- `SessionRef`: no `Serialize` or `Display`; redacted `Debug` only.

The scheduler uses the following flow for drivers:

```text
setup → start → consume → fold → record
```

Command-only manifests are promoted to `shell/cli` before scheduling; an unnormalized missing driver fails closed with `[legacy_agent_execution_removed]`. All CLI drivers share the safe spawn substrate, so policy validation, daemon-PID protection, sandbox construction, process groups, rlimits, environment allowlists, and secret redaction remain common.

## Provider Implementations

| Driver | Multi-turn | Tools | Session attach | Permission events | Cancel | Sandbox |
|---|---:|---|---:|---:|---|---:|
| `shell/cli` | No | None | No | No | Guaranteed | Yes |
| `claude/cli` | Yes | stdio MCP | Yes | Yes | Guaranteed | Yes |
| `codex/cli` | No | None | Yes | No | Guaranteed | Yes |
| `claude/sdk`, `codex/sdk` | Descriptor | HTTP | Yes | Yes | Cooperative | No |

Provider flags and JSON schemas exist only in `crates/orchestrator-runner/src/driver/`. There is no global streaming executor or provider-owned compatibility bridge; provider execution is selected only by `Agent.spec.driver`.

Claude input is JSONL over stdin. Claude and Codex JSONL output are normalized before the scheduler observes them. Unknown provider records remain in the redacted raw artifact but do not become invented semantic events.

### Codex Resume Certification

`codex/cli` session attachment is certified against `codex-cli 0.144.5`. The provider adapter obtains the opaque reference from `thread.started.thread_id` and resumes with `codex exec resume <SESSION_ID> --json ... -- <PROMPT>`. A controlled two-turn run proved that the resumed process emits the same thread ID and can recall a unique first-turn anchor.

The sanitized capture is `fixtures/driver/codex-cli-0.144.5-resume.json`. Default CI replays it offline; `scripts/qa/certify-codex-session-resume.sh` is the explicit auth/network/token-consuming recertification path. A different installed version fails that live gate until the fixture and mappings are reviewed. See [Codex Session Resume Conformance](129-codex-session-resume-conformance.md).

## Capability Validation

Every enabled explicit driver eligible for a workflow capability must satisfy the step. Validation fails closed with a stable code:

| Requirement | Diagnostic |
|---|---|
| multi-turn input | `driver_multi_turn_required` |
| hosted tool transport | `driver_tool_hosting_required` |
| cross-step attachment | `driver_session_resume_required` |
| permission request events | `driver_permission_events_required` |
| workspace sandbox | `driver_workspace_sandbox_required` |
| guaranteed external cancel | `driver_guaranteed_cancel_required` |
| executable transport | `driver_transport_unavailable` |
| invalid driver shape | `driver_config_invalid` |

`ApplyResponse.diagnostics` carries `code`, `message`, and `field_path`; the original string errors remain for additive gRPC compatibility.

## Session And Privacy Boundary

The provider token is parsed from provider output before persistence. The raw JSON artifact replaces `session_id`, `thread_id`, and equivalent nested fields with `[REDACTED]`. The normalized `Started` event records only `session_available: true|false`.

`SessionRef` can be passed only inside runner/scheduler memory. A task-scoped daemon-memory map permits later steps to attach during the same daemon lifetime. It is never copied into the public `command_runs.session_id`, task DTO, gRPC schema, Action Audit, trace payload, or Attention detail. On daemon loss, the safe recovery behavior is a new provider session from an Orchestrator boundary.

Tool arguments/results and assistant text are re-redacted against runner and SecretStore patterns before event insertion. Assistant event text is bounded to 16 KiB.

## Event And Attention Projection

Each `DriverEvent` produces one canonical database event. Tool use and tool result are separate first-class records. Usage and terminal outcome remain structured. `PermissionRequested` maps to `approval_requested`, allowing the existing Attention projector, RBAC, optimistic reservation, and audited decision flow to remain authoritative. A driver cannot approve its own request.

## MCP Isolation

Claude stdio MCP configuration is created at:

```text
{run_artifacts}/driver/mcp.json
```

The directory is unique to the command run and the file is mode `0600` on Unix. Concurrent runs therefore cannot overwrite another run's binary path or future per-run token. The removed legacy bridge's former shared `$TMPDIR/orch-streaming-mcp.json` path is not used.

## Pilot And Compatibility

`fixtures/manifests/bundles/agent-driver-fixture.yaml` retains a command-only compatibility case alongside explicit shell, Claude, and Codex Agents. The command-only case is accepted with a warning and promoted to `shell/cli`; the two shell pilots execute the same command and converge to `completed / exit 0`.

The original command-only Agent block was 9 effective YAML lines; the explicit shell block was 14, a five-line cost for typed provider/transport ownership. Runtime compatibility preserves terminal behavior, but new and production manifests must use the explicit form.

## Risks And Mitigations

- Provider JSON schemas can drift: conformance fixtures pin the fields Orchestrator consumes; Codex resume is pinned to `0.144.5`, the live certification rejects an unreviewed version, and unknown records are tolerated.
- Event payloads can leak secrets: session fields are removed before artifact persistence and all projections pass through configured redaction.
- SDK calls can bypass sandbox/process isolation: SDK is non-sandboxable and unavailable; apply rejects workspace and non-idempotent use before runtime.
- A driver process can stall: the consumer retains heartbeat, timeout, external pause, stall-kill, and process-group cancellation behavior.
- Capability claims can become optimistic: every eligible explicit driver is checked at apply, rather than checking only the currently preferred Agent.

## Operations And Rollback

- Roll out explicit drivers per Agent; keep the previous reviewed explicit driver Agent with a distinct capability for immediate fallback.
- Use `orchestrator apply --dry-run` before changing a production capability pool.
- Do not use `rawArgs` as ordinary configuration. If unavoidable, enable unsafe mode only for the reviewed apply and retain its Action Audit record.
- Rollback is additive: reassign the workflow capability to a reviewed explicit `shell/cli` Agent. Do not reintroduce command-only production configuration. No database migration or destructive conversion is required.
- A previous binary ignores no persisted driver run state because provider session material is not persisted in the public model. It cannot parse a new Agent driver manifest, so restore the prior manifest before binary rollback.

## Test Plan And Evidence

- Unit/conformance: contract folding, Claude mappings, recorded Codex `0.144.5` initial/resume streams, exact resume command construction, session redaction, unique MCP files, configuration shape, and diagnostics.
- Scheduler: direct event folding, complete event projection, Attention event mapping, redaction, timeout/cancel, and legacy regression.
- Vertical: isolated daemon applies the fixture and runs legacy/explicit shell pilots to the same terminal result.
- Security: session token absence across artifacts/events and unsafe raw-argument authorization/audit gates.

Automated entry point:

```bash
./scripts/qa/test-agent-driver-abstraction.sh
```

Codex-specific offline and controlled live entry points:

```bash
./scripts/qa/test-codex-session-resume.sh
./scripts/qa/certify-codex-session-resume.sh
```

## Acceptance Criteria

- Three CLI drivers implement the provider-neutral contracts and use the shared sandbox spawn path.
- Explicit driver phases consume normalized events directly; terminal truth does not depend on truncated stdout rereads.
- Capability mismatch and SDK workspace use fail during apply with structured diagnostics.
- Provider tokens are absent from public/control-plane persistence and observations.
- All normalized events enter the canonical event table and permission requests route to Attention.
- MCP configuration is run-scoped and private.
- The legacy/explicit shell pilot is terminally equivalent and existing shell tests remain green.
