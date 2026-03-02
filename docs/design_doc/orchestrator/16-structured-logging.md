# Structured Logging And Output Separation

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Replace ad-hoc runtime `println!`/`eprintln!` diagnostics with structured logging, preserve user-facing CLI output compatibility, and add configurable console/file sinks.
**Related QA**: `docs/qa/orchestrator/36-structured-logging.md`
**Created**: 2026-03-02
**Last Updated**: 2026-03-02

## Background And Goals

## Background

The orchestrator previously relied on scattered `println!` and `eprintln!` calls for internal diagnostics. That mixed human-facing CLI output with runtime diagnostics, made machine parsing fragile, and provided no structured log sink for operations.

The implementation introduces a central `tracing` bootstrap, a new `observability.logging` config surface, CLI log overrides, and a `TracingEventSink` bridge for runtime event summaries.

## Goals

- Separate user-facing command output from runtime diagnostics
- Emit structured logs to stderr and rolling files under `data/logs/system`
- Support global `--log-level` and `--log-format` overrides without breaking existing CLI command contracts
- Preserve existing SQLite event persistence while mirroring key event summaries into structured logs

## Non-goals

- Distributed tracing or OpenTelemetry export
- Replacing `task logs` or SQLite `events` as the system of record
- Migrating every existing user-facing `println!` to a new formatter in one pass

## Scope And User Experience

## Scope

- In scope:
  - New `observability.logging` config model in core config
  - Early logging initialization in `main`
  - Global CLI flags `--log-level` and `--log-format`
  - Default console + file sinks
  - Migration of key internal warning/error paths to `tracing`
- Out of scope:
  - Resource-manifest support for mutating observability config from `apply`
  - Full rewrite of all CLI output handlers

## UI Interactions (If Applicable)

- CLI only
- User-facing command results remain on stdout
- Structured runtime logs default to stderr and file sinks

## Key Design And Tradeoffs

## Key Design

1. Centralize logging bootstrap in `core/src/observability/init.rs` so runtime code only emits events and does not build subscribers.
2. Keep CLI result output and runtime logs on separate channels to preserve script compatibility.
3. Use `TracingEventSink` as a one-way bridge from runtime events to logs, while preserving SQLite event persistence.
4. Default to developer-friendly console logs plus JSON file logs.

## Alternatives And Tradeoffs

- Option A: Continue using `println!` with formatting conventions
  - Pros: No new dependency surface
  - Cons: No structure, poor filtering, mixed channels
- Option B: Full output abstraction rewrite first
  - Pros: Stronger long-term consistency
  - Cons: Larger migration and higher regression risk
- Why we chose: Introduce structured logging first and preserve existing command contracts, then iterate on wider output cleanup later.

## Risks And Mitigations

- Risk: stderr pollution could break scripts that read both streams
  - Mitigation: keep user-result output on stdout and only route runtime logs to stderr
- Risk: early logging init may fail before runtime startup
  - Mitigation: use safe defaults and create the log directory lazily
- Risk: event/log duplication may confuse future contributors
  - Mitigation: document that SQLite events remain audit truth; tracing is an operational mirror

## Observability And Operations

## Observability

- Logs:
  - `structured logging initialized`
  - task runner failures
  - checkpoint / rollback warnings
  - self-test phase failures
  - event summaries from `TracingEventSink`
- Metrics:
  - No new metrics in this change
- Tracing:
  - task runner span includes `task_id`

## Operations / Release

- Config:
  - CLI flags: `--log-level`, `--log-format`
  - Env overrides: `ORCHESTRATOR_LOG`, `RUST_LOG`, `ORCHESTRATOR_LOG_FORMAT`
- Migration / rollback:
  - No DB migration required
  - Reverting to previous behavior only requires removing the new bootstrap/module wiring
- Compatibility:
  - Existing CLI stdout contracts remain intact
  - Existing config rows deserialize via defaults when `observability` is absent

## Testing And Acceptance

## Test Plan

- Unit tests:
  - `config::observability::*`
  - `observability::init::*`
- Integration / CLI:
  - build release binary
  - verify `--log-level` and `--log-format` are accepted
  - verify `init` writes stable stdout while emitting structured stderr logs
  - verify `data/logs/system` receives the log record

## QA Docs

- `docs/qa/orchestrator/36-structured-logging.md`

## Acceptance Criteria

- Runtime initializes structured logging before preflight commands
- `init` continues to emit a human-readable success message on stdout
- `--log-format json` emits JSON-formatted console logs on stderr
- At least one rolling file is written under `data/logs/system`
- Existing library tests continue to pass
