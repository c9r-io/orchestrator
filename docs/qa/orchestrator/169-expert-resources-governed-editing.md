---
lifecycle: active
related_fr: FR-119
self_referential_safe: true
---

# Orchestrator GUI - Expert Resources Governed Editing

**Module**: Orchestrator Core / Daemon / GUI
**Scope**: Five typed resource catalogs, accessible detail navigation, role-gated reviewed Apply, optimistic conflicts, audit, and privacy
**Scenarios**: 5
**Priority**: High

## Automated Entry Point

Build the debug binaries, then run the isolated vertical gate:

```bash
cargo build -p orchestratord -p orchestrator-cli -p orchestrator-gui
./scripts/qa/test-expert-resources-governed-editing.sh
```

The script creates disposable HOME/data/workspace directories, starts a UDS daemon with webhooks disabled, applies deterministic non-executing resource manifests, crosses the real Tauri command bridge, inspects SQLite audit evidence, and removes the environment.

---

## Scenario 1: Typed Catalogs And Canonical Describe

### Preconditions

- Rust, Cargo, `jq`, `rg`, and `sqlite3` are installed.
- No production daemon or database is used.

### Goal

Verify all five Expert resource types return stable structured summaries and canonical apply-compatible manifests.

### Steps

1. Run `cargo test -p agent-orchestrator service::resource::tests`.
2. Run `cargo test -p orchestrator-integration-tests --test grpc_compat apply_get_describe_roundtrip -- --exact`.
3. Inspect Workspace, Workflow, Agent, StepTemplate, and ExecutionProfile summaries.
4. Compare a Workspace catalog revision with the revision returned by Describe.

### Expected

- Every summary contains `kind`, `name`, `project_id`, a 64-character revision, and source metadata.
- Names are deterministically sorted and bounded pagination returns a continuation cursor.
- Describe returns one manifest with `apiVersion`, `kind`, `metadata`, and `spec`, without ResourceStore generation/timestamp fields.
- Catalog, Describe, and current-resource revisions match.

---

## Scenario 2: Entry Visibility, Read-only Navigation, Copy, And Accessibility

### Preconditions

- Install frontend dependencies with `cd gui && npm ci`.
- Install Chromium with `cd gui && npx playwright install chromium` when absent.

### Goal

Verify read-only users can navigate all resource catalogs and inspect/copy a resource without receiving mutation controls.

### Steps

1. Open a Task detail, expand "Expert", and activate the "Resources" tab.
2. Run `cd gui && npm test -- --run src/components/ExpertResources.test.tsx`.
3. Run `cd gui && npx playwright test -g "read-only Resources"`.
4. Enter a resource row with the keyboard, inspect the canonical detail, activate "Copy", then return.
5. Switch across all five type controls and inspect the Axe result.

### Expected

- Loading, empty, failure, and populated list states do not reuse stale rows.
- Enter/Space opens detail; returning restores focus to the selected row.
- "Edit" and Apply controls are absent for read-only users.
- No serious or critical accessibility violations are reported.

---

## Scenario 3: Reviewed Operator Apply And Authoritative Reload

### Preconditions

- Use the Operator frontend fixture and the isolated daemon script.

### Goal

Prove a reviewed edit crosses the real Tauri/gRPC boundary, is audited, and reloads daemon-authoritative content.

### Steps

1. Run `cd gui && npx playwright test -g "operator reviews"`.
2. Open "Edit", change the YAML, activate "Review changes", and enter an audit reason.
3. Verify the dialog identifies the kind/name and project before activating "Apply changes".
4. Run `./scripts/qa/test-expert-resources-governed-editing.sh`.

### Expected

- Apply sends the selected revision, project, non-empty reason, and idempotency key.
- Escape closes the review dialog and returns focus; confirmed Apply succeeds.
- The detail is refreshed through Describe and displays the returned request ID.
- The real daemon records one succeeded `resource.apply` for `Workspace/fr119-workspace`.

---

## Scenario 4: Validation And Concurrent-edit Recovery

### Preconditions

- Use deterministic Vitest fixtures for validation and stale-revision responses.
- Keep the isolated daemon gate available for a real stale revision.

### Goal

Verify invalid or stale edits fail closed without losing the user's draft.

### Steps

1. Run the validation and conflict cases in `src/components/ExpertResources.test.tsx`.
2. Submit invalid YAML and confirm the daemon validation error.
3. Submit one valid revision, then submit the same original revision again.
4. Inspect the editor and authoritative detail after both failures.

### Expected

- Validation errors preserve the exact draft for correction.
- A stale revision returns conflict semantics and a request ID rather than overwriting current state.
- Conflict refreshes authoritative content/revision independently and preserves the draft for reconciliation.
- The real daemon records the stale Apply as failed.

---

## Scenario 5: Audit Privacy And Repository Regression

### Preconditions

- Scenarios 1–4 pass.

### Goal

Verify manifest content does not leak and the additive interfaces do not regress the workspace.

### Steps

1. Let the isolated script submit content containing `qa-resource-sensitive-marker`.
2. Search audit JSON, daemon logs, Tauri output, and `control_action_audit` public columns.
3. Run `cargo fmt --all -- --check`.
4. Run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cd gui && npm run test:all`, and `./scripts/qa-doc-lint.sh`.

### Expected

- The sentinel is absent from logs, UI errors, audit output, and persisted public evidence.
- Succeeded, stale, and invalid mutations retain status and request-ID evidence.
- Formatting, Rust tests, strict Clippy, Vitest, Playwright, frontend build, and documentation lint pass.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Typed catalogs and canonical Describe | PASS | 2026-07-25 | Codex | Five projections, pagination, gRPC summary, and revision parity verified |
| 2 | Entry visibility, read-only navigation, and accessibility | PASS | 2026-07-25 | Codex | Vitest and Playwright cover tab reachability, keyboard, copy, focus return, and Axe |
| 3 | Reviewed Apply and authoritative reload | PASS | 2026-07-25 | Codex | Real Tauri/gRPC Apply and request-ID evidence pass |
| 4 | Validation and conflict recovery | PASS | 2026-07-25 | Codex | Draft retention and real stale revision rejection pass |
| 5 | Audit privacy and repository regression | PASS | 2026-07-25 | Codex | Workspace tests, strict Clippy, 120 Vitest, 32 Playwright, production build, isolated QA, and doc lint pass |
