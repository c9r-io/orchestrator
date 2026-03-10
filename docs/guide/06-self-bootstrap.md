# 06 - Self-Bootstrap

The self-bootstrap workflow is a special case where the orchestrator modifies **its own source code** through AI agents. This requires additional safety mechanisms to prevent the system from permanently destroying itself.

## 2-Cycle Strategy

Self-bootstrap uses `loop.mode: fixed` with `max_cycles: 2`:

```
Cycle 1 — Production:   plan → qa_doc_gen → implement → self_test → self_restart
Cycle 2 — Validation:   implement → self_test → qa_testing → ticket_fix → align_tests → doc_governance
```

- **Cycle 1** focuses on feature development. QA steps are deferred via prehooks (`when: "is_last_cycle"`).
- **Cycle 2** is the validation pass. After self_restart rebuilds the binary, QA testing runs against the new code, tickets are fixed, and documentation is audited.

The `repeatable: false` flag on `plan` and `qa_doc_gen` ensures they only run in Cycle 1. Steps with `repeatable: true` (like `implement`, `self_test`) run in both cycles, allowing iterative improvement.

## Self-Referential Workspace

A self-referential workspace declares that it points to the orchestrator's own source tree:

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: self
spec:
  root_path: "."
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
  self_referential: true       # enables survival mechanisms
```

When `self_referential: true`, the engine enforces:
- `safety.auto_rollback` must be `true`
- `safety.checkpoint_strategy` must not be `none`
- the workflow must include an enabled builtin `self_test` step
- `safety.binary_snapshot` should be `true`

If the required settings are missing, the orchestrator refuses to start. Missing `binary_snapshot` emits a warning but does not block startup.

## 4-Layer Survival Mechanism

### Layer 1: Binary Snapshot

At the beginning of each cycle, the current release binary is copied to a `.stable` backup file. If a later step produces a corrupt binary, the system can restore from this snapshot.

```yaml
safety:
  binary_snapshot: true
```

### Layer 2: Self-Test Gate

After `implement` modifies source code, the `self_test` builtin step runs:

1. `cargo check` — compilation must pass
2. `cargo test --lib` — unit tests must pass
3. `manifest validate` — YAML manifests must be valid

If any phase fails, execution halts before the corrupt binary can be deployed.

### Layer 3: Self-Referential Enforcement

QA files are tagged with `self_referential_safe` metadata. The prehook variable `self_referential_safe` is `false` for QA docs that test the orchestrator's own configuration or execution engine — preventing the orchestrator from inadvertently modifying its own safety checks while testing.

```yaml
prehook:
  engine: cel
  when: "is_last_cycle && self_referential_safe"
  reason: "Skip unsafe self-referential QA docs"
```

### Layer 4: Watchdog

The daemon (`orchestratord --foreground`) handles self-restart via `exec()` self-replacement. When the `self_restart` builtin step rebuilds the binary, it calls `exec()` to replace the running process in-place (preserving PID). If `exec()` fails, the process exits with code 75, which external supervisors (systemd, Docker restart policy) can use to relaunch.

Exit code 75 is the self-restart signal: the `self_restart` builtin step rebuilds the binary, verifies it, snapshots `.stable`, and triggers the restart.

## Self-Restart Flow

```
Cycle 1:
  implement → modifies source code
  self_test → cargo check + test
  self_restart → cargo build --release
                → snapshot new binary hash
                → exit(75)

Watchdog detects exit 75:
  → Relaunches orchestrator with new binary
  → Orchestrator resumes at Cycle 2

Cycle 2:
  implement → reviews diff, makes incremental improvements
  self_test → validates again
  qa_testing → runs QA scenarios against the new code
  ticket_fix → fixes any QA failures
```

## StepTemplate Configuration

Self-bootstrap uses StepTemplates to decouple prompt content from agents:

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: plan
spec:
  prompt: >-
    You are working on the project at {source_tree}.
    Create a plan for: {goal}.
    Current diff: {diff}
```

The agent's command uses `{prompt}` as a placeholder:

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: architect
spec:
  capabilities: [plan, qa_doc_gen]
  command: "claude --print -p '{prompt}'"
```

At runtime: StepTemplate resolves pipeline variables → result injected into Agent command's `{prompt}`.

## Agent Roles

The self-bootstrap workflow typically uses specialized agents:

| Agent | Capabilities | Role |
|-------|-------------|------|
| architect | `plan`, `qa_doc_gen` | Planning and QA document design |
| coder | `implement`, `ticket_fix`, `align_tests` | Code generation and fixing |
| tester | `qa_testing` | QA scenario execution |
| reviewer | `doc_governance`, `review` | Documentation audit and code review |

## Complete Example

See `fixtures/manifests/bundles/self-bootstrap-mock.yaml` for the full production manifest including all StepTemplates, Agents, and the Workflow definition.

## Next Steps

- [07 - CLI Reference](07-cli-reference.md) — command quick-reference
- [03 - Workflow Configuration](03-workflow-configuration.md) — step and loop configuration
