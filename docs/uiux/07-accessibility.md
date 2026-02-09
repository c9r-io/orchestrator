# UI/UX Test - Accessibility

**Module**: Accessibility  
**Scope**: Keyboard navigation, visible focus, semantics and ARIA, color contrast  
**Scenarios**: 5

---

## Constraints

- `docs/design-system.md` (focus ring, contrast, keyboard accessibility)

## Scenario 1: Core Tasks Are Possible With Keyboard Only

### Goal
Verify users can complete "navigate + open dialog/form + submit/cancel" without using a mouse.

### Steps
1. Reload the page and start navigating with `Tab`.
2. Navigate to at least one sub-page via keyboard.
3. Open a dialog/drawer (if present).
4. Fill an input field and submit (or cancel).

### Expected
- All interactive elements are reachable via `Tab/Shift+Tab`.
- `Enter/Space` triggers buttons/links.
- No keyboard traps (Tab does not get stuck).

---

## Scenario 2: Focus Indicators Are Clear And Not Clipped

### Goal
Verify `:focus-visible` is clearly visible and does not disappear on glass backgrounds.

### Steps
1. Tab to a button, input, and link.
2. Observe focus ring color, thickness, and offset.

### Expected
- Focus ring is clearly visible (2px recommended; follow the project design).
- Focus is not clipped by overflow/shadows.

---

## Scenario 3: Dialog Has Focus Trap And Esc Close

### Goal
Verify focus cycles inside the dialog and `Esc` closes it (unless explicitly disabled).

### Steps
1. Open a dialog (example selector: `{dialog_selector}`).
2. Tab to the last focusable element, then press Tab once more.
3. Shift+Tab back to the first element.
4. Press `Esc` to close.

### Expected
- Focus cycles inside the dialog.
- After close, focus returns to the trigger element (focus restore).

---

## Scenario 4: Form Labels And Errors Are Screen-Reader Friendly

### Goal
Verify label/`aria-describedby`/error relationships are correct so screen readers can read "field name + error reason".

### Steps
1. Find a required field.
2. Submit an empty form to trigger an error.
3. Inspect DOM for:
   - `label[for]` matches `input#id`
   - Error message element has a stable id
   - `aria-invalid="true"`
   - `aria-describedby` points to the error id

### Expected
- Required/error states are accessible to assistive tech.
- Errors are not conveyed by color alone.

---

## Scenario 5: Color Contrast And Readability On Translucent Glass

### Goal
Verify text/background contrast meets requirements, especially when translucent glass overlays reduce effective contrast.

### Steps
1. Open the same page in light mode and dark mode.
2. Check:
   - body text vs background
   - secondary text vs background
   - danger/accent text vs background

### Expected
- Primary text meets WCAG AA contrast (or a higher project target).
- Text on glass surfaces remains readable and does not become too low-contrast due to transparency.

