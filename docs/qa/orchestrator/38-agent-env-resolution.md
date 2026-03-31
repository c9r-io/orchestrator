---
self_referential_safe: true
---

# Orchestrator - Agent Env Resolution and Runner Injection

**Module**: orchestrator
**Scope**: Agent env entry forms, runtime resolution, runner injection, validation, and secret redaction
**Scenarios**: 5
**Priority**: High

---

## Background

Agents can now declare environment variables via the `env` field in their spec. Three entry forms are supported:

1. **Direct value**: `name` + `value` — sets a literal env var
2. **From ref**: `fromRef` — imports all keys from a named EnvStore/SecretStore
3. **Ref value**: `name` + `refValue` — imports a single key from a named store, optionally renaming it

Resolution happens at runtime via `resolve_agent_env()`. Resolved variables are injected into spawned processes via the `extra_env` parameter on `spawn_with_runner()`. SecretStore values are automatically collected for log redaction.

---

## Scenario 1: Agent with Direct Value Env Entry

### Preconditions
- Rust toolchain available
- Unit tests available: `cargo test resolve_direct_value`

### Goal
Verify that an agent with a direct `name` + `value` env entry correctly resolves the variable.

### Steps
1. Run the direct value resolution unit test:
   ```bash
   cargo test --workspace --lib resolve_direct_value
   ```

2. Review the resolve implementation:
   ```bash
   rg -n "fn resolve_agent_env\b|Direct.*value" crates/orchestrator-config/src/env_resolve.rs
   ```

### Expected
- `resolve_direct_value` passes: direct env entry produces `(name, value)` in resolved map
- The env var is available for runner injection

---

## Scenario 2: Agent with fromRef Importing All Store Keys

### Preconditions
- Unit tests available

### Goal
Verify that `fromRef` imports all keys from the referenced store into the agent's environment.

### Steps
1. Run the fromRef resolution unit test:
   ```bash
   cargo test --workspace --lib resolve_from_ref
   ```

### Expected
- `resolve_from_ref` passes: all keys from the referenced store appear in the resolved env map
- Both EnvStore and SecretStore references are supported

---

## Scenario 3: Agent with refValue Importing Single Key with Rename

### Preconditions
- Unit tests available

### Goal
Verify that `name` + `refValue` imports a single key from the referenced store, and the env var name can differ from the store key name.

### Steps
1. Run the refValue resolution unit test:
   ```bash
   cargo test --workspace --lib resolve_ref_value
   ```

### Expected
- `resolve_ref_value` passes: single key imported with rename
- The env var name is the `name` field, not the store's key name

---

## Scenario 4: Config Validation Rejects Missing Store References

### Preconditions
- Unit tests available

### Goal
Verify that config build-time validation catches agents referencing non-existent stores and produces a clear error message.

### Steps
1. Run the missing store reference unit tests:
   ```bash
   cargo test --workspace --lib resolve_missing_store_errors
   cargo test --workspace --lib resolve_missing_key_errors
   cargo test --workspace --lib resolve_invalid_entry_errors
   ```

2. Run the config validation unit tests:
   ```bash
   cargo test --workspace --lib validate_agent_env_store_refs
   ```

### Expected
- `resolve_missing_store_errors` passes: references to non-existent stores produce clear errors
- `resolve_missing_key_errors` passes: references to non-existent keys produce clear errors
- `resolve_invalid_entry_errors` passes: malformed env entries are rejected
- Config validation catches missing store references before config is written

---

## Scenario 5: SecretStore Values Redacted in Task Logs

### Preconditions
- Unit tests available

### Goal
Verify that SecretStore values are collected by `collect_sensitive_values()` and available for redaction. EnvStore values should NOT be collected.

### Steps
1. Run the sensitive value collection unit tests:
   ```bash
   cargo test --workspace --lib collect_sensitive_values_from_secret_store
   cargo test --workspace --lib collect_sensitive_values_skips_env_stores
   cargo test --workspace --lib test_collect_all_sensitive_store_values
   cargo test --workspace --lib test_collect_all_sensitive_store_values_empty
   ```

### Expected
- `collect_sensitive_values_from_secret_store` passes: SecretStore values are collected for redaction
- `collect_sensitive_values_skips_env_stores` passes: EnvStore values are NOT collected
- Sensitive value collection is used by the runner for log redaction

---

## General Scenario: Override Precedence — Later Entries Win

### Steps
1. Run the override precedence unit test:
   ```bash
   cargo test --workspace --lib resolve_later_entries_override_earlier
   ```

### Expected
- `resolve_later_entries_override_earlier` passes: when multiple entries define the same key, the last one wins

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Agent with direct value env entry | PASS | 2026-03-31 | Claude | `resolve_direct_value` passed |
| 2 | Agent with fromRef importing all store keys | PASS | 2026-03-31 | Claude | `resolve_from_ref` passed |
| 3 | Agent with refValue importing single key with rename | PASS | 2026-03-31 | Claude | `resolve_ref_value` passed |
| 4 | Config validation rejects missing store references | PASS | 2026-03-31 | Claude | 7 sub-tests passed: `resolve_missing_store_errors`, `resolve_missing_key_errors`, `resolve_invalid_entry_errors`, `validate_agent_env_store_refs` (4 sub-tests) |
| 5 | SecretStore values redacted in task logs | PASS | 2026-03-31 | Claude | 5 sub-tests passed: `collect_sensitive_values_from_secret_store`, `collect_sensitive_values_skips_env_stores`, `test_collect_all_sensitive_store_values` (2 tests), `test_collect_all_sensitive_store_values_empty` |
| G | Override precedence — later entries win | PASS | 2026-03-31 | Claude | `resolve_later_entries_override_earlier` passed |
