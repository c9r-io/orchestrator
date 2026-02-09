# UI/UX Test - Visual Hierarchy And Layout Boundaries

**Module**: Visual Consistency  
**Scope**: Typography hierarchy, spacing system, information structure, safe container boundaries  
**Scenarios**: 4

---

## Constraints

- `docs/design-system.md`

## Scenario 1: Clear Hierarchy Between Title And Body Text

### Goal
Verify the page hierarchy (H1/H2/section title/body/help text) is clear so users can quickly find primary actions and key information.

### Steps
1. Open a typical list page and a typical form page (for example `{list_route}`, `{form_route}`).
2. Scan top-to-bottom: header/title area, main content area, supporting information.
3. Compare font sizes and weights across title/body/help text.

### Expected
- Page title is clearly above section titles; section titles are above body text; supporting text is visually weaker.
- Avoid a flat hierarchy where all text looks the same size/weight.

---

## Scenario 2: Consistent Spacing System (Grid/Card Padding/Section Spacing)

### Goal
Verify grid gaps, card padding, and section spacing follow a consistent system to avoid density drift across pages.

### Steps
1. Open a page with multiple cards/sections.
2. Check grid gap (the design system recommends 16px).
3. Check card padding (the design system recommends 20px).

### Expected
- Primary grid gaps are consistent.
- Same-kind cards share consistent padding.
- Sections have clear breathing room (not cramped together).

---

## Scenario 3: Container Safe Area Clearance

### Goal
Verify buttons and form controls keep reasonable clearance from container edges to avoid "flush-to-edge" visuals and accidental taps.

### Steps
1. Open a page or dialog with primary action buttons.
2. Check the distance between buttons and container edges, especially in bottom action bars.
3. Re-test on a narrow viewport.

### Expected
- Bottom buttons are not flush against container borders.
- On mobile, full-width buttons keep left/right padding from the screen edge (recommended >= 16px).

---

## Scenario 4: Overflow Strategy (Long Text/IDs/URLs)

### Goal
Verify long content does not break layout and the information remains accessible.

### Steps
1. Identify areas where long content can appear: name/id/token/url/description.
2. Create long content using extreme input or test data.
3. Observe whether containers overflow, tables horizontally scroll, or buttons get squeezed.

### Expected
- Use one of: `truncate`, `break-all`, tooltip/expand, and apply it consistently.
- Avoid page-wide horizontal overflow that causes layout jitter.

