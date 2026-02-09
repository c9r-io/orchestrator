# UI/UX Test - Dialogs, Drawers, Toasts, And Feedback

**Module**: Common Components And States  
**Scope**: Dialog/drawer usability, focus management, confirmation flows, notification consistency  
**Scenarios**: 5

---

## Scenario 1: Dialog Open/Close Paths Are Consistent

### Goal
Verify once a dialog/drawer is opened, users can always close it using consistent mechanisms and do not get trapped.

### Steps
1. Open a dialog (for example create/edit).
2. Try closing via 3 methods:
   - close button
   - overlay click (if allowed)
   - `Esc` (if allowed)

### Expected
- Close paths follow project conventions and are consistent.
- If overlay click is disabled, there is a clear reason (for example destructive confirmation).

---

## Scenario 2: Focus Management (Trap + Restore)

### Goal
Ensure dialogs are accessible and focus returns to the trigger after closing.

### Steps
1. Open a dialog from a button.
2. Tab around to validate focus trap.
3. Close the dialog.

### Expected
- Focus does not escape to the background.
- After close, focus returns to the trigger button (or another reasonable location).

---

## Scenario 3: Scrolling And Height Strategy (Long Content)

### Goal
Verify dialogs/drawers can handle long content with scroll and keep bottom actions reachable.

### Steps
1. Open a dialog containing a long form or long list.
2. At 375x812, scroll to the bottom.

### Expected
- The content area scrolls. Title/bottom action bar is fixed or reachable per project policy.
- Bottom actions have safe padding (avoid flush edges).

---

## Scenario 4: Confirmation Flows For Destructive Actions

### Goal
Verify destructive actions (delete/disable) follow a consistent confirmation flow and clearly communicate impact.

### Steps
1. Open a destructive confirmation dialog.
2. Check copy includes: target object, impact, recoverability.
3. Perform Cancel once and Confirm once.

### Expected
- Confirm button uses destructive styling.
- Cancel is the default focus or otherwise safer (per project policy).

---

## Scenario 5: Toast/Inline Feedback Consistency

### Goal
Verify success/failure/warning feedback is consistent and does not spam the user.

### Steps
1. Trigger one successful operation and one failed operation.
2. Observe feedback placement, duration, dismissibility, and whether an error code/trace id is copyable (if present).

### Expected
- Success feedback does not interrupt flow (short toast or inline).
- Failure feedback is actionable (retry/learn more) and does not leak sensitive information.

