# UI/UX Test - Forms And Validation Experience

**Module**: Common Components And States  
**Scope**: Labels/help text, validation timing, error-state consistency, submission states and idempotency  
**Scenarios**: 5

---

## Scenario 1: Field Labels, Help Text, And Required Markers Are Consistent

### Goal
Verify form information structure is clear: label vs help text vs optional/required markers vs placeholder all have distinct roles.

### Steps
1. Open a form (page or dialog).
2. For each field, check:
   - A visible label exists
   - Help text exists when needed
   - Optional/required markers are consistent

### Expected
- Labels do not rely on placeholders (placeholders do not replace labels).
- Help text is visually distinct from error messages.

---

## Scenario 2: Validation Timing And Error Rendering Do Not Interrupt Typing

### Goal
Verify the validation strategy avoids frequent flashing while typing (debounce/blur/submit) and error messages are actionable and locatable.

### Steps
1. Enter an invalid value (for example too short or wrong format).
2. Observe when errors appear (onChange/onBlur/onSubmit).
3. Fix to a valid value and observe how errors clear.

### Expected
- Error copy is specific and tells the user how to fix it.
- Error appearance/disappearance does not flicker.

---

## Scenario 3: Submit Button State (disabled/loading) Is Consistent

### Goal
Avoid duplicate submission and ambiguous states: on submit, the button shows loading and the form prevents repeated submits.

### Steps
1. Fill the form and click submit.
2. Before the request completes, click submit again or press Enter.
3. Check whether multiple requests are sent (Network panel).

### Expected
- After submit, the button becomes disabled or shows a visible loading state.
- No duplicate write requests are sent.

---

## Scenario 4: Distinguish Server Errors From Field Errors

### Goal
Verify server responses are presented in the right place:
- field-level errors (for example `name already exists`)
- global errors (for example 500/timeout/permission)

### Steps
1. Trigger a field conflict error (duplicate value).
2. Trigger a global error (offline/forced 500/insufficient permission).

### Expected
- Field conflict: error is close to the field and the field is highlighted.
- Global error: shown at top-of-form or as a toast; user input is preserved.

---

## Scenario 5: Cancel/Close Behavior And Unsaved Changes Prompt (If Applicable)

### Goal
Verify cancel/close behavior is consistent and, if the project requires it, warns about unsaved changes.

### Steps
1. Change form fields without submitting.
2. Click cancel/close or navigate back.

### Expected
- If policy requires: show a confirmation prompt to avoid accidental loss.
- If policy does not require: close behavior is still consistent and leaves no half-state behind.

