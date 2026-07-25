---
lifecycle: active
related_fr: FR-121
self_referential_safe: true
---

# Orchestrator - Attention Mutation Error Reconciliation

**Module**: Orchestrator
**Scope**: Safe error boundary, shared mutation lifecycle, authoritative reconciliation, focus, and privacy-safe telemetry
**Scenarios**: 5
**Priority**: High

---

## Background

This QA overlay verifies FR-121 without changing the active Orchestrator database. Component and browser tests use mocked Tauri boundaries. The daemon script starts an isolated instance on `127.0.0.1:19196` with temporary HOME and data directories.

The original Attention lifecycle remains covered by QA-143. This document owns mutation failure visibility and reconciliation behavior.

## Scenario 1: Shared Mutation Failure And Confirmed Reconciliation

### Goal

Verify Claim, Snooze, Resolve, and custom Action use one failure contract and retain the mutation cause after a successful authoritative reload.

### Steps

1. Run:

   ```bash
   cd gui
   npm test -- --run src/pages/AttentionInbox.component.test.tsx
   ```

2. Inspect the parameterized `claim`, `snooze`, `resolve`, and `execute` cases.
3. Confirm the conflict-specific test changes the authoritative assignee to `operator-b` and version to `2`.

### Expected

- Every mutation kind renders the same persistent alert and confirmed-reconciliation copy.
- The authoritative assignee/version replaces local state.
- The failed action never enters the success live region.
- A removed initiating control returns focus to the Attention queue listbox.

## Scenario 2: Dual Failure, Explicit State Retry, Success, And Dismiss

### Goal

Verify error clearing rules and the unconfirmed-state safety path.

### Steps

1. Run the component test command from Scenario 1.
2. Confirm the double-failure case rejects both Claim and its following `AttentionList`.
3. Confirm “Retry latest state check” later succeeds.
4. Confirm the same-operation success case invokes Claim twice with two distinct UUIDs.
5. Confirm the dismiss case removes only the mutation alert.

### Expected

- Mutation and query causes remain simultaneously visible after dual failure.
- Explicit retry invokes only `attention_list`, then clears the old mutation and query errors.
- A later successful business action uses a new idempotency key and clears its matching old error.
- Dismiss does not change the restored item.

## Scenario 3: Safe Error And Metric Privacy Boundary

### Goal

Verify raw provider/internal content cannot reach the UI or metric dimensions.

### Steps

1. Run:

   ```bash
   cargo test -p orchestrator-gui errors::tests
   cargo test -p agent-orchestrator \
     process_metrics::tests::attention_observations_accept_only_privacy_safe_dimensions
   cd gui
   npm test -- --run src/lib/telemetry.test.ts
   ```

2. Inspect test payloads containing token, Slack-body, database-path, and requested-decision markers.

### Expected

- UI error output contains only an allowlisted category/message and optional validated request ID.
- `attention_mutation_total` accepts only action/result/error_category.
- `attention_reconciliation_total` accepts only action/result.
- A requested-decision dimension is rejected.

## Scenario 4: Two-Client Version Competition And Authoritative Reread

### Goal

Verify the daemon contract used by GUI reconciliation with real independent clients.

### Steps

1. Build debug binaries if required:

   ```bash
   cargo build -p orchestratord -p orchestrator-cli
   ```

2. Run:

   ```bash
   ./scripts/qa/test-attention-inbox.sh
   ```

3. Confirm all ten assertions pass.

### Expected

- Exactly one of two claims against the same version succeeds.
- The loser receives the stable version-conflict category.
- `attention get` returns a newer claimed row with the winning assignee.
- Canonical audit contains two distinct retry identities.

## Scenario 5: Browser Alert, Focus, Accessibility, And Full Regression

### Goal

Verify the operator-visible conflict flow and retain the complete Console regression gate.

### Steps

1. Run the focused browser scenario:

   ```bash
   cd gui
   npm run test:e2e -- --grep "Attention conflict"
   ```

2. Run the complete GUI gate:

   ```bash
   npm run test:all
   ```

3. Run workspace gates:

   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all -- --check
   ```

### Expected

- The conflict alert stays visible while assignee/version and controls reflect daemon truth.
- The Claim control disappears after another operator wins and focus rests on the listbox.
- Secret-bearing mock text is absent and request ID remains visible.
- Axe reports no serious or critical violation for the alert.
- All unit, browser, build, workspace, clippy, and formatting gates pass.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Shared mutation failure and confirmed reconciliation | PASS | 2026-07-25 | Codex | Four parameterized operations and conflict state passed |
| 2 | Dual failure, explicit retry, success, and dismiss | PASS | 2026-07-25 | Codex | Error lifecycles and unique idempotency keys passed |
| 3 | Safe error and metric privacy boundary | PASS | 2026-07-25 | Codex | Rust and Vitest allowlist checks passed |
| 4 | Two-client version competition and authoritative reread | PASS | 2026-07-25 | Codex | Isolated daemon QA passed 10/10 |
| 5 | Browser alert, focus, accessibility, and full regression | PASS | 2026-07-25 | Codex | 120 Vitest, 32 Playwright, build and workspace gates passed |
