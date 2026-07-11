# FR-100: Agent Process Console UI

## 优先级: P1

## 状态: Proposed

## 依赖: FR-095 through FR-099

## 计划闭环产物

- `docs/design_doc/orchestrator/110-process-console-information-architecture.md`
- `docs/qa/orchestrator/147-process-console-ui.md`

## Background

The current Tauri/React GUI uses two top-level views, Wish Pool and Progress Observer, with task details and an expert panel. The next product model centers on actionable attention, explainable process history, and safe takeover. The navigation and screen hierarchy must express that operating model without discarding current resource, agent, trigger, store, secret, and system capabilities.

The existing design system recommends Liquid Glass surfaces with explicit contrast, focus, theme, and no-backdrop-filter fallbacks. Dense operational screens must apply those tokens selectively rather than rendering every row as a large animated card.

## Goals

- Make Attention the default landing view.
- Provide coherent navigation for Attention, Processes, Sessions, Sources, and System.
- Present timeline, evidence, actions, handoff, and session controls in one process workspace.
- Preserve expert functionality and role-based visibility.
- Support keyboard-first triage, responsive layouts, accessible contrast, reduced motion, and blur fallbacks.
- Keep frontend types aligned with stable gRPC/Tauri product APIs rather than raw event payloads.

## Non-goals

- Replacing Tauri with a browser deployment in this request.
- Building a generic drag-and-drop Kanban board.
- Exposing raw YAML or logs as the primary non-expert experience.
- Redesigning the entire token system.
- Hiding technical evidence from expert users.

## Scope

### In scope

- Navigation, routes, page layouts, component boundaries, interaction states, keyboard model, notification behavior, accessibility, responsive behavior, and frontend data ownership.
- Migration of existing wish/progress/expert views into the new hierarchy.

### Out of scope

- Backend contracts proposed by FR-095 through FR-099.
- Web hosting, multi-tenant identity, and mobile-native applications.

## Interfaces and Data Changes

This request adds no direct database schema. It consumes typed Tauri commands corresponding to the gRPC contracts proposed by FR-095 through FR-099:

- timeline snapshot/follow and evidence retrieval;
- attention list/follow and versioned mutations;
- handoff generation/get plus resume plan/execute;
- session list/get/read/attach/lease/input/detach/close;
- source binding and routing-state reads.

Frontend route state uses stable IDs and cursors. It must not persist provider tokens, writer fencing tokens beyond the active client session, raw secret-bearing payloads, or authoritative action state in local storage.

## Information Architecture

```text
Attention
  Inbox
  Attention detail -> Process workspace

Processes
  Process list
  Process workspace
    Overview
    Timeline
    Evidence
    Handoffs
    Sessions
    Expert

Sessions
  Active/detached session list
  Session inspector

Sources
  Source bindings
  Routing/dead-letter status

System
  Agents
  Workflows/resources
  Triggers
  Stores/secrets
  Runtime/connection
```

Wish submission remains available as a primary “New process” action rather than a permanent top-level product silo. Existing Progress Observer functionality becomes the Processes list/workspace.

## Primary Screens

### Attention Inbox

Desktop uses a three-pane layout:

```text
filters | actionable item list | decision context and actions
```

The list shows severity, kind, process, concise requested decision, age/SLA, assignee, and available primary action. Resolved history is a filter, not mixed into the default open queue.

Keyboard behavior:

- `j/k` or arrow keys move selection.
- `Enter` opens the process workspace.
- `c` claims, `s` snoozes, and `r` resolves only when focus is not in an input.
- Destructive or execution-changing actions always show a consequence preview and explicit confirmation.

### Process workspace

The header shows goal, current state, source provenance, workflow, active session, open attention count, and primary safe action. The main area is the semantic timeline. A contextual rail shows evidence, handoff versions, resume choices, and session controls for the selected entry.

Raw logs and trace JSON remain accessible under Expert; they are not the default explanation.

### Session inspector

The inspector displays state, task/step linkage, agent, working directory label, transcript, reader/writer state, lease expiry, and input controls. Writer acquisition is explicit. Read-only viewing never displays an enabled input box.

## Component Boundaries

Suggested additions:

```text
gui/src/pages/AttentionInbox.tsx
gui/src/pages/ProcessList.tsx
gui/src/pages/ProcessWorkspace.tsx
gui/src/pages/SessionList.tsx
gui/src/pages/SessionInspector.tsx
gui/src/pages/Sources.tsx

gui/src/components/AttentionList.tsx
gui/src/components/AttentionDecisionPanel.tsx
gui/src/components/ProcessTimeline.tsx
gui/src/components/TimelineEntry.tsx
gui/src/components/EvidencePanel.tsx
gui/src/components/HandoffPanel.tsx
gui/src/components/ResumePlanDialog.tsx
gui/src/components/SessionTranscript.tsx
gui/src/components/WriterLeaseControl.tsx
```

Frontend state should use query-scoped hooks with explicit snapshot cursors and stream reconciliation. Page components must not interpret raw event names or construct backend action IDs.

## Visual and Interaction Design

- Use existing background, text, accent, danger, glass, spacing, and radius tokens.
- Reserve full Liquid Glass cards for page-level panels and decision context. Timeline rows and inbox rows use denser opaque or lightly translucent surfaces.
- Disable hover elevation on large scroll lists to avoid visual movement and rendering cost.
- Provide `@supports not (backdrop-filter)` opaque fallbacks and a reduced-transparency mode.
- Use icon plus text plus shape/status, never color alone.
- Preserve visible focus rings and WCAG-compatible contrast in both themes.
- Respect `prefers-reduced-motion`; live inserts do not move keyboard focus.
- Live updates announce concise status through a polite ARIA region, while urgent intervention uses an explicit notification.

## Role and Action Presentation

- `read_only`: inspect attention, timelines, evidence, handoffs, and reader session streams.
- `operator`: claim/resolve attention, execute approved actions, generate handoffs, resume, and request writer leases.
- `admin`: configuration, source installation, resource mutation, and policy controls.

Hidden actions must also be rejected by the daemon. Disabled actions include an accessible reason when visibility helps explain policy.

## Key Design

- A left navigation replaces two top tabs because five stable operational domains now exist. On narrow windows it collapses to icons plus labels in a menu.
- The process workspace combines observation and action so operators do not lose selected evidence while deciding. High-risk actions still use modal consequence previews.
- “New process” remains prominent but no longer defines the whole product as a wish pool.
- Expert capabilities are retained under System and Process Expert rather than mixed into default triage.

## Tradeoffs

- A left navigation consumes more width than the current two tabs but matches the expanded stable domain model.
- Three-pane triage improves context retention on desktop but requires a stacked drill-down layout on narrow windows.
- Server-provided actions reduce frontend autonomy but prevent policy drift and provider-specific UI branching.
- Selective glass styling is less visually uniform than glass cards everywhere, but improves scanability, contrast, and rendering performance.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Dense screens become visually noisy | Strong hierarchy, bounded summaries, progressive disclosure |
| Live updates disrupt selection | Stable IDs, no focus stealing, snapshot reconciliation |
| Glass effects reduce readability/performance | Selective use, opaque fallback, reduced transparency |
| UI action availability diverges from daemon policy | Server-provided action descriptors and server enforcement |
| Existing users lose Wish/Progress workflows | Explicit migration mapping and preserved New Process action |
| Large timelines consume memory | Windowing, cursor pagination, and bounded live buffers |

## Observability and Operations

- Client metrics: page load duration, stream reconnects, timeline render count, action confirmation/cancel/result, and accessibility error checks in CI.
- Product metrics use daemon-issued identifiers and durations; no prompt, transcript, source body, or handoff content is collected.
- UI errors include request IDs for daemon correlation.
- Feature flags allow navigation and each new page to roll out independently.
- Existing routes/components remain available during migration until replacement acceptance tests pass.

## Testing and Acceptance

Detailed QA will be created at `docs/qa/orchestrator/147-process-console-ui.md` after implementation is approved.

Acceptance criteria:

- [ ] Attention is the default authenticated landing page and contains no ordinary autonomous tasks.
- [ ] An operator can complete the failed-process vertical flow without opening raw logs or expert JSON.
- [ ] All primary flows are usable by keyboard and maintain visible focus.
- [ ] Light/dark, no-backdrop-filter, reduced-motion, and narrow-window modes remain readable and functional.
- [ ] Live updates do not steal focus or duplicate timeline/inbox rows after reconnect.
- [ ] Read-only users can inspect but cannot invoke writer, resume, or resolution mutations.
- [ ] Existing resource, agent, trigger, store, secret, system, wish creation, task logs, and expert trace capabilities remain reachable.
