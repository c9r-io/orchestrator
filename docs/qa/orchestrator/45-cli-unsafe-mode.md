---
self_referential_safe: false
---
# Orchestrator - CLI Unsafe Mode

**Module**: orchestrator
**Scope**: Validate global `--unsafe` CLI flag: force-gate bypass, runtime runner policy override, audit event, and warning banner
**Scenarios**: 5
**Priority**: High

---

## Background

The `--unsafe` global CLI flag is a compound safety override for power users and CI pipelines. When active, it:

1. Bypasses all `--force` confirmation gates (equivalent to passing `--force` on every command)
2. Overrides runner policy to `Unsafe` at runtime (in-memory only; persisted config is not modified)
3. Skips `validate_self_referential_safety()` checks
4. Emits an `unsafe_mode_activated` audit event
5. Prints a stderr warning banner

This flag does NOT disable redaction, timeouts, or stagnation detection. It is CLI-flag only — no environment variable equivalent exists.

Force-gate behavior without `--unsafe` is validated in `docs/qa/orchestrator/43-cli-force-gate-audit.md`.
Runner policy YAML-level configuration is validated in `docs/qa/orchestrator/21-runner-security-observability.md` and `docs/qa/orchestrator/31-runner-policy-defaults-compatibility.md`.

Entry point: `orchestrator`

---

## Scenario 1: Unsafe Mode Bypasses Force Gate

### Preconditions

- Orchestrator binary built from latest source.
- Runtime initialized.

### Goal

Verify `--unsafe` bypasses the `--force` gate on a project-scoped destructive command without requiring `--force`.

> **Safety note**: Do not use `--unsafe db reset` in QA scenarios — it destroys
> the runtime database and kills any running self-bootstrap task. Use a
> project-scoped command to verify force-gate bypass instead.

### Steps

1. Create an isolated QA project with a task to operate on:
   ```bash
   QA_PROJECT="qa-unsafe-force-$(date +%s)"
   orchestrator project reset "${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project "${QA_PROJECT}"
   ```

2. Run a force-gated command (`project reset`) with `--unsafe` but without `--force`:
   ```bash
   orchestrator --unsafe project reset "${QA_PROJECT}" 2>&1; echo "exit=$?"
   ```

### Expected

- Command executes successfully (exit code 0).
- No `Use --force to confirm` prompt appears.
- The project was reset (force gate bypassed by `--unsafe`).

---

## Scenario 2: Unsafe Mode Overrides Runner Policy

### Preconditions

- Runtime initialized.

### Goal

Verify `--unsafe` overrides the manifest's `policy: allowlist` to `Unsafe` at runtime, allowing a task to execute even when the configured shell is not in the allowlist.

### Steps

1. Prepare isolated project with restrictive allowlist policy:
   ```bash
   QA_PROJECT="qa-unsafe-override"
   orchestrator project reset "${QA_PROJECT}" --force 2>/dev/null || true
   rm -rf "workspace/${QA_PROJECT}"
   orchestrator apply -f fixtures/manifests/bundles/echo-workflow.yaml --project "${QA_PROJECT}"
   cat > /tmp/runner-allowlist-strict.yaml << 'YAML'
   runner:
     policy: allowlist
     executor: shell
     shell: /bin/bash
     shell_arg: -lc
     allowed_shells: [/usr/local/bin/nonexistent-shell]
     allowed_shell_args: [-lc]
   resume:
     auto: false
   defaults:
     workspace: default
     workflow: qa_only
   workspaces:
     default:
       root_path: .
       qa_targets: [docs/qa]
       ticket_dir: fixtures/ticket
   agents:
     mock:
       metadata:
         name: mock
       capabilities: [qa]
       templates:
         qa: "echo '{\"confidence\":0.9,\"quality_score\":0.86,\"artifacts\":[]}'"
   workflows:
     qa_only:
       steps:
         - id: qa
           required_capability: qa
           enabled: true
       loop:
         mode: once
         guard:
           enabled: false
           stop_when_no_unresolved: true
       finalize:
         rules: []
   YAML
   orchestrator apply --project "${QA_PROJECT}" -f /tmp/runner-allowlist-strict.yaml
   ```

2. Create and start task WITH `--unsafe`:
   ```bash
   TASK_ID=$(orchestrator --unsafe task create --project "${QA_PROJECT}" --name "unsafe-override" --goal "policy override" --no-start | grep -oE '[0-9a-f-]{36}' | head -1)
   orchestrator --unsafe task start "${TASK_ID}" || true
   ```

3. Inspect task result:
   ```bash
   orchestrator task info "${TASK_ID}" -o json
   ```

### Expected

- Task executes successfully (runner policy override to `Unsafe` bypasses allowlist check).
- Task status is `completed` (not `failed` with policy deny).

---

## Scenario 3: Unsafe Mode Emits Audit Event

### Preconditions

- Orchestrator binary built from latest source.

### Goal

Verify that any command invoked with `--unsafe` emits an `unsafe_mode_activated` event in the `events` table.

### Steps

1. Run a read-only command with `--unsafe`:
   ```bash
   orchestrator --unsafe get agents
   ```

2. Query the events table for the audit event:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT event_type, payload FROM events WHERE event_type='unsafe_mode_activated' ORDER BY created_at DESC LIMIT 1;"
   ```

### Expected

- An `unsafe_mode_activated` row exists in `events`.
- `payload` contains the command that was invoked (e.g., includes `GetAgents` or similar command identifier).

### Expected Data State

```sql
SELECT COUNT(*)
FROM events
WHERE event_type = 'unsafe_mode_activated';
-- Expected: >= 1
```

---

## Scenario 4: Unsafe Mode Prints Warning Banner

### Preconditions

- Orchestrator binary built from latest source.

### Goal

Verify that `--unsafe` prints a visible warning to stderr.

### Steps

1. Run any command with `--unsafe`, capturing stderr:
   ```bash
   orchestrator --unsafe get agents 2>/tmp/unsafe-stderr.txt
   cat /tmp/unsafe-stderr.txt
   ```

2. Check for warning message:
   ```bash
   grep -c "unsafe" /tmp/unsafe-stderr.txt
   ```

### Expected

- stderr contains: `WARNING: --unsafe mode active` (or equivalent warning text).
- The warning appears before command output.

---

## Scenario 5: Default Behavior Unchanged Without Flag

### Preconditions

- Orchestrator binary built from latest source.

### Goal

Regression guard: verify that force gates still block without `--unsafe` or `--force`.

### Steps

1. Run `db reset` without `--force` or `--unsafe`:
   ```bash
   orchestrator db reset 2>&1; echo "exit=$?"
   ```

2. Run `task retry` without `--force` or `--unsafe` (requires a retryable item):
   ```bash
   ITEM_ID=$(sqlite3 data/agent_orchestrator.db \
     "SELECT id FROM task_items WHERE status IN ('qa_failed','unresolved') LIMIT 1;")
   if [ -n "$ITEM_ID" ]; then
     orchestrator task retry "$ITEM_ID" 2>&1; echo "exit=$?"
   fi
   ```

3. Verify no `unsafe_mode_activated` event was emitted for these invocations:
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT COUNT(*) FROM events WHERE event_type='unsafe_mode_activated' AND created_at > datetime('now', '-10 seconds');"
   ```

### Expected

- `db reset` prints confirmation prompt and exits with code 1 (blocked).
- `task retry` prints confirmation prompt and exits with code 1 (blocked).
- No `unsafe_mode_activated` event emitted in the last 10 seconds.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Unsafe Mode Bypasses Force Gate | ☐ | | | |
| 2 | Unsafe Mode Overrides Runner Policy | ☐ | | | |
| 3 | Unsafe Mode Emits Audit Event | ☐ | | | |
| 4 | Unsafe Mode Prints Warning Banner | ☐ | | | |
| 5 | Default Behavior Unchanged Without Flag | ☐ | | | |
