---
lifecycle: active
related_fr: FR-109
self_referential_safe: true
---

# Orchestrator - Source Task Binding And Badge Matching

**Module**: Orchestrator
**Scope**: Native lifecycle, exact deterministic matching, conflict rollback, hot lifecycle mutation, audit privacy, and reference deletion
**Scenarios**: 5
**Priority**: High

---

## Background

FR-109 maps authenticated normalized reaction evidence to one SourceTaskTemplate. It does not contact Slack, resolve a permalink, render a goal, or create a task. The deterministic script starts an isolated daemon with separate ports, HOME, and data directory and applies only a mock echo-agent fixture:

```bash
cargo build -p orchestratord -p orchestrator-cli
./scripts/qa/test-source-task-binding.sh
```

Required fixture: `fixtures/manifests/bundles/source-task-binding-fixture.yaml`.

CLI: `orchestrator source binding simulate|suspend|resume`

## Database Schema Reference

| Table | Purpose |
|---|---|
| `resources` | Project-scoped Trigger, SourceTaskTemplate, and SourceTaskBinding persistence |
| `orchestrator_config_versions` | Atomic restart-safe config revisions |
| `control_action_audit` | Canonical binding apply/delete/suspend/resume evidence |
| `tasks` | Must not be created by FR-109 matching |
| `source_events` | Existing live route evidence; simulation does not write it |

---

## Scenario 1: Native Resource Lifecycle And Restart Round-Trip

### Preconditions

- Build current binaries.
- Apply only the deterministic mock fixture:

  ```bash
  orchestrator apply --project qa-source-binding \
    -f fixtures/manifests/bundles/source-task-binding-fixture.yaml
  ```

### Goal

Verify the binding is a native project-scoped resource with complete lifecycle projection and stable revision.

### Steps

1. Get and describe `sourcetaskbinding/slack-code-analysis` in `qa-source-binding`.
2. Export all manifests and locate the binding.
3. Simulate a valid match and record `binding_revision`.
4. Restart the isolated daemon without removing its data directory.
5. Repeat get and simulation.

### Expected

- Trigger, match rule, channel policy, template, role policy, and suspend state round-trip without loss.
- Revision is 64 lowercase hexadecimal characters and remains identical after restart.
- No live agent, Slack API, task, or source row is created.

### Expected Data State

```sql
SELECT kind, project, name FROM resources
WHERE kind='SourceTaskBinding' AND project='qa-source-binding';
-- Expected: one row named slack-code-analysis
```

---

## Scenario 2: Exact Match And Stable No-Match Matrix

### Preconditions

- Keep the valid fixture from Scenario 1 active with `reactionRouting: bindings`.

### Goal

Verify exact evidence selects one template and untrusted/mismatched evidence fails safely.

### Steps

1. Simulate provider `slack`, installation `T_QA_BINDING`, reaction `agent-analyze`, target `message`, channel `C_QA_ALLOWED`, and actor `U_OPERATOR`.
2. Repeat separately with wrong reaction, target, channel, installation, unknown actor, and `U_READER`.
3. Inspect overall and per-candidate reason codes.
4. Run:

   ```bash
   cargo test -p agent-orchestrator source_task_binding
   cargo test -p orchestratord source_router::tests --bin orchestratord
   ```

### Expected

- Valid evidence returns `matched/binding_matched`, binding `slack-code-analysis`, template `analyze-from-slack`, and resolved role `operator`.
- Wrong evidence returns `no_match` with stable field-specific reasons.
- Role is resolved from Trigger `actorRoles`; no simulation field can supply a role.
- Pure simulation selects the same binding without provider or task effects. Live enabled routing reuses that matcher before the FR-110 permalink/task path; its vertical behavior is verified in QA 158.

### Expected Data State

```sql
SELECT COUNT(*) FROM tasks WHERE project_id='qa-source-binding';
-- Expected: 0
```

---

## Scenario 3: Secure Defaults, References, And Overlap Rollback

### Preconditions

- Keep the valid binding active.
- Use the mock fixture command from Scenario 1 for every reset.

### Goal

Verify unsafe policies and ambiguous rules cannot replace active configuration.

### Steps

1. Apply bindings omitting both `channels` and `allChannels`, setting both, omitting roles, or using wildcard/colon-wrapped reactions.
2. Apply bindings referencing a missing/non-Slack Trigger, missing template, or unreachable role.
3. Attempt cross-project references by placing only the target in another project.
4. Apply an enabled rule with `allChannels: true` overlapping the valid rule.
5. Simulate the original valid event again.

### Expected

- Every unsafe/reference-invalid apply fails with field/resource context.
- Overlap fails with both binding names; no specificity winner is selected.
- The prior active binding and revision remain available after every rejection.
- Unit tests prove runtime ambiguity also fails closed if invalid legacy state bypasses apply validation.

---

## Scenario 4: Suspend/Resume Hot Reload And Audit Privacy

### Preconditions

- Apply `fixtures/manifests/bundles/source-task-binding-fixture.yaml` in the isolated Admin-capable daemon.

### Goal

Verify lifecycle changes are immediate, complete-project validated, restart-safe, and canonically audited.

### Steps

1. Suspend `slack-code-analysis`; immediately simulate the valid event.
2. Resume it; immediately simulate again.
3. Restart the daemon and simulate once more.
4. Query `orchestrator audit list --project qa-source-binding -o json`.
5. Search audit output for message URLs, bodies, Slack tokens, or rendered goals.

### Expected

- Suspended simulation returns `binding_suspended`; resumed simulation returns `binding_matched`.
- Resume is rejected if it would reintroduce an enabled overlap.
- Resumed revision survives restart.
- Audit has succeeded `source.binding.apply`, `.suspend`, and `.resume` entries with actor/role, target, request hash, and result ID.
- Audit contains no message content, URL, token, or goal.

### Expected Data State

```sql
SELECT action, target_type, target_id, status
FROM control_action_audit
WHERE project_id='qa-source-binding'
  AND action LIKE 'source.binding.%';
-- Expected: succeeded apply, suspend, and resume rows
```

---

## Scenario 5: Reference-Safe Trigger/Template/Binding Deletion

### Preconditions

- Apply the deterministic fixture from Scenario 1.

### Goal

Verify deletion cannot orphan bindings and all mutations use explicit audit/reference policy.

### Steps

1. Force-delete `sourcetasktemplate/analyze-from-slack` without reference cleanup.
2. Repeat with `--force --force-references` as Admin.
3. Reapply the fixture and force-delete `trigger/slack-main` without reference cleanup, then with cleanup.
4. Reapply and directly delete `sourcetaskbinding/slack-code-analysis --force`.
5. Query resources and action audit after each operation.

### Expected

- Normal Trigger/template deletion names the referencing binding and leaves all resources intact.
- Admin cleanup removes target plus references in one persisted config change.
- Direct binding deletion removes only that binding and records `source.binding.delete`.
- No deletion audit stores source message content or URL.

### Expected Data State

```sql
SELECT COUNT(*) FROM resources
WHERE project='qa-source-binding' AND kind='SourceTaskBinding';
-- Expected after each successful cleanup/direct delete: 0
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Native resource lifecycle and restart round-trip | PASS | 2026-07-17 | Codex | Isolated script + restart round-trip |
| 2 | Exact match and stable no-match matrix | PASS | 2026-07-17 | Codex | Isolated script + matcher/router unit tests |
| 3 | Secure defaults, references, and overlap rollback | PASS | 2026-07-17 | Codex | Isolated script + atomic snapshot regression |
| 4 | Suspend/resume hot reload and audit privacy | PASS | 2026-07-17 | Codex | Isolated script + canonical audit query |
| 5 | Reference-safe Trigger/template/binding deletion | PASS | 2026-07-17 | Codex | Isolated script; blocked, cleanup, and direct delete paths |
