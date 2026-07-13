# Orchestrator - Agent Process Console Information Architecture

**Module**: Orchestrator GUI
**Status**: Approved
**Related Plan**: FR-100 console information architecture and release hardening
**Related QA**: `docs/qa/orchestrator/147-process-console-ui.md`
**Created**: 2026-07-14
**Last Updated**: 2026-07-14

## Background

The desktop GUI previously organized work around Wish Pool and Progress Observer tabs. The process-console roadmap instead treats human attention as the scarce resource: autonomous work stays out of the default view, while approvals, failures, and blocked decisions lead into an explainable process workspace. FR-095 through FR-099 already provide the typed timeline, Attention, handoff/resume, session, and source contracts consumed by this slice.

## Goals

- Make Attention the default route and provide stable navigation for Attention, Processes, Sessions, Sources, and System.
- Let an operator move from an exception to semantic evidence, handoff, safe resume, or session takeover without reconstructing context from raw logs.
- Preserve New Process, resource administration, task diagnostics, source provenance, and expert views.
- Keep live selection stable across snapshot reloads and bounded stream reconciliation.
- Enforce role-sensitive presentation, keyboard operation, responsive layouts, reduced motion, and transparency fallbacks.

## Non-goals

- Changing the persisted `Task` aggregate or introducing a Process database table.
- Adding or changing daemon, gRPC, or Tauri command contracts.
- Replacing Tauri with a hosted browser application.
- Making the frontend authoritative for actions, leases, cursors, or policy.
- Replacing the project design-token system or removing expert diagnostics.

## Scope

This slice changes the React information architecture, hash routes, screen composition, local presentation state, browser-level test harness, and GUI error correlation. It reuses the existing typed Tauri commands and keeps legacy Wish/Progress components as implementation adapters behind New Process and Processes.

Major touchpoints are `gui/src/App.tsx`, `gui/src/lib/routes.ts`, `gui/src/pages/AttentionInbox.tsx`, `gui/src/pages/ProcessWorkspace.tsx`, `gui/src/pages/SessionList.tsx`, `gui/src/pages/SessionInspector.tsx`, `gui/src/pages/System.tsx`, `gui/src/styles/tokens.css`, and `crates/gui/src/errors.rs`.

## Information Architecture And Routes

The shell uses one stable left navigation. The route parser defaults unknown or empty hashes to Attention and encodes identifiers as path segments.

| Product destination | Hash route | Primary responsibility |
|---|---|---|
| Attention | `#/attention[/<attention-id>]` | Human decisions, approvals, failures, and blockers only |
| Processes | `#/processes[/<task-id>]` | Process list and integrated process workspace |
| Sessions | `#/sessions[/<session-id>]` | Cross-process session inventory and inspector |
| Sources | `#/sources[/<task-id>]` | Source routing plus process provenance |
| System | `#/system[/<section>]` | Agents, resources, triggers, stores, secrets, and runtime |
| New Process | `#/new-process[/<draft-id>]` | Existing wish/draft creation flow as a primary action |

`Cmd/Ctrl+1..5` selects the five stable destinations; `Cmd/Ctrl+N` opens New Process. Hash navigation preserves browser back/forward behavior and supports copyable local deep links without introducing a web-server routing dependency.

## Primary Workspaces

### Attention

Desktop uses filters, an actionable list, and decision context in a three-pane layout. `j`/`k` and arrow keys change the selection by stable Attention ID, `Enter` opens the process, and `c`/`s`/`r` invoke claim, snooze, and resolve only outside text inputs. Execution-changing actions require consequence confirmation. Snapshot or `reset_required` replacement retains the selected ID when it still exists and otherwise selects the first visible item; upserts never move focus.

### Process Workspace

The header presents goal, state, workflow, provenance, active session, and safe next actions. The semantic timeline is the primary explanation surface. Evidence, handoffs, session state, and source binding remain visible in the contextual rail. Raw logs, trace JSON, and the legacy technical task view are retained under Expert. The live timeline buffer is bounded to 500 entries, deduplicated by stable entry ID, and replaced from a fresh cursor after a reset signal.

### Sessions, Sources, And System

Sessions is a global list plus inspector so an operator can re-enter work without first remembering its process. The inspector links back to the process and exposes input only when the current role and writer state allow it. Sources keeps routing/dead-letter operations and opens routed work in the same Process Workspace. System groups the pre-existing expert administration panels so no agent, workflow, trigger, store, secret, connection, or runtime capability is lost.

## Data Ownership And Interfaces

- Pages consume typed Tauri responses; they do not parse raw daemon event names to invent product actions.
- The daemon remains authoritative for RBAC, optimistic versions, idempotency, resume safety, session fencing, and source routing.
- Route state stores stable IDs only. Writer tokens, prompt content, transcripts, source bodies, and action authority are never persisted in local storage.
- Stream hooks own cursors and cancellation. Page reducers reconcile immutable snapshots and bounded deltas by stable ID.
- GUI error humanization preserves daemon `x-request-id` or `request-id` metadata so user-visible failures can be correlated with structured logs.

## Role Presentation

| Capability | `read_only` | `operator` | `admin` |
|---|---:|---:|---:|
| Inspect Attention, timeline, evidence, handoff, sessions, sources | Yes | Yes | Yes |
| Claim/snooze/resolve or execute advertised Attention action | No | Yes | Yes |
| Generate handoff, preview/execute resume, acquire writer control | No | Yes | Yes |
| Replay source events and mutate System configuration | No | No | Yes |

Unavailable mutations are absent where a hidden control would be misleading, or disabled with an accessible reason when the policy boundary is useful context. The daemon still rejects every unauthorized direct invocation.

## Visual And Accessibility Design

- Existing color, type, radius, spacing, focus, and glass tokens remain the source of truth.
- Glass is reserved for page-level framing and decision context; dense rows use quieter surfaces without hover elevation.
- The left navigation collapses behind an explicitly labelled menu on narrow windows; panes stack without horizontal page overflow.
- `prefers-reduced-motion` removes non-essential transitions, while `@supports not (backdrop-filter)` and the user-controlled reduced-transparency mode provide opaque surfaces.
- Status uses text and shape in addition to color. Lists, live regions, alerts, dialogs, and active navigation expose semantic roles and visible focus.
- Read-only users receive no hidden-but-focusable mutation or session-input controls.

## Feature Flags, Observability, And Privacy

Each stable destination is guarded by a build-time `VITE_CONSOLE_<DOMAIN>_ENABLED` flag. An unavailable route renders an explicit safe state; it does not silently redirect into an action surface. Local structured UI metrics cover page-load duration, stream reconnect, timeline render count, and action confirm/cancel/result. Fields are restricted to route names, identifiers, durations, and result codes. Prompt text, evidence content, transcript data, source bodies, and handoff text are excluded.

## Alternatives And Tradeoffs

- Keeping top tabs uses less width but no longer represents five stable operational domains or global sessions.
- Separate evidence/handoff/session pages simplify individual screens but force operators to lose the selected failure context while deciding.
- A frontend action registry would reduce server calls but risks policy and provider drift; server-advertised actions stay authoritative.
- Unbounded live history avoids pagination transitions but leaks memory during long sessions; a bounded buffer plus cursor reload is predictable.

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Live updates move the selected decision | Stable-ID reconciliation; no focus changes on upsert |
| Dense process pages become unreadable | Semantic timeline first, contextual rail, Expert progressive disclosure |
| UI role rules drift from daemon policy | Shared role ordering in the client plus mandatory daemon enforcement |
| Existing Wish/Progress users lose entry points | New Process and Processes reuse the accepted flows and typed commands |
| Blur or animation harms accessibility/performance | Opaque fallback, reduced-transparency preference, reduced-motion CSS |
| A rollout regression blocks all work | Per-domain feature flags and retained legacy components |

## Operations, Compatibility, And Rollback

The change is frontend-only and requires no schema migration or task/session data rewrite. Existing CLI, gRPC, Tauri commands, stored tasks, sessions, Attention items, handoffs, and source bindings are unchanged. Roll out domains independently with the `VITE_CONSOLE_*_ENABLED` variables. To roll back, disable the affected domain at build time or deploy the prior GUI bundle; the additive hash routes and local presentation preferences may remain. New Process and legacy expert components are retained during the migration.

## Test Plan

- Unit/component: route parsing/formatting, role ordering, Attention stable selection/reconciliation, and semantic evidence rendering.
- Browser E2E: mocked typed Tauri boundary for default Attention, failed-process evidence flow, read-only mutation gates, narrow navigation, transparency fallback, and Sessions reachability.
- Rust: request-ID preservation in GUI error humanization.
- Build/security: TypeScript/Vite production build and dependency audit.
- Regression: `cargo test --workspace` and workspace Clippy gate verify the frontend reorganization does not break the control plane.

## QA Docs

- `docs/qa/orchestrator/147-process-console-ui.md`
- `scripts/qa/test-process-console-ui.sh`

## Acceptance Criteria

- Attention is the default and ordinary autonomous tasks do not appear in its actionable list.
- A failed process opens a semantic timeline and evidence without requiring raw logs or expert JSON.
- Keyboard selection remains stable through update/reset reconciliation and execution-changing actions require confirmation.
- Read-only users can inspect but cannot invoke Attention, resume, handoff-generation, writer, replay, or System mutations.
- Narrow, reduced-motion, no-backdrop-filter, light/dark, and reduced-transparency modes retain readable navigation and controls.
- New Process plus all pre-existing resource, task, session, source, connection, and expert capabilities remain reachable.
