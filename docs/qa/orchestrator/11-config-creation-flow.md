---
self_referential_safe: true
---

# Orchestrator - 配置创建流程测试

**Module**: orchestrator
**Scope**: 验证通过 apply 命令创建配置资源的流程
**Scenarios**: 4
**Priority**: High

---

## Background

测试使用 `apply` 命令创建 workspace、agent、workflow 配置资源。核心逻辑通过 `core/src/resource/tests.rs` 和 `core/src/config_load/validate/tests.rs` 中的单元测试全面覆盖。

---

## Scenario 1: 创建 Workspace (dry-run)

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm dry-run logic in `core/src/service/resource.rs`:
   - When `--dry-run` is set, `persist_config_and_reload()` is never called
   - Resources are validated and merged in-memory only
   - Results show "would be created (dry run)" messages
   - No database writes occur

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- apply_result_created_when_missing
   cargo test --workspace --lib -- apply_to_store_returns_created_for_new_resource
   ```

### Expected

- Dry-run validates without persisting
- Apply result correctly reports "created" status for new resources
- No side effects on database

---

## Scenario 2: 创建 Workspace (实际)

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm workspace creation in `core/src/resource/apply.rs`:
   - `apply_to_project()` routes Workspace to project scope
   - New workspace gets `generation: 1`
   - Config snapshot is updated

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- apply_to_project_routes_workspace_to_project_scope
   cargo test --workspace --lib -- apply_to_store_returns_created_for_new_resource
   cargo test --workspace --lib -- apply_to_store_increments_generation
   ```

### Expected

- Workspace is created with correct metadata
- Generation starts at 1 for new resources
- Config snapshot is updated to reflect new workspace

---

## Scenario 3: 创建完整的最小配置

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm multi-resource apply:
   - `apply_manifests()` processes multiple resources in order
   - Each resource type (Workspace, Agent, Workflow) is dispatched correctly via `resource_dispatch()`
   - Validation catches schema errors (e.g., wrong `kind`/`spec` pairing)

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- resource_dispatch_maps_workspace_manifest
   cargo test --workspace --lib -- resource_dispatch_rejects_mismatched_spec_kind
   cargo test --workspace --lib -- apply_to_project_routes_agent_to_project_scope
   cargo test --workspace --lib -- apply_to_project_routes_workflow_to_project_scope
   cargo test --workspace --lib -- validate_workflow_accepts_builtin_step_without_agent
   cargo test --workspace --lib -- validate_workflow_accepts_command_step_without_agent
   ```

### Expected

- All resource types (Workspace, Agent, Workflow) are created successfully
- Resource dispatch correctly maps manifest `kind` to resource type
- Workflow validation accepts valid step configurations

---

## Scenario 4: 资源存在时 apply (更新)

### Verification Method

Code review + unit test verification.

### Steps

1. **Code review** — confirm update-on-existing logic in `core/src/resource/apply.rs`:
   - When a resource already exists, `apply_to_store()` compares with config snapshot
   - Changed resources get `configured` result and incremented generation
   - Identical resources get `unchanged` result

2. **Unit test verification**:
   ```bash
   cargo test --workspace --lib -- apply_result_configured_when_resource_changes
   cargo test --workspace --lib -- apply_result_unchanged_for_identical_resource
   cargo test --workspace --lib -- apply_to_store_returns_configured_for_changed
   cargo test --workspace --lib -- apply_to_store_returns_unchanged_for_identical
   cargo test --workspace --lib -- apply_to_store_seeds_from_config_snapshot_for_correct_change_detection
   ```

### Expected

- Updating an existing resource reports `configured`
- Unchanged resources report `unchanged` (no unnecessary writes)
- Generation increments only on actual changes
- Config snapshot is used for correct change detection

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | 创建 Workspace (dry-run) | ☑ | 2026-03-18 | Claude | Code review + unit test verified |
| 2 | 创建 Workspace (实际) | ☑ | 2026-03-18 | Claude | Code review + unit test verified |
| 3 | 创建完整的最小配置 | ☑ | 2026-03-18 | Claude | Code review + unit test verified |
| 4 | 资源存在时 apply (更新) | ☑ | 2026-03-18 | Claude | Code review + unit test verified |
