---
self_referential_safe: true
---

# Orchestrator - Config Self-Heal Auditability

**Module**: orchestrator
**Scope**: `query_heal_log_entries` backend function, `check` enhancement for persisted self-heal audit, `config_heal_log` table
**Scenarios**: 5
**Priority**: High

---

## Background

The orchestrator auto-heals persisted config drift on startup (e.g., removing `required_capability` from builtin steps). Previously, self-heal results were only visible as in-memory notices that disappeared on process restart. Phase 3 Task 03 adds:

- A `config_heal_log` table that persists structured change details alongside the healed config version
- A `query_heal_log_entries` backend function for querying self-heal history (no CLI surface — the `config` subcommand is not implemented)
- Enhanced `check` command that reads persisted heal records when no in-memory notice exists

### Key Files

| File | Role |
|------|------|
| `core/src/db.rs` | `config_heal_log` table schema |
| `core/src/config_load/persist.rs` | `persist_heal_log`, `query_heal_log_entries`, `query_latest_heal_summary` |
| `core/src/config_load/build.rs` | `build_active_config_with_self_heal` writes heal log in same transaction |
| `core/src/config_load/self_heal.rs` | `ConfigSelfHealRule` with Display + Serialize |
| `crates/cli/src/cli.rs` | current public CLI surface reference |
| `core/src/config_load/persist.rs` | heal-log persistence and query paths |
| `core/src/service/system.rs` | persisted heal notice fallback used by daemon-backed checks |

---

## Scenario 1: Heal Log Persisted During Self-Heal

### Preconditions

- Orchestrator binary built and available
- A config with a healable drift (builtin step with `required_capability`) persisted in DB

### Goal

Verify that `build_active_config_with_self_heal` writes structured entries to `config_heal_log` table.

### Steps

1. Run the unit test that validates heal log persistence:
   ```bash
   cd core && cargo test --lib build_active_config_with_self_heal_persists_heal_log_entries -- --nocapture
   ```

2. Run the persist_heal_log roundtrip test:
   ```bash
   cd core && cargo test --lib persist_heal_log_roundtrip -- --nocapture
   ```

### Expected

- Both tests pass
- `build_active_config_with_self_heal_persists_heal_log_entries` confirms: heal log entries are written with correct version, original error, workflow_id, step_id, rule, and detail
- `persist_heal_log_roundtrip` confirms: entries are queryable via `query_heal_log_entries` in DESC order

---

## Scenario 2: Heal Log Query Backend - Table Output

### Preconditions

- Orchestrator crate compiles

### Goal

Verify `query_heal_log_entries` returns structured heal log data. Note: the `config heal-log` CLI command is not implemented; heal log data is queryable programmatically via the backend function.

### Steps

1. Run the query_heal_log_entries unit tests:
   ```bash
   cd core && cargo test --lib query_heal_log_entries -- --nocapture
   ```

### Expected

- `query_heal_log_entries` tests pass
- Entries are returned in DESC order by creation time
- Each entry contains: version, original_error, workflow_id, step_id, rule, detail, created_at
- Exit code 0

---

## Scenario 3: Heal Log Query Backend - JSON Serialization

### Preconditions

- Orchestrator crate compiles

### Goal

Verify that heal log entries serialize correctly to JSON. Note: the `config heal-log --json` CLI command does not exist; JSON serialization is tested via unit tests for the query function.

### Steps

1. Run the query_heal_log_entries unit tests (covers serialization):
   ```bash
   cd core && cargo test --lib query_heal_log_entries -- --nocapture
   ```

### Expected

- Tests pass
- Each entry has fields: `version`, `original_error`, `workflow_id`, `step_id`, `rule`, `detail`, `created_at`
- `rule` values are stable enum labels (e.g., `"DropRequiredCapabilityFromBuiltinStep"`, `"NormalizeStepExecutionMode"`)
- Exit code 0

---

## Scenario 4: Check Command Shows Persisted Heal Warning

### Preconditions

- A self-heal event persisted in DB for the current active config version
- No in-memory notice (simulating a process restart)

### Goal

Verify that `check` falls back to DB-persisted heal log when no in-memory notice exists.

### Steps

1. Run the query_latest_heal_summary tests:
   ```bash
   cd core && cargo test --lib query_latest_heal_summary -- --nocapture
   ```

### Expected

- `query_latest_heal_summary_returns_none_when_empty` passes
- `query_latest_heal_summary_returns_summary_for_matching_version` passes: returns summary when DB heal version matches current config version
- `query_latest_heal_summary_returns_none_for_non_matching_version` passes: returns None when config version has advanced past the heal version

---

## Scenario 5: ConfigSelfHealRule Display and Serialize Stability

### Preconditions

- Orchestrator crate compiles

### Goal

Verify that `ConfigSelfHealRule` has stable Display and Serialize representations used for DB persistence and CLI output.

### Steps

1. Run Display tests:
   ```bash
   cd core && cargo test --lib config_self_heal_rule_display -- --nocapture
   ```

2. Run Serialize test:
   ```bash
   cd core && cargo test --lib config_self_heal_rule_serializes -- --nocapture
   ```

3. Run as_label consistency test:
   ```bash
   cd core && cargo test --lib config_self_heal_rule_as_label -- --nocapture
   ```

### Expected

- `DropRequiredCapabilityFromBuiltinStep` displays as `"DropRequiredCapabilityFromBuiltinStep"`
- `NormalizeStepExecutionMode` displays as `"NormalizeStepExecutionMode"`
- `as_label()` output matches `to_string()` for all variants
- JSON serialization includes the variant name as a string
- All 3 tests pass

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Heal Log Persisted During Self-Heal | ✅ PASS | 2026-03-30 | Claude | Both tests passed |
| 2 | Heal Log Query Backend - Table Output | ✅ PASS | 2026-03-30 | Claude | 2 tests passed, DESC order confirmed |
| 3 | Heal Log Query Backend - JSON Serialization | ✅ PASS | 2026-03-30 | Claude | 2 tests passed, field serialization confirmed |
| 4 | Check Command Shows Persisted Heal Warning | ✅ PASS | 2026-03-30 | Claude | 3 tests passed (empty, matching, non-matching versions) |
| 5 | ConfigSelfHealRule Display and Serialize Stability | ✅ PASS | 2026-03-30 | Claude | 3 tests passed (display, serialize, as_label) |
