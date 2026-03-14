# Design Doc 56: Sandbox denial detection & self_test empty-change guard (FR-044)

## Overview

Adds trace-level anomaly detection for sandbox write denials and a pre-compilation empty-change guard in the self_test step, closing the false-positive pipeline gap where EPERM errors were silently swallowed.

## Motivation

When the macOS seatbelt sandbox denies writes (EPERM), the agent process may still exit 0. Downstream steps (self_test, self_restart) succeed vacuously because no code changed, creating a degenerate loop. FR-044 addresses this at three layers:

1. **writable_paths** — add missing `proto/` (already done in commit `919a90f`)
2. **Anomaly detection** — surface sandbox denials in `task trace`
3. **Empty-change guard** — fail self_test early when implement produced no diff

## Design

### Sandbox Denial Anomaly

The event pipeline already emits `sandbox_denied` events with payload `{step, reason, resource_kind, ...}` from `phase_runner/record.rs`. The accumulator tracks `sandbox_denied_count` and propagates it into pipeline variables and finalize context.

What was missing: the `task trace` anomaly system did not scan for these events.

**New anomaly rule**: `SandboxDenied` — severity `Error`, escalation `Intervene`.

**Detector**: `detect_sandbox_denied()` in `scheduler/trace/anomaly.rs`:
- Scans events for `event_type == "sandbox_denied"`
- Aggregates by step name
- Emits one anomaly per affected step with denial count and first occurrence timestamp

### Self-Test Empty-Change Guard

Before running `cargo check`, self_test now executes `git diff --stat HEAD`:
- If stdout is empty → no code changes after implement → return `exit_code=1` with `"[empty_change_check] no code changes detected after implement step"`
- If git command fails → assume changes exist (fail-open to avoid blocking on git issues)

This phase emits `self_test_phase` events with `phase: "empty_change_check"` for observability.

## Files Changed

| File | Role |
|------|------|
| `core/src/anomaly.rs` | `SandboxDenied` variant in `AnomalyRule` enum |
| `core/src/scheduler/trace/anomaly.rs` | `detect_sandbox_denied()` function + unit tests |
| `core/src/scheduler/trace/builder.rs` | Wire detector into trace builder |
| `core/src/scheduler/safety/self_test.rs` | `git diff --stat HEAD` empty-change guard |
