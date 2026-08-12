---
lifecycle: active
related_fr: FR-103
self_referential_safe: true
---

# Orchestrator - Process Console Recovery, Attention Notifications, And Live E2E

**Module**: Orchestrator  
**Scope**: Reviewed recovery routing, actor-aware Attention follow, safe native notifications, real Tauri/gRPC vertical proof, audit redaction, and accessibility  
**Scenarios**: 5  
**Priority**: High

---

## Background

This document is the executable closure evidence for FR-103. The live workflow uses only the deterministic fixture below; it must not use a workflow under `docs/workflow/` or a paid AI agent.

```bash
cargo build -p orchestratord -p orchestrator-cli
orchestrator apply \
  -f fixtures/manifests/bundles/process-console-vertical-flow.yaml \
  --project qa-process-console-vertical
./scripts/qa/test-process-console-vertical-flow.sh
```

The script performs the apply inside a temporary HOME/data/workspace environment on `127.0.0.1:19103`, invokes production Tauri handlers, and removes the environment unless `KEEP_QA=1` is set.

---

## Scenario 1: Authoritative Attention Filters And Notification Transitions

### Preconditions

- Rust dependencies are available.
- No daemon or external notification provider is required.

### Goal

Verify snapshot/follow matching shares trusted actor semantics and only new or reopened actionable versions are notification-eligible.

### Steps

1. Run `cargo test -p agent-orchestrator attention::tests`.
2. Run `cargo test -p orchestratord server::attention::tests::descriptor_is_bounded_allowlisted_and_transition_scoped`.
3. Run `cd gui && npm test -- --run src/pages/AttentionInbox.test.ts`.
4. Inspect follow request construction for state, kind, severity, assignee, task, and active-only parity with list.

### Expected

- Mine matches only the trusted current actor; unassigned and active-only semantics are stable.
- New rows use `open`, resolved recurrences use `reopen`, and ordinary changes use `upsert`.
- A row leaving the current filter emits a removal rather than remaining stale in the client.
- Only open/reopen intervention or configured approval transitions have a notification descriptor.
- Descriptor fields are bounded and allowlisted; ordinary updates have no notification.

---

## Scenario 2: Canonical Recovery UI And Automated Accessibility

### Preconditions

- Install frontend dependencies with `cd gui && npm ci`.
- Install Chromium with `cd gui && npx playwright install chromium` when absent.
- Use only the in-page deterministic Tauri mock for this fast UI scenario.

### Goal

Verify a failed process enters reviewed recovery, never routes its primary action to orphan repair, and remains operable across keyboard, roles, visual fallbacks, and narrow layouts.

### Entry Visibility

The recovery entry must be visible from Attention → Process Workspace and as the failed-process "Review safe resume" primary action. A direct hash, Expert-only control, or inline retry action is not an acceptable substitute.

### Steps

1. Run `cd gui && npm run test:e2e`.
2. Open the failed fixture process and select "Review safe resume".
3. Create a preview, enter an operator reason, execute the reviewed plan, and inspect captured Tauri command names.
4. Exercise `Tab`/`Shift+Tab`, `Escape`, read-only mode, reduced motion/transparency, and the 640 px layout.
5. Run the axe assertion on Attention and the failed Process Workspace.

### Expected

- The primary path calls `resume_boundary_list`, `resume_plan`, and `resume_execute`; it never calls `task_recover`.
- "Repair orphaned running items" exists only under Expert with maintenance semantics.
- Dialog focus is trapped and returns to the actual initiating button.
- Read-only users have no focusable recovery or session mutations.
- Axe reports no serious/critical violations; key text colors meet WCAG AA.
- Reduced motion, reduced transparency, and narrow navigation remain functional.

---

## Scenario 3: Live Failure To Durable Reviewed Resume

### Preconditions

- Build debug binaries with `cargo build -p orchestratord -p orchestrator-cli`.
- The script will run this exact public-interface apply in isolation:

  ```bash
  orchestrator apply \
    -f fixtures/manifests/bundles/process-console-vertical-flow.yaml \
    --project qa-process-console-vertical
  ```

- Do not export a developer or production `ORCHESTRATORD_DATA_DIR` into the script.

### Goal

Verify the complete failure → Attention → evidence → handoff → reviewed resume → resolved Attention journey through production Tauri handlers and live gRPC.

### Steps

1. Run `./scripts/qa/test-process-console-vertical-flow.sh`.
2. Confirm the live bridge creates and starts the deterministic failing task.
3. Confirm TaskInfo and semantic Timeline expose the failed state and typed test evidence.
4. Confirm handoff generation, boundary selection, one intentionally stale execution rejection, a fresh plan, and successful reviewed execution.
5. Confirm the source Attention item is later queryable as `resolved`.

### Expected

- Production Tauri command handlers communicate with the isolated daemon over gRPC.
- The deterministic shell agent fails without invoking any AI provider.
- The stale state version fails before mutation and retains a request ID in the humanized Tauri error.
- A reviewed fresh plan creates/enqueues a correlated child and succeeds.
- Attention resolves only after the durable resume state-change event.

---

## Scenario 4: Audit Ordering, Correlation, And Notification Privacy

### Preconditions

- Apply `fixtures/manifests/bundles/process-console-vertical-flow.yaml` through the isolated script as described in Scenario 3.
- Scenario 3 has completed successfully.

### Goal

Verify the cross-boundary recovery attempt is supportable without exposing raw operational content.

### Steps

1. Inspect the script's isolated `audit list --project qa-process-console-vertical -o json` assertion.
2. Confirm successful `handoff.generate`, at least two successful `resume.plan` rows, one failed `resume.execute`, and one successful `resume.execute`.
3. Confirm every resume execution has a non-empty `request_id` and successful handoff time precedes successful execution time.
4. Inspect resolved Attention and audit JSON with the script's forbidden-field scan.
5. Inspect `AttentionDelta.notification` fields in the protobuf and Tauri mapping.

### Expected

- Handoff review is durably recorded before successful resume.
- Rejected and accepted execution attempts remain independently correlated.
- Public Attention, audit, and notification evidence excludes prompt, transcript, stdout/stderr, tokens, API keys, source messages, and raw error bodies.
- Notification text is daemon-produced from bounded title, severity, process ID, and deep link only.

---

## Scenario 5: Full Regression And Reachability Gate

### Preconditions

- Rust and Node.js toolchains are available.
- The worktree contains no retained `test-results` or live QA directories.

### Goal

Verify FR-103 preserves the rest of the Process Console and repository quality gates.

### Steps

1. Run `cargo test --workspace`.
2. Run `cargo clippy --workspace --all-targets -- -D warnings`.
3. Run `cd gui && npm run test:all`.
4. Verify Attention, Tasks, Sessions, Sources, System, and New task through the shell/browser tests.
5. Verify Process Expert still exposes raw logs, trace, and orphan repair.

### Expected

- Workspace tests and strict Clippy pass.
- GUI unit tests, 15 Playwright scenarios, TypeScript, and production Vite build pass.
- Tauri snake-case argument contracts match the existing frontend calls at the real IPC boundary.
- Existing navigation, logs, trace, Sources, Sessions, System, and New Process remain reachable.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Authoritative Attention filters and notification transitions | PASS | 2026-07-14 | Codex | Core/daemon/React transition, actor, descriptor, and removal tests passed |
| 2 | Canonical recovery UI and automated accessibility | PASS | 2026-07-16 | Codex | 15 Playwright scenarios passed, including no TaskRecover bypass, focus, axe, motion, transparency, narrow, roles, Sources, navigation, and Attention mutations |
| 3 | Live failure to durable reviewed resume | PASS | 2026-07-14 | Codex | Real Tauri/gRPC isolated flow passed from deterministic failure through resolved Attention |
| 4 | Audit ordering, correlation, and notification privacy | PASS | 2026-07-14 | Codex | Request IDs, stale rejection, handoff-before-resume order, and forbidden-field scan passed |
| 5 | Full regression and reachability gate | PASS | 2026-07-14 | Codex | Workspace tests, strict Clippy, GUI test:all, build, and all primary destinations passed |
