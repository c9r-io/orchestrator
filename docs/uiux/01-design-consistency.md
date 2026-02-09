# UI/UX Test - Design Consistency And Tokens

**Module**: Visual Consistency  
**Scope**: Design tokens, consistent glass containers (Liquid Glass), fallback behavior  
**Scenarios**: 5

---

## Constraints

- `docs/design-system.md`

## Scenario 1: Global Token Availability (Light/Dark)

### Goal
Verify the page uses the design system CSS variables instead of scattered hard-coded colors/shadows.

### Steps
1. Open any UI page (for example `{route}`).
2. Open DevTools Console and read key token values for `:root` and `[data-theme="dark"]`.

### Expected
- In light mode, variables like `--bg-primary`, `--glass-bg`, `--text-primary`, `--accent` exist and are non-empty.
- After switching to dark mode, values like `--glass-bg`, `--text-primary` change in a reasonable way (dark mode must not reuse light values).

### Verification Tooling
```javascript
const root = getComputedStyle(document.documentElement);
const pick = (k) => root.getPropertyValue(k).trim();

[
  '--bg-primary',
  '--bg-secondary',
  '--glass-bg',
  '--glass-border',
  '--text-primary',
  '--accent',
  '--danger',
].forEach((k) => console.log(k, pick(k)));
```

---

## Scenario 2: Glass Container Style Consistency (Card/Panel)

### Goal
Verify primary containers (card/panel) use consistent glass parameters and radius/shadow so visual "material" does not drift across pages.

### Steps
1. Identify a typical container (example selector: `{glass_selector}`, such as `.liquid-glass`).
2. Inspect computed styles.
3. Hover the container and inspect hover state differences.

### Expected
- `backdrop-filter` and `-webkit-backdrop-filter` are present (if the browser supports them).
- `border-radius` matches the convention (cards are recommended at 20px).
- On hover, the container slightly lifts (`transform`) and shadow increases (`box-shadow`).

### Verification Tooling
```javascript
const el = document.querySelector('{glass_selector}');
if (!el) throw new Error('glass element not found');

const s = getComputedStyle(el);
console.log('background', s.backgroundColor);
console.log('backdropFilter', s.backdropFilter);
console.log('webkitBackdropFilter', s.webkitBackdropFilter);
console.log('borderRadius', s.borderRadius);
console.log('boxShadow', s.boxShadow);
console.log('transition', s.transition);
```

---

## Scenario 3: Fallback When `backdrop-filter` Is Unsupported

### Goal
Verify glass containers degrade to a readable fallback in environments without `backdrop-filter` support (for example, use `--bg-secondary`) and do not cause text contrast failures.

### Steps
1. Validate in an unsupported/restricted environment (for example disable related features, or use a browser/version without support).
2. Open a page containing glass containers.
3. Compare readability and contrast for text inside the container.

### Expected
- Glass container background degrades to an opaque (or more opaque) background (readability first).
- Text contrast does not significantly degrade.

---

## Scenario 4: Control Radius And Size Consistency (Buttons/Inputs)

### Goal
Verify buttons/inputs follow a consistent radius and size system and do not mix multiple styles on the same screen.

### Steps
1. Find at least 2 buttons and 2 inputs (for example `{primary_button}`, `{secondary_button}`, `{input}`).
2. Inspect computed `border-radius` and `height/line-height/padding`.

### Expected
- Buttons/inputs use the recommended radius (12px by default; follow the current `docs/design-system.md`).
- Same-kind controls on the same page have consistent height (small/large variants are allowed, but must be systematic).

---

## Scenario 5: Consistent Color Semantics For Destructive Actions

### Goal
Verify destructive actions consistently use `--danger` (or a destructive variant) to reduce user misinterpretation.

### Steps
1. Open a page or dialog that contains destructive actions like delete/disable.
2. Check whether button and warning colors follow destructive semantics.

### Expected
- Destructive action buttons are clearly distinguishable from primary (accent) actions.
- The same destructive actions look consistent across different pages.

