---
self_referential_safe: true
---

# Orchestrator - EnvStore and SecretStore Resource CRUD

**Module**: orchestrator
**Scope**: EnvStore/SecretStore resource apply, get, delete, export, and validation
**Scenarios**: 5
**Priority**: High

---

## Background

Two new resource kinds (`EnvStore` and `SecretStore`) allow declaring reusable environment variable sets in YAML manifests. Both share the same `spec.data` shape (`HashMap<String, String>`) but differ in the `sensitive` flag: EnvStore is non-sensitive, SecretStore is sensitive (values are redacted in logs).

Both resources use the standard `apply` / `manifest export` / `delete` CLI commands and are stored in the unified `env_stores` config map.

---

## Scenario 1: Apply EnvStore and SecretStore — Created Status

### Preconditions
- Rust toolchain available
- Unit tests available: `cargo test env_store_apply`, `cargo test secret_store_apply`

### Goal
Verify that both EnvStore and SecretStore resources can be applied and return `Created` status for new entries, with correct sensitive flag.

### Steps
1. Run the apply unit tests:
   ```bash
   cargo test --workspace --lib env_store_apply_and_get
   cargo test --workspace --lib secret_store_apply_and_get
   ```

2. Review the EnvStore apply implementation:
   ```bash
   rg -n "fn apply_to\b|fn get_from\b" core/src/resource/env_store.rs
   rg -n "fn apply_to\b|fn get_from\b" core/src/resource/secret_store.rs
   ```

### Expected
- `env_store_apply_and_get` passes: EnvStore created with `sensitive=false`, data map preserved
- `secret_store_apply_and_get` passes: SecretStore created with `sensitive=true`, data map preserved
- Both store types share the `env_stores` config map, distinguished by `sensitive` flag

---

## Scenario 2: Apply Idempotency — Unchanged on Re-Apply

### Preconditions
- Unit tests available

### Goal
Verify that re-applying the same manifest produces `Unchanged` status for both resources (idempotent apply).

### Steps
1. Run the idempotency unit tests:
   ```bash
   cargo test --workspace --lib env_store_apply_unchanged
   cargo test --workspace --lib secret_store_apply_unchanged
   ```

### Expected
- `env_store_apply_unchanged` passes: second apply returns `Unchanged`
- `secret_store_apply_unchanged` passes: second apply returns `Unchanged`
- No data is modified on re-apply

---

## Scenario 3: Delete EnvStore and SecretStore

### Preconditions
- Unit tests available

### Goal
Verify that delete works correctly for both resource kinds and that deleting one kind does not affect the other.

### Steps
1. Run the delete unit tests:
   ```bash
   cargo test --workspace --lib env_store_delete
   cargo test --workspace --lib secret_store_delete
   ```

2. Review delete implementation:
   ```bash
   rg -n "fn delete_from\b" core/src/resource/env_store.rs core/src/resource/secret_store.rs
   ```

### Expected
- `env_store_delete` passes: EnvStore entry removed from config
- `secret_store_delete` passes: SecretStore entry removed from config
- Delete returns `true` for existing entries

---

## Scenario 4: Validate Rejects Empty Resource Name

### Preconditions
- Unit tests available

### Goal
Verify that applying an EnvStore or SecretStore with an empty name produces a validation error.

### Steps
1. Run the validation unit tests:
   ```bash
   cargo test --workspace --lib env_store_validate_rejects_empty_name
   cargo test --workspace --lib secret_store_validate_rejects_empty_name
   ```

### Expected
- `env_store_validate_rejects_empty_name` passes: empty name rejected with error
- `secret_store_validate_rejects_empty_name` passes: empty name rejected with error
- No resource is created when validation fails

---

## Scenario 5: EnvStore and SecretStore Isolation — Cross-Kind Get/Delete

### Preconditions
- Unit tests available

### Goal
Verify that `get_from` for EnvStore skips sensitive entries, and `get_from` for SecretStore skips non-sensitive entries. Also verify that `delete` for the wrong kind returns false.

### Steps
1. Run the isolation unit tests:
   ```bash
   cargo test --workspace --lib env_store_get_from_skips_sensitive
   cargo test --workspace --lib secret_store_get_from_skips_non_sensitive
   cargo test --workspace --lib env_store_get_from_returns_none_for_missing
   cargo test --workspace --lib secret_store_get_from_returns_none_for_missing
   ```

2. Review the YAML export implementation:
   ```bash
   cargo test --workspace --lib env_store_to_yaml
   cargo test --workspace --lib secret_store_to_yaml
   ```

### Expected
- `env_store_get_from_skips_sensitive` passes: EnvStore ignores SecretStore entries
- `secret_store_get_from_skips_non_sensitive` passes: SecretStore ignores EnvStore entries
- Cross-kind access is correctly blocked by the `sensitive` flag
- YAML serialization preserves kind labels and data maps

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Apply EnvStore and SecretStore — Created Status | PASS | 2026-03-20 | Claude | 2/2 unit tests passed |
| 2 | Apply idempotency — Unchanged on re-apply | PASS | 2026-03-20 | Claude | 2/2 unit tests passed |
| 3 | Delete EnvStore and SecretStore | PASS | 2026-03-20 | Claude | 2/2 unit tests passed |
| 4 | Validate rejects empty resource name | PASS | 2026-03-20 | Claude | 2/2 unit tests passed |
| 5 | EnvStore/SecretStore isolation — cross-kind get/delete | PASS | 2026-03-20 | Claude | 6/6 unit tests passed |
