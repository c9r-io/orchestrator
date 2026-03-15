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

- Orchestrator initialized: `orchestrator init --force`
- Config applied: `orchestrator apply -f fixtures/manifests/bundles/output-formats.yaml` (or any valid config fixture)

### Steps

1. Export runtime config:
   ```bash
   orchestrator manifest export -f /tmp/exported-config.yaml
   ```

2. Verify adaptive workflow snippet:
   ```bash
   grep -A 5 "adaptive:" /tmp/exported-config.yaml || true
   ```

3. Verify dynamic steps snippet:
   ```bash
   grep -A 15 "dynamic_steps:" /tmp/exported-config.yaml | head -20 || true
   ```

4. Validate exported YAML:
   ```bash
   orchestrator manifest validate -f /tmp/exported-config.yaml
   ```

### Expected

- Steps 1-3: Export succeeds and dynamic orchestration fields (adaptive, dynamic_steps) are preserved in the exported YAML representation.
- Step 4: Validation of the exported YAML succeeds **for the workflows defined in the applied fixture**. If the runtime previously loaded workflows from other fixtures (e.g., `wp05-store-spawn-child`), those may fail self-referential policy checks (`SELF_REF_POLICY_VIOLATION`) that are unrelated to the dynamic fields under test. Such failures are **not** regressions in YAML integration — they reflect pre-existing policy mismatches in those workflows.
- YAML remains an artifact for edit/export/apply, not the runtime source of truth.

> **Note:** To isolate this scenario from cross-fixture contamination, run
> `orchestrator init --force` immediately before applying the fixture so the
> exported config contains only the fixture's own resources.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | YAML Configuration Integration for Dynamic Fields | ✅ | 2026-03-15 | claude | PASS - export preserves adaptive/dynamic_steps; validation pass requires clean init (cross-fixture wp05 self-ref failures are not regressions) |
