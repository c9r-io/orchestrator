---
self_referential_safe: false
self_referential_safe_scenarios: [S4]
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
- default rolling file logs under `data/logs/system`
- preservation of human-readable command results on stdout

Entry point: `orchestrator`

---

## Scenario 1: Release Build Includes Logging Surface

### Preconditions

- Repository root is available

### Goal

Ensure the latest binary is built before QA execution and exposes the logging configuration surface. Note: `--log-level` and `--log-format` CLI flags do not exist. Logging is configured via environment variables (`ORCHESTRATOR_LOG` or `RUST_LOG` for level, `ORCHESTRATOR_LOG_FORMAT` for format) and the `-v, --verbose` CLI flag for debug-level override.

### Steps

1. Build the release binary:
   ```bash
   cargo build --release -p orchestratord -p orchestrator-cli
   ```
2. Verify the verbose flag appears in help:
   ```bash
   orchestrator --help | rg -- "-v|--verbose"
   ```
3. Verify env var logging works:
   ```bash
   ORCHESTRATOR_LOG=debug orchestrator --help > /dev/null 2>&1; echo "exit=$?"
   ```

### Expected

- Release build exits with code `0`
- Top-level help lists `-v, --verbose` flag
- Environment variable `ORCHESTRATOR_LOG` is accepted without error

---

## Scenario 2: `init` Preserves stdout Contract

### Preconditions

- Release binary exists

### Goal

Ensure preflight command results remain on stdout even when structured logging is enabled.

### Steps

1. Capture stdout and stderr separately:
   ```bash
   orchestrator init > /tmp/orch-init-stdout.txt 2> /tmp/orch-init-stderr.txt
   ```
2. Inspect captured output:
   ```bash
   cat /tmp/orch-init-stdout.txt
   cat /tmp/orch-init-stderr.txt
   ```

### Expected

- Stdout contains `Orchestrator initialized at ...`
- Stderr may contain logging output, but does not replace or suppress the stdout success line

---

## Scenario 3: JSON Console Logging Works Via Environment Variable

### Preconditions

- Release binary exists

### Goal

Ensure `ORCHESTRATOR_LOG_FORMAT=json` switches console logging to JSON on stderr. Note: `--log-format` CLI flag does not exist; use the environment variable instead.

### Steps

1. Run `init` with JSON console logging via env var:
   ```bash
   ORCHESTRATOR_LOG_FORMAT=json orchestrator init > /tmp/orch-json-stdout.txt 2> /tmp/orch-json-stderr.txt
   ```
2. Inspect stderr:
   ```bash
   cat /tmp/orch-json-stderr.txt
   ```

### Expected

- Stderr contains at least one JSON log line
- The JSON log includes `structured logging initialized`
- Stdout still contains the human-readable init success message

---

## Scenario 4: Rolling File Logs Are Written

### Preconditions

- At least one command has been run after logging bootstrap

### Goal

Ensure runtime logs are persisted under `data/logs/system`.

### Steps

1. List current system log files:
   ```bash
   ls -1 data/logs/system
   ```
2. Inspect the latest log file:
   ```bash
   LATEST_LOG=$(ls -1t data/logs/system | head -1)
   tail -n 20 "data/logs/system/${LATEST_LOG}"
   ```

### Expected

- `data/logs/system` exists
- At least one rolling log file exists
- The latest file contains the logging bootstrap record or subsequent structured runtime events

---

## Scenario 5: Logging Config Resolution Unit Tests Pass

### Preconditions

- Rust toolchain is available

### Goal

Ensure config defaults and CLI override precedence for structured logging remain covered by tests.

### Steps

1. Run focused tests:
   ```bash
   cd core && cargo test --lib config::observability::tests:: observability::init::tests::
   ```

### Expected

- Focused unit tests pass
- Coverage includes default config values and CLI override behavior

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Release Build Includes Logging Surface | ☐ | | | Previous result based on non-existent `--log-level`/`--log-format` flags |
| 2 | `init` Preserves stdout Contract | ✅ | 2026-03-02 | Codex | stdout retained human-readable success line; stderr contained structured log |
| 3 | JSON Console Logging Works Via Environment Variable | ☐ | | | Previous result based on non-existent `--log-format` flag |
| 4 | Rolling File Logs Are Written | ☐ | | | Needs re-verification |
| 5 | Logging Config Resolution Unit Tests Pass | ✅ | 2026-03-02 | Codex | focused `config::observability` and `observability::init` tests passed |
