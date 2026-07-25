---
lifecycle: active
related_fr: FR-120
---

# Orchestrator GUI - Handoff Dialog Focus Lifecycle

**Module**: Orchestrator GUI  
**Status**: Released  
**Related Plan**: FR-120 handoff review dialog focus lifecycle  
**Related QA**: `docs/qa/orchestrator/170-handoff-dialog-focus-lifecycle.md`  
**Created**: 2026-07-25  
**Last Updated**: 2026-07-25

## Background

The safe-resume dialog can be opened manually from a Process workspace or automatically after an operator selects "Review safe resume" in Attention. The original dialog remembered `document.activeElement` without validating it. Automatic navigation commonly made `body` the origin, while a refresh after successful execution could remove the original button. Closing the dialog could therefore lose the keyboard user's position or target a disconnected node.

FR-120 defines focus as an explicit lifecycle owned by the handoff surface. It does not change the daemon resume contract: planning remains non-mutating, execution remains reviewed and fenced, and Attention never calls a direct retry/resume mutation.

## Goals

- Preserve a meaningful keyboard position across manual and Attention-initiated review.
- Keep focus inside the modal and make every close path obey the same busy and return rules.
- Remain safe when asynchronous boundary loading finishes after navigation, task replacement, or unmount.
- Keep preview and execution errors inside an operable dialog.
- Prove the lifecycle in component tests and a real Chromium browser.

## Non-goals

- Changing resume-plan or resume-execution RPC semantics.
- Creating a generic application-wide dialog framework.
- Moving focus across a component that has already unmounted.
- Automatically executing an Attention recovery action.
- Retaining the review intent in the URL after it has been consumed.

## Entry And Intent Model

There are two entry paths:

1. Manual: the Process workspace header or the panel's "Preview resume" button increments a review request and supplies a logical return target.
2. Attention: "Review safe resume" navigates to `#/processes/{taskId}?review=safe-resume`. `TaskDetail` consumes that intent once for the selected task, removes it from the active route, and opens review without synthesizing an Attention DOM target.

Only the exact `review=safe-resume` value enables automatic review. Unknown query values do not open the dialog. Attention's advertised `retry_failed_item` and `resume_task` actions are withheld from generic action execution; the UI exposes the reviewed handoff path instead.

## Focus Source And Fallback Model

An element is a valid return target only when all of these are true:

- it is connected and is neither `body` nor `documentElement`;
- neither it nor an ancestor is hidden or `aria-hidden`;
- it is not disabled or `aria-disabled`;
- computed `display` and `visibility` keep it visible;
- it matches the supported focusable-control selector.

On close, the panel evaluates this ordered fallback ladder at the next animation frame:

1. the valid opening control captured for this review;
2. the caller-provided logical Process header control;
3. the panel's stable "Preview resume" button;
4. the persistent handoff panel itself, using programmatic `tabIndex="-1"`.

Every candidate is revalidated at restoration time. A successful resume may refresh task state and remove controls before focus restoration; the ladder therefore tolerates a disconnected source instead of throwing or returning focus to `body`. Component unmount cancels restoration because ownership has moved to the new route.

## Modal Lifecycle

- Opening waits for `resume_boundary_list`, then focuses the first safe control: "Close".
- A document-level Tab handler cycles first-to-last and last-to-first. If focus is externally displaced while the modal is open, the next Tab returns it to the appropriate modal edge.
- Escape and "Close" call one close function. While an asynchronous preview or execute is busy, both paths are disabled so a partially observed action cannot be dismissed.
- Creating a preview moves focus to "Operator reason". Returning to boundary selection moves focus to "Logical boundary".
- Preview and execution failures render as `role="alert"` inside the open modal and leave focus in the actionable region.
- Successful execution renders `role="status"` and requests a parent refresh. The dialog stays open until the operator explicitly closes it, allowing status review and deterministic fallback selection after the refresh.

The modal retains `role="dialog"`, `aria-modal="true"`, a labelled title, native controls, and the design-system focus ring. `aria-busy` is projected on the persistent handoff surface.

## Async And React Lifecycle Safety

Each boundary request receives a monotonically increasing request number. Results mutate state only when:

- the component is still mounted;
- the request remains the newest request; and
- the task identity has not invalidated it.

Unmount and task changes increment the request fence. Mount state is set in the effect setup as well as cleared in cleanup so React StrictMode's development setup/cleanup cycle does not permanently suppress valid results. Review requests also carry a monotonically increasing value and are consumed once per task.

## Accessibility And Visual Contract

- Keyboard users can enter, traverse, cancel, retry after failure, and leave the review without losing context.
- Native focus indicators remain visible in light and dark themes.
- Reduced motion changes transition behavior only; it does not alter focus movement.
- Reduced-transparency fallback removes glass dependence without hiding the focus outline.
- The opened dialog is included in Axe coverage, rather than checking only the underlying Process page.

## Compatibility And Operations

- No gRPC, Tauri command, persistence, migration, or daemon behavior changes.
- Existing `#/processes/{taskId}` links remain valid.
- The new query intent is additive and one-shot; after consumption the canonical route is the plain Process URL.
- Rollback can remove the query intent and focus lifecycle without data rollback. The resume safety contract remains intact.

## Test Plan

- Vitest covers source validation, manual Escape and close return, automatic fallback, focus trapping, removed sources, late boundary responses after unmount, busy close protection, and failure recovery.
- Route, App, Attention, and TaskDetail tests cover exact intent parsing, one-shot consumption, and the reviewed Attention path.
- Playwright covers both entry paths, consumed URLs, trap behavior, success-refresh fallback, dark/reduced-transparency focus visibility, and opened-dialog Axe results.
- Repository gates are `cd gui && npm run test:all`, `./scripts/qa/test-process-console-ui.sh`, `cargo test --workspace`, strict Clippy, Rust formatting, and QA document lint.

## Acceptance Criteria

- Manual Escape and Close return focus to the opening control.
- Automatic review returns focus to a stable Process control, never `body`.
- Removed, hidden, disabled, or disconnected sources select the next valid fallback without throwing.
- Tab and Shift+Tab remain contained by the open dialog.
- Preview and execution failures preserve the dialog and an actionable focus position.
- Component and Chromium tests cover manual, automatic, Escape, Close, refresh-removal, and unmount paths.

