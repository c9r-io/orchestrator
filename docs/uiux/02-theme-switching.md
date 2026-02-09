# UI/UX Test - Theme Switching And Persistence

**Module**: Visual Consistency  
**Scope**: Light/dark switching, persistence, transitions, no flash  
**Scenarios**: 4

---

## Constraints

- `docs/design-system.md`

## Scenario 1: Theme Switching Works And State Is Consistent

### Goal
Verify theme switching updates global tokens and the UI state matches the underlying `data-theme` value.

### Steps
1. Open any page.
2. Trigger theme switching (button/menu/shortcut depending on project implementation).
3. Observe changes in background, text, and glass containers.

### Expected
- `document.documentElement.dataset.theme` changes (for example to `dark`).
- All areas change consistently: background, text, borders, glass, icons/badges.
- No mixed state where parts of the UI did not switch.

### Verification Tooling
```javascript
console.log('data-theme', document.documentElement.getAttribute('data-theme'));
```

---

## Scenario 2: Theme Persists (localStorage)

### Goal
Verify the selected theme persists across reloads and new tabs.

### Steps
1. Switch to dark mode.
2. Reload the page.
3. Open another page under the same origin.

### Expected
- The theme remains applied.
- The persistence key follows the project convention (the design system recommends `theme`).

### Verification Tooling
```javascript
console.log('stored theme', localStorage.getItem('theme'));
```

---

## Scenario 3: No Flash Of Incorrect Theme (First Paint)

### Goal
Avoid a visible flash of light theme before switching to dark on first paint (FOUC), which is especially noticeable with glass backgrounds.

### Steps
1. Set dark mode and persist it.
2. Hard reload (Disable cache + reload).
3. Watch the first 1-2 seconds for "white then black" (or the reverse).

### Expected
- No noticeable color flash on first paint (or it is effectively imperceptible).

---

## Scenario 4: Transition Duration And Readability

### Goal
Verify the theme transition is not jarring or sluggish and does not break readability.

### Steps
1. Switch themes multiple times and observe key areas: background, text, glass containers, borders.
2. Re-test once on a slower device or in power-saving mode.

### Expected
- Transition duration is reasonable (200-300ms recommended; follow the project implementation and avoid 1s+).
- Readability does not significantly degrade during the transition.

