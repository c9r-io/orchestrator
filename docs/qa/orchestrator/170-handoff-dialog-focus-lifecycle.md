---
lifecycle: active
related_fr: FR-120
self_referential_safe: true
---

# Orchestrator GUI - Handoff Dialog Focus Lifecycle

**Module**: Orchestrator GUI  
**Scope**: Manual and Attention entry, focus trap and restoration, async invalidation, failure recovery, visual accessibility  
**Scenarios**: 5  
**Priority**: Medium

## Automated Entry Point

Run the complete Process Console frontend gate:

```bash
./scripts/qa/test-process-console-ui.sh
```

For focused iteration:

```bash
cd gui
npm test -- --run src/components/HandoffPanel.test.tsx src/pages/TaskDetail.test.tsx src/pages/AttentionInbox.component.test.tsx src/lib/routes.test.ts src/App.test.tsx
npx playwright test tests/e2e/process-console.spec.ts
```

The tests use deterministic Tauri invoke fixtures and do not execute a real resume against an operator workspace.

---

## Scenario 1: Manual Entry, Trap, Escape, And Close

### Preconditions

- Use an Operator frontend fixture with at least one logical resume boundary.
- Open a failed Process workspace.

### Goal

Verify a manually opened modal owns focus and returns it to the exact initiating control.

### Steps

1. Open review from the Process header control and confirm focus enters "Close".
2. Tab from the final enabled control and Shift+Tab from the first enabled control.
3. Close with Escape and inspect `document.activeElement`.
4. Reopen from the panel's "Preview resume" button and close with the visible Close button.

### Expected

- Forward and reverse traversal wrap inside the dialog.
- Escape and Close use the same lifecycle.
- Each close returns focus to the still-valid opening control.
- The page never receives focus while the modal remains open.

---

## Scenario 2: Attention Entry Visibility And One-shot Review Intent

### Preconditions

- An Attention item advertises `retry_failed_item` or `resume_task`.
- The actor has Operator access.

### Goal

Verify Attention transfers the operator into reviewed safe resume without performing a direct mutation and without retaining a stale route intent.

### Steps

1. Activate "Review safe resume" on the Attention item.
2. Inspect the Process route and opened dialog.
3. Close the dialog.
4. Trigger an unrelated Process rerender and inspect whether review reopens.

### Expected

- Navigation uses the exact `review=safe-resume` intent.
- The dialog opens once and the canonical URL returns to `#/processes/{taskId}`.
- No `attention_execute_action`, retry, or resume mutation is invoked by navigation.
- Close returns to the stable "Preview resume" control rather than `body`.
- Rerendering does not reopen the consumed intent.

---

## Scenario 3: Invalidated Sources And Async Boundaries

### Preconditions

- Use component fixtures that can remove controls, change the task, delay boundary responses, and unmount the panel.

### Goal

Verify focus and state updates remain safe when the initiating DOM or async owner disappears.

### Steps

1. Open review, remove the captured source, then close.
2. Execute a successful reviewed plan whose parent refresh removes the Process recovery controls, then close.
3. Start boundary loading and unmount or change task identity before it resolves.
4. Resolve the delayed request.

### Expected

- A removed source is skipped and the next valid logical control is focused.
- When all controls disappear, the persistent handoff panel receives programmatic focus.
- Late responses do not open a dialog, update busy state, or attempt focus after unmount/task change.
- No exception is emitted for disconnected nodes.

---

## Scenario 4: Busy And Failure Recovery

### Preconditions

- Configure preview and execute fixtures to remain pending and then reject.

### Goal

Verify asynchronous recovery cannot be partially dismissed and failures remain actionable.

### Steps

1. Start preview, then press Escape and activate Close while the request is pending.
2. Reject preview and inspect the alert and focused control.
3. Create a plan, enter a reason, start execution, and repeat the close attempts.
4. Reject execution and inspect the dialog and focus.

### Expected

- Busy Escape and Close do not dismiss the dialog.
- Both failures render `role="alert"` inside the still-open dialog.
- Preview failure retains an operable boundary/preview path.
- Execute failure retains the reviewed plan, operator input, and focus within the actionable modal region.

---

## Scenario 5: Accessibility, Visual Modes, And Repository Regression

### Preconditions

- Scenarios 1–4 pass.
- Chromium is installed for Playwright.

### Goal

Verify the opened modal remains accessible across supported visual preferences and the repository stays green.

### Steps

1. Run the opened-dialog Axe assertion.
2. Run the dark-theme and reduced-transparency focus-ring assertion.
3. Run `cd gui && npm run test:all`.
4. Run `./scripts/qa/test-process-console-ui.sh`.
5. Run `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `./scripts/qa-doc-lint.sh`.

### Expected

- The modal has an accessible name, `aria-modal`, focusable controls, and no serious Axe violations.
- Focus remains visibly outlined in dark and reduced-transparency modes.
- Reduced motion does not change focus semantics.
- Frontend unit, browser, build, audit, Rust workspace, strict Clippy, formatting, and documentation gates pass.

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|---|---|---|---|---|
| 1 | Manual entry, trap, Escape, and Close | PASS | 2026-07-25 | Codex | Vitest and Chromium verify both close paths and bidirectional trap |
| 2 | Attention entry visibility and one-shot review intent | PASS | 2026-07-25 | Codex | Exact intent is consumed, no direct mutation occurs, and fallback avoids body |
| 3 | Invalidated sources and async boundaries | PASS | 2026-07-25 | Codex | Removed controls, refresh removal, task fencing, and unmount are covered |
| 4 | Busy and failure recovery | PASS | 2026-07-25 | Codex | Busy close protection and both failure phases remain operable |
| 5 | Accessibility, visual modes, and regression | PASS | 2026-07-25 | Codex | FR-120 closure snapshot: 112 Vitest and 31 Playwright; current repository counts are governed by QA-172 |
