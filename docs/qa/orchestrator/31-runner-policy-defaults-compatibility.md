---
self_referential_safe: true
---

# Orchestrator - Runner Policy Defaults and Compatibility

**Module**: orchestrator
**Scope**: Validate backward-compatible policy parsing for explicit unsafe and legacy alias modes
**Scenarios**: 2
**Priority**: High

---

## Background

This document covers the compatibility-focused policy checks split from `docs/qa/orchestrator/21-runner-security-observability.md` to preserve the repository max-scenario rule:

- Explicit `unsafe` mode must continue to work when intentionally selected
- Legacy `policy: legacy` manifests must remain backward-compatible and normalize to `unsafe`

---

## Scenario 1: Explicit Unsafe Mode Remains Functional

### Goal

Ensure that explicitly setting `policy: unsafe` is accepted, serialized correctly, and enforcement rules are applied.

### Steps

1. **Unit test** — verify `unsafe` serialization:
   ```bash
   cargo test -p orchestrator-config --lib test_unsafe_serializes_as_unsafe
   ```

2. **Unit test** — verify runner policy dispatch and kind:
   ```bash
   cargo test -p agent-orchestrator --lib runtime_policy_dispatch_and_kind
   ```

3. **Unit test** — verify unsafe policy runner spec:
   ```bash
   cargo test -p agent-orchestrator --lib runner_spec_unsafe_policy
   ```

4. **Unit test** — verify unsafe mode skips allowlist enforcement:
   ```bash
   cargo test -p agent-orchestrator --lib validate_accepts_unsafe_with_empty_lists
   ```

5. **Unit test** — verify runner config serde:
   ```bash
   cargo test -p orchestrator-config --lib test_runner_config_serde_round_trip
   cargo test -p orchestrator-config --lib test_runner_config_deserialize_minimal
   ```

6. **Unit test** — verify runner policy enforcement allows valid commands:
   ```bash
   cargo test -p agent-orchestrator --lib test_enforce_runner_policy_allows_valid_command
   cargo test -p agent-orchestrator --lib test_enforce_runner_policy_allows_newline_in_command
   ```

### Expected

- `unsafe` serializes as `"unsafe"` in YAML output
- Runtime policy dispatches correctly for unsafe kind
- Unsafe runner spec has no shell/arg allowlist enforcement
- Runner config round-trips with correct defaults
- Valid commands pass enforcement; newlines are allowed

---

## Scenario 2: Legacy Alias Backward Compatibility

### Goal

Ensure that `policy: legacy` in YAML manifests is accepted as a backward-compatible alias for `unsafe` and correctly normalized.

### Steps

1. **Unit test** — verify legacy normalizes to unsafe:
   ```bash
   cargo test -p agent-orchestrator --lib runner_spec_legacy_policy_normalizes_to_unsafe
   ```

2. **Unit test** — verify unknown policy falls back to allowlist:
   ```bash
   cargo test -p agent-orchestrator --lib runner_spec_unknown_policy_falls_back_to_allowlist
   ```

3. **Unit test** — verify default runner spec produces allowlist policy:
   ```bash
   cargo test -p agent-orchestrator --lib default_runner_spec_produces_allowlist
   ```

4. **Code review** — verify legacy alias handling in deserialization:
   ```bash
   rg -n "legacy|Legacy|normalize.*policy" core/src/resource/runtime_policy.rs crates/orchestrator-config/src/config/runner.rs
   ```

### Expected

- `policy: legacy` is accepted and normalized to `unsafe` on re-export
- Unknown policy strings fall back to `allowlist` (safe default)
- Default runner produces `allowlist` policy (not unsafe)
- Legacy alias handling is in the deserialization path, not a runtime check

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Explicit Unsafe Mode Remains Functional | ☐ | | | |
| 2 | Legacy Alias Backward Compatibility | ☐ | | | |
