---
self_referential_safe: true
---

# Orchestrator - Capability Manifest Export Fields

**Module**: orchestrator
**Scope**: Validate manifest export exposes capability-orchestration fields
**Scenarios**: 1
**Priority**: Medium

---

## Background

This document is split from `07-capability-orchestration.md` to keep each QA document within 5 scenarios.

Entry point: `orchestrator manifest export`

---

## Scenario 1: Manifest Export Shows Capability Fields

### Goal

Verify that manifest export includes capability-orchestration fields: `required_capability`, `cost`, `capabilities`, `selection.strategy`, `repeatable`, `is_guard`, and `builtin`.

### Steps

1. **Code review** — verify `required_capability` is set during normalization for all SDLC step types:
   ```bash
   rg -n "normalize_sets_required_capability" core/src/config_load/normalize/tests.rs
   ```
   Expected: 17+ test functions covering qa, fix, plan, implement, review, build, test, lint, gitops, qa_doc_gen, qa_testing, ticket_fix, doc_governance, align_tests, retest, smoke_chain, and from_id.

2. **Unit test** — verify normalization sets `required_capability` correctly:
   ```bash
   cargo test -p agent-orchestrator --lib normalize_sets_required_capability
   ```

3. **Unit test** — verify export includes all resource types with correct field serialization:
   ```bash
   cargo test -p agent-orchestrator --lib export_manifest_resources_includes_all_resource_types
   cargo test -p agent-orchestrator --lib export_validate_roundtrip_all_kinds
   cargo test -p agent-orchestrator --lib export_manifest_documents_maps_all_kind_variants
   ```

4. **Unit test** — verify agent selection and capability support:
   ```bash
   cargo test -p orchestrator-config --lib test_agent_supports_capability
   cargo test -p orchestrator-config --lib test_agent_selection_config_default
   ```

5. **Code review** — verify `cost` is optional (`Option<u8>`):
   ```bash
   rg -n "pub cost:" crates/orchestrator-config/src/config/agent.rs
   ```

### Expected

- All 17+ normalization tests pass — each SDLC step type gets correct `required_capability`
- Export roundtrip tests pass — all resource kinds survive export/re-import
- Agent config correctly serializes optional `cost`, `capabilities`, and `selection.strategy`
- Step config correctly serializes `repeatable`, `is_guard`, `required_capability`, and `builtin`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Manifest Export Shows Capability Fields | ✅ | 2026-04-01 | Claude | All 17 normalization tests + 3 export tests + 2 agent config tests passed |
