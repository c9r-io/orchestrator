---
self_referential_safe: true
---

# Orchestrator - CLI Config and Database

**Module**: orchestrator
**Scope**: Validate configuration update and database reset flows
**Scenarios**: 4
**Priority**: High

---

## Background

This document validates config lifecycle commands and database reset behavior through code review and unit tests.

The config lifecycle logic (apply, validate, delete) is fully covered by unit tests in `core/src/resource/tests.rs` (69+ tests) and `core/src/config_load/validate/tests.rs` (90+ tests).

> **Note**: `apply` and `manifest validate` accept multi-document YAML with
> `apiVersion`/`kind`/`metadata`/`spec` resources. The flat config format
> (runner/defaults/workspaces/…) is the internal serialization and is **not**
> accepted by these commands. If any resource in a manifest has a validation
> error, the entire apply is aborted and no changes are persisted.

---

## Scenario 1: Manifest Apply - Update Configuration

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm apply logic in `core/src/service/resource.rs`:
   - `apply_manifests()` parses multi-document YAML
   - Each resource is dispatched to `apply_to_project()` via `resource_dispatch()`
   - Apply result reports `created`, `configured` (updated), or `unchanged`
   - Config version is incremented on successful apply

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- apply_result_created_when_missing
   cargo test --workspace --lib -- apply_result_unchanged_for_identical_resource
   cargo test --workspace --lib -- apply_result_configured_when_resource_changes
   cargo test --workspace --lib -- apply_to_store_returns_created_for_new_resource
   cargo test --workspace --lib -- apply_to_store_increments_generation
   ```

### Expected

- Apply creates new resources when missing
- Apply reports `unchanged` for identical resources
- Apply reports `configured` when resources change
- Config version increments on each successful apply

---

## Scenario 2: Manifest Apply - Invalid Configuration

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm validation in `core/src/config_load/validate/`:
   - Empty `metadata.name` is rejected
   - Invalid resource specs are rejected before persistence
   - On validation error, entire apply is aborted (atomic — no partial writes)

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- validate_workflow_rejects_empty_steps
   cargo test --workspace --lib -- validate_workflow_rejects_no_enabled_steps
   cargo test --workspace --lib -- validate_workflow_config_rejects_duplicate_step_ids
   cargo test --workspace --lib -- resource_dispatch_rejects_mismatched_spec_kind
   ```

### Expected

- Invalid manifests are rejected with clear validation errors
- Existing runtime config remains unchanged after a rejected apply
- Atomic abort: no partial resources are persisted

---

## Scenario 3: Manifest Apply - Add New Workspace

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm project-scoped apply in `core/src/resource/apply.rs`:
   - `apply_to_project()` routes Workspace resources to project scope
   - `ensure_project()` auto-creates project entry if needed
   - Existing workspaces in other projects are unaffected

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- apply_to_project_routes_workspace_to_project_scope
   cargo test --workspace --lib -- apply_to_project_auto_creates_project_entry
   cargo test --workspace --lib -- apply_to_project_returns_unchanged_for_identical
   cargo test --workspace --lib -- apply_to_store_returns_created_for_new_resource
   ```

### Expected

- New workspace is persisted in the specified project scope
- Project entry is auto-created if it doesn't exist
- Existing workspaces in other projects remain available

---

## Scenario 4: Delete Project Clears Task State

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm delete logic in `core/src/service/resource.rs`:
   - `delete_project()` calls `reset_project_data()` to clear tasks, items, events
   - Project entry is removed from config
   - Resource store entries for the project are removed
   - Other projects are unaffected

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- registered_resource_delete_from_removes_project
   cargo test --workspace --lib -- delete_from_store_removes_from_store_and_config_snapshot
   cargo test --workspace --lib -- delete_from_store_returns_false_for_missing
   ```

### Expected

- Delete project clears all task records within the target project
- Project entry and resource store entries are removed
- Other project data is unaffected
- Delete returns false for non-existent projects

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Manifest Apply - Update Configuration | PASS | 2026-03-18 | Claude | 5/5 tests passed |
| 2 | Manifest Apply - Invalid Configuration | PASS | 2026-03-18 | Claude | 4/4 tests passed |
| 3 | Manifest Apply - Add New Workspace | PASS | 2026-03-18 | Claude | 4/4 tests passed |
| 4 | Delete Project Clears Task State | PASS | 2026-03-18 | Claude | 3/3 tests passed |
