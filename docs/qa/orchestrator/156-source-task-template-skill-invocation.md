---
self_referential_safe: true
---

# Orchestrator - Source Task Template And Skill Invocation

**Module**: Orchestrator  
**Scope**: Native resource lifecycle, deterministic safe preview, hot reload/restart, validation rollback, and reference-governed deletion  
**Scenarios**: 5  
**Priority**: High

---

## Background

FR-108 adds a project-scoped `SourceTaskTemplate` and read-only preview. It does not match reactions, contact Slack, or create a task. The deterministic QA script starts an isolated daemon with its own ports, HOME, data directory, and mock agent:

```bash
cargo build -p orchestratord -p orchestrator-cli
./scripts/qa/test-source-task-template.sh
```

The script applies only `fixtures/manifests/bundles/source-task-template-fixture.yaml`. No live AI agent or external provider is used.

CLI: `orchestrator source template preview`

## Database Schema Reference

| Table | Purpose |
|---|---|
| `resources` | Unified project-scoped resource persistence, including SourceTaskTemplate and test binding CRs |
| `orchestrator_config_versions` | Restart-safe active configuration snapshots |
| `tasks` | Must not change during preview |
| `source_events` | Must not change during preview |
| `source_bindings` | Must not change during preview |
| `control_action_audit` | Canonical Admin force-reference cleanup evidence |

---

## Scenario 1: Resource Lifecycle And Cross-Reference Round-Trip

### Preconditions

- Build the current debug binaries.
- Apply the deterministic mock fixture explicitly:

  ```bash
  orchestrator apply --project qa-source-template \
    -f fixtures/manifests/bundles/source-task-template-fixture.yaml
  ```

### Goal

Verify `SourceTaskTemplate` is a native project-scoped resource and retains its trusted Skill/action/template fields.

### Steps

1. Run get and describe for `sourcetasktemplate/slack-docs` in `qa-source-template`.
2. Run `orchestrator manifest export -o yaml`.
3. Confirm Skill invocation, arguments, workflow, workspace, start, initial variables, goal template, and allowlist are preserved.
4. Attempt an apply referencing a missing workflow and then a missing workspace.

### Expected

- Apply/get/describe/export work through the normal resource interfaces.
- Workflow and workspace references resolve only within the selected project.
- Missing references return actionable validation errors and do not replace active config.

### Expected Data State

```sql
SELECT kind, project, name FROM resources
WHERE kind='SourceTaskTemplate' AND project='qa-source-template';
-- Expected: one row named slack-docs
```

---

## Scenario 2: Safe Deterministic Preview And Zero Mutation

### Preconditions

- Use the fixture and isolated daemon from Scenario 1.

### Goal

Verify one-pass rendering, URL policy, hash/revision, warning, RuntimePolicy redaction, and read-only behavior.

### Steps

1. Record project counts in `tasks`, `source_events`, and `source_bindings`.
2. Preview `slack-docs` with provider `slack`, installation `qa-installation`, and an HTTPS `*.slack.com/archives/...` URL containing `{source_reaction}` as source text.
3. Inspect rendered goal, Skill/action, hash, revision, warnings, and redacted initial variables.
4. Re-read all three persistence counts.
5. Run renderer, URL, hash, and redaction unit tests through `cargo test -p agent-orchestrator source_task_template`.

### Expected

- `{{source}}` becomes literal `{source}` and the source-provided `{source_reaction}` remains inert; no nested evaluation occurs.
- Hash is 64 lowercase hexadecimal characters and equals revision.
- Preview contains `sample_url_not_verified_against_installation`.
- The configured sensitive marker is redacted from all public text fields.
- HTTP URLs, credentials, non-Slack hosts, non-permalink paths, oversized values, and undeclared tokens fail closed.

### Expected Data State

```sql
SELECT COUNT(*) FROM tasks WHERE project_id='qa-source-template';
SELECT COUNT(*) FROM source_events WHERE project_id='qa-source-template';
SELECT COUNT(*) FROM source_bindings WHERE project_id='qa-source-template';
-- Expected: every before/after pair is identical (zero in the fixture)
```

---

## Scenario 3: Invalid Update Rollback And Backward Compatibility

### Preconditions

- Keep the valid template active from Scenario 1.

### Goal

Verify invalid templates cannot partially replace active config and existing resource behavior remains compatible.

### Steps

1. Record the valid template's preview revision.
2. Apply a template using unsupported `source_body`, then apply one with a missing workflow reference.
3. Preview the original template again.
4. Run `cargo test --workspace`.

### Expected

- Both invalid applies fail with stable field/reference context.
- The original template remains previewable at the prior revision.
- Existing StepTemplate, Trigger, resource, and source-event suites pass without manifest changes.

---

## Scenario 4: Atomic Hot Reload And Restart-Stable Revision

### Preconditions

- Use the isolated fixture and data directory from Scenario 1.

### Goal

Verify each render observes exactly one immutable active-config version and canonical content survives restart.

### Steps

1. Preview and record revision V1.
2. Apply a valid update changing invocation and goal; preview revision V2 immediately.
3. Stop and restart the isolated daemon without removing its data directory.
4. Preview again and record revision V3.
5. Run the ArcSwap concurrency and YAML ordering unit tests.

### Expected

- V1 differs from V2.
- V2 equals V3 and the updated invocation is present after restart.
- Concurrent readers return only a complete old or complete new revision, never mixed fields.
- Equivalent allowed-variable/key ordering yields the same content hash.

---

## Scenario 5: Reference-Safe Delete, Admin Cleanup, And Audit Privacy

### Preconditions

- Use the valid template from Scenario 4.
- Apply the script's deterministic native `SourceTaskBinding` and mock Slack Trigger in `qa-source-template`; the binding references `slack-docs` through `spec.templateRef`.
- Start the isolated daemon with an Admin test policy. Do not use a shared daemon.

### Goal

Verify deletion fails closed by default and explicit reference cleanup is authorized, atomic, and privacy-safe.

### Steps

1. Run normal forced deletion without `--force-references`.
2. Confirm the template and binding still exist.
3. Run `orchestrator delete sourcetasktemplate/slack-docs --project qa-source-template --force --force-references`.
4. Query both resources and `orchestrator audit list --project qa-source-template --action delete_references -o json`.
5. Search audit output for the sensitive fixture marker and Slack permalink.

### Expected

- Normal deletion returns `FailedPrecondition` naming the referencing binding.
- Admin cleanup removes the template and references as one committed config change.
- Canonical audit records Admin role, resource target, reason, request hash, succeeded status, and result identifier.
- Audit contains no rendered goal, source URL, or configured sensitive value.

### Expected Data State

```sql
SELECT COUNT(*) FROM resources
WHERE project='qa-source-template'
  AND kind IN ('SourceTaskTemplate','SourceTaskBinding');
-- Expected after force-reference cleanup: 0

SELECT action, target_type, target_id, status
FROM control_action_audit
WHERE project_id='qa-source-template' AND action='delete_references';
-- Expected: one succeeded source_task_template audit row
```

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Resource lifecycle and cross-reference round-trip | PASS | 2026-07-17 | Codex | Isolated script |
| 2 | Safe deterministic preview and zero mutation | PASS | 2026-07-17 | Codex | Isolated script + unit/workspace suite |
| 3 | Invalid update rollback and backward compatibility | PASS | 2026-07-17 | Codex | Isolated script + workspace suite |
| 4 | Atomic hot reload and restart-stable revision | PASS | 2026-07-17 | Codex | Isolated script + unit tests |
| 5 | Reference-safe delete, Admin cleanup, and audit privacy | PASS | 2026-07-17 | Codex | Isolated script; no ticket created |
