---
self_referential_safe: true
---

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

### Goal

Verify that adaptive workflow configuration survives YAML serialization/deserialization roundtrip and that validation catches invalid references.

### Steps

1. **Unit test** — verify adaptive workflow validation (owned agents):
   ```bash
   cargo test -p agent-orchestrator --lib adaptive_none_returns_ok
   cargo test -p agent-orchestrator --lib adaptive_disabled_returns_ok
   cargo test -p agent-orchestrator --lib adaptive_missing_planner_agent_errors
   cargo test -p agent-orchestrator --lib adaptive_unknown_agent_errors
   cargo test -p agent-orchestrator --lib adaptive_valid_config_passes
   ```

2. **Unit test** — verify adaptive workflow validation (borrowed agent refs):
   ```bash
   cargo test -p agent-orchestrator --lib adaptive_refs_valid_config_passes
   cargo test -p agent-orchestrator --lib adaptive_refs_unknown_agent_errors
   cargo test -p agent-orchestrator --lib adaptive_refs_agent_missing_capability_errors
   ```

3. **Unit test** — verify adaptive planner generates valid plans:
   ```bash
   cargo test -p agent-orchestrator --lib test_adaptive_planner_generate_plan_enabled
   cargo test -p agent-orchestrator --lib test_validate_generated_plan_rejects_unknown_entry
   ```

4. **Unit test** — verify manifest export roundtrip preserves all fields:
   ```bash
   cargo test -p agent-orchestrator --lib export_validate_roundtrip_all_kinds
   cargo test -p agent-orchestrator --lib export_manifest_documents_produces_orchestrator_resources
   ```

5. **Code review** — verify adaptive field serialization:
   ```bash
   rg -n "adaptive|AdaptiveConfig" crates/orchestrator-config/src/config/workflow.rs
   ```

### Expected

- All 13 adaptive validation tests pass (7 owned + 6 refs variants)
- Adaptive planner tests pass — plan generation and validation work correctly
- Export roundtrip preserves adaptive fields in YAML representation
- `adaptive` field uses `skip_serializing_if` for clean output when disabled

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | YAML Configuration Integration for Dynamic Fields | ☐ | | | |
