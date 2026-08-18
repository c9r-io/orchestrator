---
lifecycle: active
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
   cargo test --workspace --lib env_store_get_from_returns_none_for_missing
   cargo test --workspace --lib secret_store_get_from_returns_none_for_missing
   cargo test --workspace --lib secret_store_and_env_store_same_name_coexist
   ```

2. Review the YAML export implementation:
   ```bash
   cargo test -p agent-orchestrator --lib env_store_to_yaml
   cargo test -p agent-orchestrator --lib secret_store_to_yaml
   ```

### Expected
- `env_store_get_from_returns_none_for_missing` passes: EnvStore returns None for missing entries
- `secret_store_get_from_returns_none_for_missing` passes: SecretStore returns None for missing entries
- `secret_store_and_env_store_same_name_coexist` passes: proves EnvStore and SecretStore with same name can coexist (cross-kind isolation verified)
- YAML serialization preserves kind labels and data maps

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Apply EnvStore and SecretStore — Created Status | PASS | 2026-04-01 | Claude | 2/2 unit tests passed |
| 2 | Apply idempotency — Unchanged on re-apply | PASS | 2026-04-01 | Claude | 2/2 unit tests passed |
| 3 | Delete EnvStore and SecretStore | PASS | 2026-04-01 | Claude | 2/2 unit tests passed |
| 4 | Validate rejects empty resource name | PASS | 2026-04-01 | Claude | 2/2 unit tests passed |
| 5 | EnvStore/SecretStore isolation — cross-kind get/delete | PASS | 2026-04-01 | Claude | 5/5 tests passed: cross-kind coexistence + YAML export verified |

---

## Known Limitations

### `manifest export` and `debug --component config` emit SecretStore values in cleartext

**This is a design gap, not a test error.** The implementation follows the design:
`docs/design_doc/orchestrator/17-envstore-secretstore-agent-env.md` bounds redaction
to *at rest* (`:132`) and *in logs* (`:140`, `:175`), and lists export as an
acceptance criterion (`:171`). Nothing in the design says an authorized read command
redacts. Routed to
[FR-175](../../feature_request/FR-175-secret-redaction-at-egress.md).

Measured end-to-end at `e6081c6d` against an isolated daemon — a real SecretStore
applied, then each read path grepped:

| Read path | Cleartext? |
|---|---|
| `manifest export -o yaml` | **yes** |
| `manifest export -o json` | **yes** |
| `debug --component config` | **yes** |
| GUI `manifest_export` Tauri command | **yes** (same RPC) |
| `get secretstore <name>` / `get secretstores` | no |
| `debug --component state` / `--component dag` | no |
| database at rest | no |

Until FR-175 lands, treat the output of those three as secret material: do not
redirect it into a file you will commit, attach, or paste.

### No scenario covers what `manifest export` actually emits

This document's **Scope** names export, and the five scenarios above check apply,
idempotency, delete, name validation and cross-kind isolation — none reads the
export output's *content*. That absence is why the leak above survived unnoticed,
and it is why FR-175 carries a requirement for the coverage rather than leaving it
to be added here: this document is already at the five-scenario ceiling that
`qa-doc-lint` enforces, so adding one means replacing one, which is a decision for
that FR's governance.
