---
lifecycle: active
self_referential_safe: true
---

# Orchestrator - Process Console Functional And UI Regression

**Module**: Orchestrator GUI
**Scope**: Primary destination reachability, process/Attention behavior, timeline and handoff safety, Sources authorization, Sessions, accessibility, and source-wide frontend coverage
**Scenarios**: 5
**Priority**: High

---

## Background

This suite expands the fast Process Console regression without connecting to a live daemon or external agent. Browser scenarios install deterministic typed Tauri fixtures before page load. Production Tauri/gRPC and durable mutation behavior remain covered by `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md`.

Run the complete fast gate:

```bash
./scripts/qa/test-process-console-ui.sh
```

## Scenario 1: Entry Visibility, Preferences, And Process Prioritization

### Preconditions

- Install dependencies with `cd gui && npm ci`.
- Install Chromium with `cd gui && npx playwright install chromium` when absent.
- Do not connect the browser fixture to a live daemon.

### Goal

Verify every primary destination is discoverable and active work remains keyboard reachable in operational priority order.

### Steps

1. Run `cd gui && npx playwright test -g "Processes prioritizes|global shortcuts"`.
2. Verify visible links for "Attention", "Tasks", "Sessions", "Sources", and "System".
3. Activate `Cmd/Ctrl+1..5`, toggle the theme, and open a process row with `Enter`.
4. Inspect ordering for running, failed, and completed process fixtures.

### Expected

- Five visible navigation entries reach stable hash routes.
- Theme state changes through the labelled control and persists in the document theme attribute.
- Running work precedes failed work, which precedes completed work.
- Process rows expose accessible names and open with keyboard activation.

## Scenario 2: Attention Mutation Closure And Semantic Process Evidence

### Preconditions

- Use the operator fixture with two open Attention items and one failed process.

### Goal

Verify an operator can claim and resolve work safely, then understand the linked failure without relying on raw logs.

### Steps

1. Run `cd gui && npx playwright test -g "Attention mutations|Attention is the default"`.
2. Activate "Claim", then "Resolve" and confirm through "Resolve item".
3. Inspect the captured Tauri calls for stable ID, expected version, and non-empty idempotency key.
4. Open the remaining failed process and inspect timeline and evidence.

### Expected

- Claim and resolve use guarded commands; resolved work leaves the default open queue.
- Selection moves deterministically to the remaining item.
- The linked Process Workspace shows goal/state, failure summary, checkpoint, and typed test evidence.
- Raw trace/log controls remain secondary under Expert.

## Scenario 3: Timeline Resilience And Risk-aware Handoff

### Preconditions

- Use deterministic Vitest mocks only; no workflow or provider is started.

### Goal

Verify reconnected timeline streams do not duplicate evidence and risky resume cannot execute without explicit review.

### Steps

1. Run `cd gui && npm test -- --run src/hooks/useTimeline.test.tsx src/components/HandoffPanel.test.tsx`.
2. Exercise initial snapshot, durable follow cursor, stable-ID upsert, opaque pagination, reset-required refresh, and stream error.
3. Generate a handoff and inspect current state, changed files, recommendation, and bounded snapshot hash.
4. Preview an external-side-effect boundary; enter an operator reason and select the elevated confirmation before "Execute reviewed plan".

### Expected

- Stable timeline IDs are updated rather than duplicated; pagination retains earlier entries.
- Reset replaces the local snapshot authoritatively, while follow errors preserve readable evidence.
- Provider-session mode appears only when the selected boundary advertises availability.
- Risky execution remains disabled until both reason and elevated confirmation are present.

## Scenario 4: Sources Correlation, Filtering, And Role Boundary

### Preconditions

- Run once with `read_only` and once with `admin` typed browser fixtures.

### Goal

Verify source events remain inspectable and correlated while replay is limited to actionable failures and the admin role.

### Steps

1. Run `cd gui && npm test -- --run src/pages/Sources.test.tsx`.
2. Run `cd gui && npx playwright test -g "Sources supports|read-only Sources"`.
3. Filter to `needs_attention`, activate "重新路由" as admin, then activate "打开进程".
4. Repeat as read-only and inspect focusable controls.

### Expected

- Routing filters are sent to the authoritative list command and reduce the visible list.
- Admin replay calls `source_replay` only for the selected actionable event.
- Read-only users retain provider, installation, routing state, error, and process correlation but have no replay control.
- The correlated task opens in the integrated Process Workspace.

## Scenario 5: Sessions, Accessibility, Responsive Fallbacks, Coverage, And Build

### Preconditions

- Node.js and Chromium are installed.
- Generated `gui/coverage/`, Playwright traces, and reports are not committed.

### Goal

Verify re-entry, constrained visual modes, automated accessibility, source-wide measurement, and the production build remain healthy.

### Steps

1. Run `cd gui && npm run test:coverage`.
2. Run `cd gui && npm run test:e2e`.
3. Run `cd gui && npm run build && npm audit`.
4. Inspect the Session reader offset/reconnect, single-writer controls, linked process, read-only state, axe checks, reduced motion/transparency, and 640 px navigation scenarios.

### Expected

- 120 Vitest scenarios and 32 Playwright journeys pass.
- Coverage collects all eligible runtime TypeScript/TSX files and writes only ignored local output.
- Session output is offset-deduplicated; writer controls remain role-gated and the process link is reachable.
- Attention, Process Workspace, and Operations have no serious/critical axe violations.
- Reduced motion/transparency and narrow navigation remain functional; TypeScript/Vite build and audit pass.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Entry visibility, preferences, and process prioritization | PASS | 2026-07-16 | Codex | Playwright validates five entries, shortcuts, theme, ordering, and keyboard open |
| 2 | Attention mutation closure and semantic process evidence | PASS | 2026-07-16 | Codex | Guarded claim/resolve and linked evidence journey pass |
| 3 | Timeline resilience and risk-aware handoff | PASS | 2026-07-16 | Codex | Snapshot/follow/reset/pagination and elevated replay gates pass |
| 4 | Sources correlation, filtering, and role boundary | PASS | 2026-07-16 | Codex | Unit and browser tests cover admin replay and read-only inspection |
| 5 | Sessions, accessibility, responsive fallbacks, coverage, and build | PASS | 2026-07-25 | Codex | 120 Vitest and 32 Playwright scenarios pass; source-wide line coverage is 89.21% and production build passed |
