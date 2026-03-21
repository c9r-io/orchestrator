---
self_referential_safe: true
---

# Orchestrator - CLI Output Formats

**Module**: orchestrator
**Scope**: Validate JSON/YAML output formats for all list and info commands
**Scenarios**: 5
**Priority**: Medium

---

## Background

This document tests that all CLI commands support proper JSON and YAML output formats for scripting and integration.

> **Note on log lines**: Structured log lines (e.g., `INFO agent_orchestrator: structured logging initialized`) are written to **stderr**, not stdout. When piping CLI output to `jq` or `yq`, only stdout is passed through the pipe, so log lines do **not** interfere with JSON/YAML parsing. If you see log lines interleaved in terminal output, that is normal stderr display — it does not affect `| jq` correctness.

---

## Scenario 1: Task List JSON/YAML Output

### Preconditions

- Tasks exist in database

### Steps

1. Get JSON output:
   ```bash
   orchestrator task list -o json
   ```

2. Get YAML output:
   ```bash
   orchestrator task list -o yaml
   ```

3. Verify JSON is valid:
   ```bash
   orchestrator task list -o json | jq '.'
   ```

### Expected

- JSON output is valid and parseable
- YAML output is valid
- Both contain all task fields

---

## Scenario 2: Task Info JSON/YAML Output

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm `task info` output formatting in `crates/cli/src/output/`:
   - Task detail includes task fields, items, status, and event details
   - `-o json` produces valid JSON with all fields
   - `-o yaml` produces valid YAML

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- load_task_detail_rows_returns_items_runs_and_events
   cargo test --workspace --lib -- load_task_detail_rows_includes_events
   cargo test --workspace --lib -- load_task_detail_rows_includes_command_runs
   ```
   > **Note**: The original test name `task_detail_value_includes_item_run_and_event_details` no longer exists in the codebase. The behavior is verified by the three tests above (all pass).

### Expected

- Task detail JSON structure includes items, runs, and events
- Unit test verifies JSON structure completeness
- Output format is valid JSON/YAML

---

## Scenario 3: Workspace List JSON/YAML

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm workspace list serialization:
   - Workspace resources implement `to_yaml()` via the `Resource` trait
   - JSON serialization uses `serde_json::to_value()`
   - All workspace fields are included in output

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- resource_to_yaml
   cargo test --workspace --lib -- resource_trait_to_yaml_serializes_manifest_shape
   cargo test --workspace --lib -- registered_resource_to_yaml_delegates
   ```

### Expected

- Workspace list outputs all workspaces in valid JSON/YAML format
- Resource `to_yaml()` produces correct manifest-shaped output
- All resource types delegate YAML serialization correctly

---

## Scenario 4: Manifest Export JSON/YAML

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm manifest export in `crates/cli/src/commands/`:
   - `manifest export -o json` returns a JSON array of CRD resources
   - Each resource has `apiVersion`, `kind`, `metadata`, `spec` fields
   - `manifest export -o yaml` returns multi-document YAML

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- resource_trait_to_yaml_serializes_manifest_shape
   cargo test --workspace --lib -- execution_profile_to_yaml_contains_kind
   cargo test --workspace --lib -- env_store_kind_name_validate_yaml
   cargo test --workspace --lib -- secret_store_kind_name_validate_yaml
   cargo test --workspace --lib -- step_template_kind_name_validate_yaml
   ```

### Expected

- Manifest export produces CRD-style resources with correct structure
- All resource types serialize to valid YAML with `kind` field
- JSON array format is parseable with `jq`

---

## Scenario 5: Workflow/Agent List JSON/YAML

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm workflow and agent list serialization:
   - Workflow resources serialize all steps, loop config, and finalize rules
   - Agent resources serialize capabilities and command config
   - Both support `-o json` and `-o yaml` output flags

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- registered_resource_to_yaml_delegates
   cargo test --workspace --lib -- registered_resource_kind_name_for_all_variants
   ```

### Expected

- Workflow/agent details are included in output
- Both JSON and YAML formats are valid
- All resource variants serialize correctly

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Task List JSON/YAML | PASS | 2026-03-20 | Claude | JSON valid & parseable by jq; YAML valid; all fields present |
| 2 | Task Info JSON/YAML | PASS | 2026-03-20 | Claude | 7/7 tests pass: load_task_detail* (items, runs, events, graph debug bundles) |
| 3 | Workspace List JSON/YAML | PASS | 2026-03-20 | Claude | 3/3 tests pass: resource_to_yaml, project_resource_to_yaml, registered_resource_to_yaml_delegates |
| 4 | Manifest Export JSON/YAML | PASS | 2026-03-20 | Claude | 4/4 tests pass: execution_profile_to_yaml, env_store, secret_store, step_template |
| 5 | Workflow/Agent List JSON/YAML | PASS | 2026-03-21 | Claude | 10/10 tests pass: registered_resource_* (kind, validate, to_yaml, delete, get) |
