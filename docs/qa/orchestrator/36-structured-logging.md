---
self_referential_safe: true
---

# Orchestrator - Structured Logging Bootstrap

**Module**: orchestrator
**Scope**: Validate structured logging initialization, CLI log overrides, stderr/stdout separation, and rolling file output
**Scenarios**: 5
**Priority**: High

---

## Background

This document validates the structured logging bootstrap introduced for the Rust CLI:

- `tracing`-based runtime diagnostics
- Logging configured via environment variables: `ORCHESTRATOR_LOG` or `RUST_LOG` for level, `ORCHESTRATOR_LOG_FORMAT` for format
- CLI flag `-v, --verbose` for debug-level override
- daemon log file at `data/daemon.log`
- preservation of human-readable command results on stdout

Entry point: `orchestrator`

---

## Scenario 1: Release Build Includes Logging Surface

### Preconditions

- Rust toolchain available

### Goal

Verify the logging configuration surface exists (env vars, `-v` flag) — validated via code review + implicit compilation from `cargo test`.

### Steps

1. **Code review** — verify CLI verbose flag definition:
   ```bash
   rg -n "verbose|ORCHESTRATOR_LOG|RUST_LOG" crates/cli/src/ | head -10
   ```

2. **Code review** — verify logging bootstrap accepts env vars:
   ```bash
   rg -n "ORCHESTRATOR_LOG|ORCHESTRATOR_LOG_FORMAT|init_logging|init_tracing" crates/ core/src/ | head -15
   ```

3. **Unit test** — implicit compilation verifies binary builds without error:
   ```bash
   cargo test --workspace --lib -- observability_defaults 2>&1 | tail -5
   ```

### Expected

- CLI defines `-v, --verbose` flag in its argument struct
- Logging bootstrap reads `ORCHESTRATOR_LOG` / `ORCHESTRATOR_LOG_FORMAT` environment variables
- `observability_defaults_are_safe` test passes (compilation succeeds implicitly)

---

## Scenario 2: `init` Preserves stdout Contract

### Preconditions

- Rust toolchain available

### Goal

Verify that CLI commands write human-readable results to stdout and structured logs to stderr — validated via code review of the output architecture.

### Steps

1. **Code review** — verify stdout/stderr separation in CLI output:
   ```bash
   rg -n "println!|eprintln!|stdout|stderr|writeln!" crates/cli/src/ | head -20
   ```

2. **Code review** — verify logging subscriber writes to stderr (not stdout):
   ```bash
   rg -n "stderr|make_writer|with_writer" crates/ core/src/ --glob "*.rs" | grep -i "log\|trac\|subscrib" | head -10
   ```

3. **Unit test** — verify observability config defaults:
   ```bash
   cargo test --workspace --lib -- observability_serde_defaults 2>&1 | tail -5
   ```

### Expected

- CLI commands use `println!` / stdout for user-facing results
- Tracing subscriber directs log output to stderr (not stdout)
- Observability serde defaults test passes

---

## Scenario 3: JSON Console Logging Works Via Environment Variable

### Preconditions

- Rust toolchain available

### Goal

Verify that `ORCHESTRATOR_LOG_FORMAT=json` is accepted and switches console logging format — validated via code review + unit test.

### Steps

1. **Code review** — verify format parsing accepts "json" variant:
   ```bash
   rg -n "format_parse|LogFormat|json" crates/orchestrator-config/src/config/observability.rs | head -15
   ```

2. **Code review** — verify format env var is read during bootstrap:
   ```bash
   rg -n "ORCHESTRATOR_LOG_FORMAT|log_format" crates/ core/src/ | head -10
   ```

3. **Unit test** — run format parsing tests:
   ```bash
   cargo test --workspace --lib -- format_parse_accepts_common_variants 2>&1 | tail -5
   ```

### Expected

- `format_parse_accepts_common_variants` passes: "json", "text", "pretty" all parsed successfully
- `ORCHESTRATOR_LOG_FORMAT` is read from environment during logging init
- JSON format variant configures a JSON-structured tracing layer

---

## Scenario 4: Daemon Log File Is Written

### Preconditions

- At least one command has been run after logging bootstrap

### Goal

Ensure runtime logs are persisted to the daemon log file.

### Steps

1. Verify the daemon log file exists:
   ```bash
   ls -l data/daemon.log
   ```
2. Inspect the latest log entries:
   ```bash
   tail -n 20 data/daemon.log
   ```

### Expected

- `data/daemon.log` exists
- The file contains structured log entries with ISO 8601 timestamps
- Log entries include the logging bootstrap record or subsequent structured runtime events

---

## Scenario 5: Logging Config Resolution Unit Tests Pass

### Preconditions

- Rust toolchain available

### Goal

Ensure config defaults and CLI override precedence for structured logging remain covered by tests.

### Steps

1. **Unit test** — run observability config tests:
   ```bash
   cargo test --workspace --lib -- observability 2>&1 | tail -5
   ```

### Expected

- All observability unit tests pass (`observability_defaults_are_safe`, `observability_serde_defaults_missing_fields`, `format_parse_accepts_common_variants`, `level_parse_accepts_common_variants`)
- Coverage includes default config values and CLI override behavior

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Release Build Includes Logging Surface | ☐ | | | Code review + unit test (observability_defaults, implicit compilation) |
| 2 | `init` Preserves stdout Contract | ☐ | | | Code review (stdout/stderr separation, tracing subscriber) |
| 3 | JSON Console Logging Works Via Environment Variable | ☐ | | | Code review + unit test (format_parse_accepts_common_variants) |
| 4 | Daemon Log File Is Written | ✅ | 2026-03-18 | Claude | verified data/daemon.log exists with ISO 8601 timestamps and structured runtime events |
| 5 | Logging Config Resolution Unit Tests Pass | ☐ | | | Unit test (observability config tests) |
