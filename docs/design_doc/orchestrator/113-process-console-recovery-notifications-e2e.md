---
lifecycle: active
related_fr: FR-103
---

# Orchestrator - Process Console Recovery, Attention Notifications, And Live E2E

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-103 recovery convergence, actor-aware Attention notifications, accessibility, and real Tauri/daemon vertical acceptance  
**Related QA**: `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md`  
**Created**: 2026-07-14  
**Last Updated**: 2026-07-14

## Background

The Process Console already exposed Attention, semantic timelines, handoffs, resume plans, sessions, and source provenance, but its final operator journey was split across inconsistent boundaries. A failed task's primary button invoked legacy orphan repair, Attention snapshot and follow filters could disagree about `assignee=me`, and browser tests stopped at an in-page Tauri mock. Native notifications also lacked a daemon-authoritative eligibility and redaction contract.

FR-103 makes reviewed handoff/resume the canonical failed-process recovery path, makes notification policy part of the trusted daemon projection, and proves the complete journey through production Tauri handlers and a live isolated daemon.

## Goals

- Route failed-process recovery through daemon-produced boundaries, consequence plans, and reviewed execution.
- Retain orphaned-running-item repair as an accurately named Expert maintenance action.
- Give snapshots and follow streams identical state, severity, kind, assignee, task, and active-only semantics.
- Notify at most once for each newly opened or reopened actionable item version, without exposing source or failure payloads.
- Preserve an in-app live-region path when native notification permission or delivery is unavailable.
- Prove failure, evidence, handoff, stale rejection, successful resume, audit correlation, and durable Attention resolution through real Tauri and gRPC boundaries.
- Automate keyboard, dialog, contrast, reduced-motion/transparency, narrow-layout, and role-visibility acceptance.

## Non-goals

- Third-party notification delivery, schedules, or routing policy.
- A second frontend recovery engine or workspace rollback implementation.
- Changes to FR-102 session fencing or FR-104 operational metrics.
- A browser-hosted or multi-tenant control plane.

## Scope

- In scope: Attention list/follow filters and delta semantics, safe notification descriptors, Tauri notification delivery/deduplication, Process Workspace action hierarchy, dialog focus behavior, real bridge testing, deterministic fixtures, and accessibility regression.
- Out of scope: outbound Slack/email/mobile notifications, metric dashboards, and provider-specific recovery behavior.

## UI Interactions

- `#/attention`: actor-authoritative Mine filtering, permission-aware notification fallback, stable selection, and safe deep links.
- `#/processes/{task_id}`: failed tasks expose "Review safe resume"; the normal flow selects a logical boundary, creates a consequence preview, requires an operator reason, and executes the reviewed plan.
- Process Expert: "Repair orphaned running items" invokes `TaskRecover` only after copy explains that it repairs crashed-worker residue and does not resume a logical workflow boundary.
- Confirmation and resume dialogs trap focus, close with `Escape`, and restore focus to the actual initiating control.

## Interfaces And Data Changes

- `AttentionListRequest.active_only` is additive.
- `AttentionFollowRequest` now carries the same optional `state`, `kind`, `severity`, `assignee`, `task_id`, and `active_only` filters as the snapshot request.
- `AttentionDelta.notification` is an optional daemon-produced `AttentionNotificationDescriptor` containing only `dedupe_key`, `attention_item_id`, `item_version`, bounded `title`, `severity`, `process_id`, and `deep_link`.
- New items emit an `open` change, resolved items recurring emit `reopen`, and non-transition updates emit `upsert`. Clients still receive only public `upsert`/`remove` reconciliation kinds.
- No database migration was required. Existing `attention_changes`, action audit, handoff, plan, execution, task, and event rows retain authority.
- Process Console Tauri commands explicitly decode their established snake-case frontend arguments. This avoids optional multi-word arguments silently falling back to defaults at the real IPC boundary.

## Key Design

1. Recovery is plan-authoritative. The primary failed-task button can open only `ResumeBoundaryList` → `ResumePlan` → `ResumeExecute`; direct retry/resume actions are suppressed from normal Attention recovery UI.
2. Orphan repair remains distinct. `TaskRecover` is available under Expert with maintenance wording and its existing operator authorization.
3. Filtering is daemon-authoritative. Snapshot and follow use the same matcher and trusted actor. A previously matching item that stops matching emits `remove`, so React does not guess identity or retain stale rows.
4. Notification eligibility is transition-scoped. Only `open`/`reopen` intervention items and configured approval kinds receive a descriptor. Ordinary updates and reconnect replay are quiet.
5. Notification content is allowlisted. The daemon constructs bounded metadata; Tauri never derives notification text from event, prompt, transcript, stdout/stderr, source message, or arbitrary error content.
6. Delivery dedupe is versioned and bounded. Tauri remembers at most 512 `item/version` keys, preserves the deep link separately, and emits an in-app fallback event when permission or platform delivery fails.
7. Acceptance crosses the production seams. A Tauri `MockRuntime` invokes the production command handlers, which connect over gRPC to an isolated daemon running a deterministic shell fixture.

## Alternatives And Tradeoffs

- Exposing a current actor string to React would permit local Mine filtering, but would duplicate authorization identity semantics. Server-side filtering keeps identity trusted and snapshot/follow behavior identical.
- Deriving notifications from every delta would simplify the client but create noise and leak risk. A daemon descriptor adds a protobuf field while making policy reviewable and bounded.
- Reusing `TaskRecover` for all failures would avoid UI work but conflates crashed-worker cleanup with logical re-entry. Two accurately named paths preserve safety and operator understanding.
- A pure browser E2E is faster but cannot detect Tauri argument naming, request-correlation, or real daemon projection failures. The browser suite remains fast coverage; one isolated live script supplies the missing vertical proof.

## Risks And Mitigations

- Risk: reconnect produces duplicate native notifications.
  - Mitigation: notify only transition deltas and dedupe by item/version in a bounded ledger.
- Risk: filtered follow state diverges from snapshot state.
  - Mitigation: both use `attention_filter_matches` with the same trusted actor and full filter set.
- Risk: notification copy leaks operational content.
  - Mitigation: daemon allowlist, bounded title, minimal body, and fixture scans for forbidden fields.
- Risk: dialog overlays become unreachable inside glass stacking contexts.
  - Mitigation: explicit active-dialog stacking and Playwright pointer/focus assertions.
- Risk: live E2E changes developer state or invokes a paid agent.
  - Mitigation: temporary HOME/data/workspace roots, a dedicated port, deterministic `echo`/`exit` agent, bounded polling, and cleanup trap.

## Observability

- Logs and public evidence use process ID, Attention item/version, transition kind, request ID, action name, and terminal result only.
- `control_action_audit` joins `handoff.generate`, `resume.plan`, failed `resume.execute`, and successful `resume.execute` by non-empty request IDs.
- The vertical script asserts handoff precedes successful resume and scans public Attention/audit JSON for prompt, transcript, stdout/stderr, token, and API-key fields.
- In-app `aria-live` announcements cover new safe descriptors and native-notification fallback; product-level metrics remain FR-104.

## Operations / Release

- `RuntimePolicy.spec.attention_inbox_enabled`, `handoff_enabled`, and `mutating_resume_enabled` gate the isolated journey. Elevated resume remains disabled in the closure fixture.
- No schema rollback is needed. Protocol additions are optional/additive; older clients ignore descriptors and continue snapshot/list behavior.
- UI rollback can remove native delivery while retaining in-app Attention. Recovery rollback must not relabel orphan repair as boundary resume.
- Run `scripts/qa/test-process-console-vertical-flow.sh`; `KEEP_QA=1` retains isolated artifacts for diagnosis.

## Test Plan

- Unit: transition kinds, actor-aware Mine/unassigned/active-only matching, update/remove reconciliation, notification eligibility, descriptor bounds, and redaction.
- Integration: real Tauri handler invocation over gRPC, stale-plan error request ID, reviewed execution, durable Attention resolution, and audit ordering.
- E2E: primary recovery routing, no `TaskRecover` bypass, dialog focus trap/restore, read-only controls, axe serious/critical checks, reduced motion/transparency, and narrow layout.
- Regression: GUI unit/E2E/build, workspace tests, strict Clippy, and deterministic isolated script.

## QA Docs

- `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md`

## Acceptance Criteria

- Failed-process primary recovery uses the reviewed handoff/resume flow and never substitutes orphan repair.
- Orphan repair remains reachable and accurately described under Expert.
- New/reopened actionable item versions notify once; ordinary updates and reconnects remain quiet.
- Snapshot and follow filters converge for Mine, unassigned, state, severity, kind, task, and active-only queries.
- Notification denial retains a visible and announced in-app signal.
- The isolated live flow reports five passes and zero failures across failure, evidence, handoff, stale rejection, successful resume, audit, redaction, and Attention resolution.
- Automated accessibility assertions pass for keyboard, focus, role visibility, contrast, motion/transparency, and narrow layout.
- Existing Process Console navigation, Sources, Sessions, System, logs, trace, and New Process remain reachable.
