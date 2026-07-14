# FR-103: Process Console Recovery, Attention Notifications, And Live E2E

## 优先级: P1

## 状态: Proposed

## 依赖: FR-096 through FR-100 closure artifacts, FR-101, FR-102

## 计划闭环产物

- `docs/design_doc/orchestrator/113-process-console-recovery-notifications-e2e.md`
- `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md`
- `scripts/qa/test-process-console-vertical-flow.sh`

## Background

The Process Console shell, Attention Inbox, semantic timeline, handoff/resume panel, global Sessions, Sources, and System navigation exist. The current browser suite uses an in-page Tauri mock and proves navigation/evidence presentation, but it does not execute the roadmap's complete vertical outcome against a live isolated daemon.

Three interaction gaps remain: failed processes still expose a misleading legacy `TaskRecover` action alongside safe resume, Attention deltas do not create native intervention notifications, and stream-side `Mine` filtering cannot reliably compare the assignee to the current actor. These issues should be closed together because they affect the same operator journey from interruption to safe continuation.

## Goals

- Make Handoff/ResumePlan/ResumeExecute the canonical failed-process recovery path.
- Preserve orphaned-running-item repair as an accurately named Expert/maintenance action, not a substitute for safe resume.
- Deliver privacy-safe native notifications for newly actionable intervention/approval items with deduplication and permission-aware fallback.
- Reconcile `Mine` and other Attention filters correctly for both snapshots and stream deltas.
- Prove the full failed-process vertical flow through the real Tauri command boundary and an isolated daemon.
- Add automated accessibility checks for keyboard triage, dialogs, role-hidden controls, live regions, and narrow layouts.

## Non-goals

- Replacing Tauri with a browser-hosted control plane.
- Sending outbound Slack/email/mobile notifications.
- Inventing a second recovery service in the frontend.
- Changing session fencing semantics owned by FR-102.
- Building operational metric dashboards owned by FR-104.

## Scope

### In scope

- Process Workspace recovery labels, action hierarchy, consequence preview, and safe-resume result handling.
- Expert placement and accurate semantics for `TaskRecover` orphan repair.
- Attention notification policy for new/reopened `intervention` and configured approval items.
- Notification dedupe, quiet behavior for updates to already-visible items, permission denial, and content redaction.
- Actor-aware Attention snapshot/delta filtering and stable selected-ID behavior.
- Live integration fixture: failure materialization, semantic evidence, handoff generation, resume execution or session attach, and post-state-change Attention resolution.
- Playwright/Tauri driver or equivalent test harness using real daemon RPCs rather than an in-page RPC mock for the vertical scenario.

### Out of scope

- General notification routing rules, schedules, or third-party delivery providers.
- UI redesign outside the Process Console journey.
- A hosted multi-user identity layer.

## Interfaces And Data Changes

Prefer existing typed APIs. An additive Attention notification descriptor may be introduced only if severity alone cannot express daemon-authoritative notification policy. The frontend must not derive notification text from raw event payloads.

Expose the current trusted actor identifier to the frontend through an existing safe identity/probe response or make the daemon apply stream filters, so `assignee=me` has identical snapshot and delta semantics.

The live E2E harness must create a temporary project/workspace/daemon and deterministic shell agents. It may seed safe fixture events through public interfaces but must not mutate the repository's active database or invoke a paid AI agent.

## Key Design Constraints

- Recovery mutations always originate from daemon-produced boundaries and plans.
- A failed-process primary action cannot call `TaskRecover` while describing boundary resume.
- Notifications contain bounded title, severity, process identifier, and deep link only; no prompt, error body, transcript, source message, or secret.
- Reconnect/deduplication does not re-notify the same item version.
- Native-notification denial leaves an in-app `aria-live`/badge path without blocking Attention.
- Read-only users may inspect and open processes but cannot focus or invoke recovery/session mutations.

## Acceptance Criteria

- [ ] Failed-process recovery opens the handoff/consequence flow and cannot directly invoke task retry/resume/recover as a boundary-resume substitute.
- [ ] Orphan repair remains reachable under Expert/System with accurate wording and its existing authorization.
- [ ] A new or reopened intervention item emits at most one privacy-safe native notification per item version; reconnect and ordinary updates do not duplicate it.
- [ ] Notification permission denial and unsupported platforms retain a clear in-app Attention signal.
- [ ] `Mine`, unassigned, severity, and state filters produce the same result for snapshots and subsequent stream deltas.
- [ ] A live isolated-daemon E2E completes: failure → Attention → Process Workspace/evidence → handoff → reviewed resume or session takeover → durable state change → Attention resolved.
- [ ] The live E2E traverses real gRPC and Tauri command handlers; the pure browser mock remains only for fast component tests.
- [ ] Keyboard-only, focus trap/restore, reduced motion/transparency, narrow layout, and role visibility pass automated accessibility assertions.
- [ ] Existing New Process, logs, trace, Sources, Sessions, and System capabilities remain reachable.

## QA Plan

- Unit tests for notification eligibility/dedupe, actor-aware delta filters, recovery action routing, and redaction.
- Browser tests with accessibility scanning for operator/read-only and desktop/narrow modes.
- An isolated live-daemon script drives the deterministic failed-process fixture and verifies the durable Attention/resume audit order.
- Tauri integration coverage proves request IDs and error states survive the real command bridge.

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Native notifications become noisy | Notify only new/reopened configured actionable versions; dedupe by item/version |
| Live GUI E2E is platform-flaky | Separate deterministic daemon fixture from a minimal Tauri bridge driver and use bounded readiness checks |
| Removing recovery button hides orphan repair | Relocate it to Expert with explicit orphan-repair wording |
| Deep links open stale context | Stable IDs plus authoritative reload and a clear resolved/stale state |
| Notification text leaks sensitive failure details | Daemon-safe descriptor and strict field allowlist |
