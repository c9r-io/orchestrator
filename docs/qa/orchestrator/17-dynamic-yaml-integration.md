# Orchestrator - Dynamic YAML Integration

**Module**: orchestrator
**Scope**: Validate dynamic orchestration fields through YAML export/import path
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `13-dynamic-orchestration.md` to keep each QA document within 5 scenarios.

Entry point: `orchestrator manifest <command>`

---

## Scenario 1: YAML Configuration Integration for Dynamic Fields

### Preconditions

- Orchestrator initialized with clean state: `orchestrator init` (delete `data/agent_orchestrator.db` beforehand if cross-fixture isolation is needed)
- Config applied: `orchestrator apply -f fixtures/manifests/bundles/adaptive-runtime.yaml`

### Steps

1. Export runtime config:
   ```bash
   orchestrator manifest export -o yaml > /tmp/exported-config.yaml
   ```

2. Verify adaptive workflow snippet:
   ```bash
   grep -A 5 "adaptive:" /tmp/exported-config.yaml
   ```

3. Validate exported YAML:
   ```bash
   orchestrator manifest validate -f /tmp/exported-config.yaml
   ```

### Expected

- Steps 1-2: Export succeeds and the `adaptive` field is preserved in the exported YAML representation.
- Step 3: Validation of the exported YAML succeeds **for the workflows defined in the applied fixture**. If the runtime previously loaded workflows from other fixtures (e.g., `wp05-store-spawn-child`), those may fail self-referential policy checks (`SELF_REF_POLICY_VIOLATION`) that are unrelated to the dynamic fields under test. Such failures are **not** regressions in YAML integration — they reflect pre-existing policy mismatches in those workflows.
- YAML remains an artifact for edit/export/apply, not the runtime source of truth.

> **Note:** To isolate this scenario from cross-fixture contamination, delete
> `data/agent_orchestrator.db` and restart the daemon before `orchestrator init`
> so the exported config contains only the fixture's own resources.

### Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `grep "adaptive:"` returns no matches | Wrong fixture applied (e.g., `output-formats.yaml` has no adaptive fields) | Use `adaptive-runtime.yaml` |
| Validation fails with `SELF_REF_POLICY_VIOLATION` | Cross-fixture contamination from prior applies | Clean-init: delete DB, restart daemon, re-apply only this fixture |

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | YAML Configuration Integration for Dynamic Fields | ✅ | 2026-03-16 | claude | PASS — fixture corrected to adaptive-runtime.yaml; adaptive fields preserved in export; validation passes with clean init |
