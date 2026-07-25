---
lifecycle: active
related_fr: FR-100
self_referential_safe: true
---

# Orchestrator - Agent Process Console UI

**Module**: Orchestrator GUI
**Scope**: Information architecture, failed-process vertical flow, keyboard/reconnect behavior, RBAC, responsive fallbacks, and error correlation
**Scenarios**: 5
**Priority**: High

---

## Background

FR-100 reorganizes the Tauri/React client without changing daemon state or public RPC contracts. The fast browser suite installs an in-page mock at the typed Tauri boundary; it never starts a daemon, changes a database, invokes an agent, or performs an external action. FR-103 retains that fast suite and adds the real Tauri/gRPC isolated-daemon vertical proof in QA-150.

Run the deterministic gate:

```bash
./scripts/qa/test-process-console-ui.sh
```

## Scenario 1: Entry Visibility, Default Navigation, And Migration Reachability

### Preconditions

- Frontend dependencies are installed with `cd gui && npm ci`.
- Use the deterministic Playwright Tauri mock; do not connect to a live daemon.

### Goal

Verify the new information architecture is the visible default while every legacy capability has a stable destination.

### Steps

1. Run `cd gui && npm test -- --run src/lib/routes.test.ts`.
2. Run the first Playwright scenario in `gui/tests/e2e/process-console.spec.ts`.
3. Inspect the shell links and activate `Cmd/Ctrl+1..5`, then `Cmd/Ctrl+N`.
4. Open System sections and Process Expert; confirm New Process opens the existing wish/draft flow.

### Expected

- Empty, unknown, and explicit Attention hashes resolve to `#/attention` semantics.
- Attention, Processes, Sessions, Sources, and System have unique active states and copyable stable-ID routes.
- Wish submission remains reachable as New Process; legacy Progress behavior is reachable as Processes.
- Agents, resources, workflows, triggers, stores, secrets, connection/runtime, logs, and raw trace remain reachable under System or Process Expert.

---

## Scenario 2: Keyboard Triage And Stream Reconciliation

### Preconditions

- Scenario 1 dependencies are installed.
- Use the Attention fixtures in the unit/browser suites with at least two stable item IDs.

### Goal

Verify keyboard operation and live updates do not move focus, duplicate records, or apply actions to the wrong Attention item.

### Steps

1. Run `cd gui && npm test -- --run src/pages/AttentionInbox.test.ts`.
2. In the browser fixture, focus the Attention list and use `j`/`k`, arrows, and `Enter`.
3. Reconcile an upsert for the selected stable ID, then a reset snapshot that contains it; repeat with a snapshot that removes it.
4. As operator, activate claim/snooze/resolve and an advertised action; inspect the consequence confirmation before submitting.

### Expected

- Selection is keyed by Attention ID, survives reorder/update/reset when present, and falls back deterministically when removed.
- Inserts and reconnects do not steal DOM focus or duplicate list/timeline rows.
- Shortcuts are ignored while typing in an input.
- Execution-changing actions show a consequence preview and explicit confirmation.

---

## Scenario 3: Failed-Process Semantic Evidence Flow

### Preconditions

- Use the operator-role browser fixture with `attention-1`, `task-1`, and typed test evidence.
- No daemon or external workflow is running.

### Goal

Verify an operator can understand a failed process and reach recovery context without opening raw logs or JSON.

### Steps

1. Run `cd gui && npx playwright test -g "Attention is the default"`.
2. Open the fixture Attention item with `Enter`.
3. Inspect the process header, selected semantic timeline row, evidence panel, handoff/session/source rail, and Expert disclosure.
4. Run `cd gui && npm test -- --run src/components/EvidencePanel.test.tsx`.

### Expected

- The route changes to `#/processes/{task_id}` and preserves the process identity.
- Goal, failed state, workflow, failure summary, and typed test evidence are readable in the normal workspace.
- Raw logs and trace data are not the primary explanation and remain available under Expert.
- The live timeline is deduplicated by stable ID and bounded to 500 entries.

---

## Scenario 4: Role Gates And Safe Session Re-entry

### Preconditions

- Use the same deterministic frontend fixture once per `read_only`, `operator`, and `admin` role.
- Session state is supplied only by the in-page typed Tauri mock.

### Goal

Verify read-only inspection remains useful without exposing focusable or invokable mutations.

### Steps

1. Run `cd gui && npm test -- --run src/hooks/useRole.test.ts`.
2. Run `cd gui && npx playwright test -g "read-only mutations"`.
3. Open the global Sessions list and inspector, then follow its process link.
4. Inspect Attention actions, handoff/resume controls, session input/lease controls, source replay, and System mutations as `read_only`, `operator`, and `admin`.

### Expected

- `read_only` can inspect Attention, timelines, evidence, existing handoffs, transcript, session metadata, and sources.
- Attention mutations, handoff generation, resume, writer/input, replay, and configuration controls require their documented role and are absent or disabled accessibly.
- Sessions is a top-level re-entry surface and its process link opens the integrated Process Workspace.
- Direct unauthorized Tauri/RPC calls remain daemon-rejected; client visibility is not the security boundary.

---

## Scenario 5: Narrow/Fallback Modes, Build, And Error Correlation

### Preconditions

- Chromium for Playwright is installed with `cd gui && npx playwright install chromium`.
- Rust and Node.js toolchains are available; the worktree contains no generated report artifacts.

### Goal

Verify the console remains operable under constrained visual modes and produces supportable, privacy-preserving failures.

### Steps

1. Run `cd gui && npx playwright test -g "narrow layout"`.
2. Run `cd gui && npm run build && npm audit`.
3. Run `cargo test -p orchestrator-gui errors::tests`.
4. Inspect CSS for `prefers-reduced-motion`, `@supports not (backdrop-filter)`, and `[data-transparency="reduced"]` rules.
5. Return an RPC error with `x-request-id: req-qa-147` and confirm the humanized message retains the request ID without response content telemetry.

### Expected

- At 640 px, the labelled menu exposes navigation and panes stack without losing primary controls.
- Reduced transparency persists as an explicit preference; reduced motion and no-backdrop-filter use readable fallbacks.
- Production build, dependency audit, and GUI Rust tests pass.
- Errors retain daemon request IDs for log correlation; UI metrics contain only identifiers, route names, durations, and result codes.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Default navigation and migration reachability | PASS | 2026-07-14 | Codex | Route unit tests and browser default-flow test passed |
| 2 | Keyboard triage and stream reconciliation | PASS | 2026-07-14 | Codex | Stable-ID reducer and keyboard browser assertions passed |
| 3 | Failed-process semantic evidence flow | PASS | 2026-07-14 | Codex | Mocked typed Tauri E2E and EvidencePanel test passed |
| 4 | Role gates and safe session re-entry | PASS | 2026-07-14 | Codex | Role unit test and read-only browser gate passed |
| 5 | Narrow/fallback modes, build, and error correlation | PASS | 2026-07-14 | Codex | Narrow E2E, build, audit, request-ID tests, workspace tests and Clippy passed |
