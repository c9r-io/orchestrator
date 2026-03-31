---
self_referential_safe: true
---

# Custom Resource Definitions (CRD) Extension System

**Module**: orchestrator
**Scope**: CRD registration, custom resource lifecycle (apply/get/list/describe/delete/export), schema+CEL validation, lifecycle hooks
**Scenarios**: 5
**Priority**: High

---

## Background

The CRD extension system allows users to define new resource types beyond the 10 builtin kinds (Workspace, Agent, Workflow, Project, Defaults, RuntimePolicy, StepTemplate, EnvStore, SecretStore, Trigger). A CRD defines the kind name, plural form, short aliases, API group, versioned JSON Schema, CEL validation rules, and lifecycle hooks. Custom resource instances are validated against their CRD's schema and CEL rules before being persisted.

> **Note**: Since the unified CRD migration (see `42-crd-unified-resource-store.md`), the 10 builtin types are themselves registered as CRDs with `builtin: true`. All resources (builtin + user-defined) flow through the same ResourceStore pipeline. This document covers **user-defined CRD** behavior; for builtin CRD store mechanics, projection writeback, and normalization, see doc 42.

**Key design principles**:
- All resources stored in unified `ResourceStore` with composite key `"{Kind}/{name}"`
- Builtin CRDs are protected (`builtin: true`) — cannot be deleted or overwritten by users
- User-defined CRDs use untyped spec (`serde_json::Value`), validated at runtime via schema + CEL
- Two-phase YAML parsing: `kind` string is read first to route to builtin Resource trait or CRD validation path

**Unit test entry points**:
- `core/src/crd/mod.rs` — `apply_crd`, `apply_custom_resource`, `delete_crd`, `delete_custom_resource` tests
- `core/src/crd/validate.rs` — schema validation, CEL validation, CRD definition validation tests
- `core/src/crd/store.rs` — ResourceStore key isolation, generation, namespaced keys
- `core/src/crd/schema.rs` — JSON Schema type checks, pattern validation

---

## Scenario 1: CRD Registration and Custom Resource Creation

### Preconditions
- Rust toolchain available
- Unit tests available: `cargo test apply_crd`, `cargo test apply_custom_resource`

### Goal
Verify that a CRD can be registered and a custom resource instance created, with correct change detection.

### Steps

1. Run the CRD apply unit tests:
   ```bash
   cargo test --workspace --lib apply_crd_creates
   cargo test --workspace --lib apply_custom_resource_creates
   ```

2. Verify the CRD registration validates the definition:
   ```bash
   cargo test --workspace --lib validate_crd_valid
   ```

3. Review the apply implementation:
   ```bash
   rg -n "pub fn apply_crd\b|pub fn apply_custom_resource\b" core/src/crd/mod.rs
   ```

### Expected
- `apply_crd_creates` passes: new CRD returns `Created` status
- `apply_custom_resource_creates` passes: new CR instance returns `Created`
- CRD definition is validated before registration (kind name, versions, schema)

---

## Scenario 2: Schema and CEL Validation Rejects Invalid Resources

### Preconditions
- Unit tests available

### Goal
Verify that schema validation (missing required fields) and CEL validation (custom rules) correctly reject invalid custom resources.

### Steps

1. Run the validation unit tests:
   ```bash
   cargo test --workspace --lib apply_custom_resource_schema_validation_fails
   cargo test --workspace --lib apply_custom_resource_with_cel_validation
   cargo test --workspace --lib validate_custom_resource_schema_fail
   cargo test --workspace --lib validate_custom_resource_cel_fail
   cargo test --workspace --lib validate_custom_resource_no_crd
   ```

2. Review schema and CEL validation logic:
   ```bash
   rg -n "fn validate_custom_resource\b|fn validate_against_schema\b|fn evaluate_cel_rules\b" core/src/crd/validate.rs
   ```

### Expected
- `apply_custom_resource_schema_validation_fails` passes: missing required fields rejected
- `validate_custom_resource_cel_fail` passes: CEL rule violations rejected
- `validate_custom_resource_no_crd` passes: CR without registered CRD rejected
- No invalid CR instances are created when validation fails

---

## Scenario 3: Custom Resource Get, Describe, and Label Selector

### Preconditions
- CRD `PromptLibrary` registered and `qa-prompts` instance exists. Apply the fixture:
  ```bash
  orchestrator apply -f fixtures/manifests/bundles/crd-test.yaml --project crd-qa
  ```

### Goal
Verify get/describe/list operations work for custom resources, including label-based filtering.

### Steps

1. Get single CR in JSON format:
   ```bash
   orchestrator get pl/qa-prompts -o json
   ```

2. Describe the CR:
   ```bash
   orchestrator describe pl/qa-prompts
   ```

3. List with label selector:
   ```bash
   orchestrator get promptlibraries -l team=platform
   ```

4. List with non-matching label selector:
   ```bash
   orchestrator get promptlibraries -l team=nonexistent
   ```

### Expected
- JSON output contains `kind`, `apiVersion`, `metadata`, `spec`, `generation`
- Describe output shows resource details (kind, name, apiVersion, generation, timestamps, spec)
- Label selector `team=platform` returns `qa-prompts`
- Non-matching selector returns empty list

---

## Scenario 4: Custom Resource Delete and CRD Cascade Protection

### Preconditions
- Unit tests available

### Goal
Verify that deleting a CR works, deleting a CRD with existing instances is rejected (cascade protection), and deleting a CRD after all instances are removed succeeds.

### Steps

1. Run the delete and cascade protection unit tests:
   ```bash
   cargo test --workspace --lib delete_custom_resource_ok
   cargo test --workspace --lib delete_custom_resource_not_found
   cargo test --workspace --lib delete_crd_ok
   cargo test --workspace --lib delete_crd_blocked_by_instances
   ```

2. Review the cascade protection logic:
   ```bash
   rg -n "blocked_by_instances|cascade" core/src/crd/mod.rs
   ```

### Expected
- `delete_custom_resource_ok` passes: CR deleted successfully
- `delete_custom_resource_not_found` passes: deleting non-existent CR returns false
- `delete_crd_ok` passes: CRD deleted after all instances removed
- `delete_crd_blocked_by_instances` passes: CRD delete fails while instances exist

---

## Scenario 5: CRD Validation Rules — Kind, Schema, and CEL

### Preconditions
- Unit tests available

### Goal
Verify CRD definition validation rejects invalid kind names, missing versions, builtin kind collisions, and invalid CEL syntax.

### Steps

1. Run the CRD definition validation unit tests:
   ```bash
   cargo test --workspace --lib validate_crd_rejects_lowercase_kind
   cargo test --workspace --lib validate_crd_rejects_builtin_kind
   cargo test --workspace --lib validate_crd_rejects_builtin_plural
   cargo test --workspace --lib validate_crd_rejects_empty_group
   cargo test --workspace --lib validate_crd_rejects_no_versions
   cargo test --workspace --lib validate_crd_rejects_no_served_version
   cargo test --workspace --lib validate_crd_rejects_invalid_cel_syntax
   ```

2. Run the idempotency tests:
   ```bash
   cargo test --workspace --lib apply_crd_unchanged
   cargo test --workspace --lib apply_crd_configured
   cargo test --workspace --lib apply_custom_resource_unchanged
   ```

### Expected
- Lowercase kind names are rejected
- Builtin kind names and plural forms are protected
- CRD requires at least one version with `served: true`
- Invalid CEL syntax is caught at CRD registration time
- Re-applying unchanged CRD/CR returns `Unchanged`; changed spec returns `Configured`

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | CRD Registration and Custom Resource Creation | PASS | 2026-03-31 | claude | 3/3 unit tests passed — apply_crd_creates, apply_custom_resource_creates, validate_crd_valid |
| 2 | Schema and CEL Validation Rejects Invalid Resources | PASS | 2026-03-31 | claude | 5/5 unit tests passed — schema/CEL/no-crd rejection paths |
| 3 | Custom Resource Get, Describe, and Label Selector | PASS | 2026-03-31 | claude | Read-only CLI ops — get/describe/list all correct, label selector works |
| 4 | Custom Resource Delete and CRD Cascade Protection | PASS | 2026-03-31 | claude | 4/4 unit tests passed — delete ok/not-found, cascade protection |
| 5 | CRD Validation Rules — Kind, Schema, and CEL | PASS | 2026-03-31 | claude | 10/10 unit tests passed — kind/group/version/CEL validation + idempotency |
