# UI/UX Test - Animations And Transitions

**Module**: Interaction Experience  
**Scope**: Duration/easing, interaction feedback, reduced motion, performance and stability  
**Scenarios**: 5

---

## Constraints

- `docs/design-system.md` (200-300ms recommended; avoid overly long animations)

## Scenario 1: Clear Hover/Active Micro-Interactions

### Goal
Verify buttons/cards/navigation items have clear hover/active feedback and that the feedback is consistent.

### Steps
1. Find 3 types of interactive elements: button, link/nav item, card/list row.
2. Trigger hover and active (press and hold).
3. Observe whether feedback is perceptible.

### Expected
- Hover provides small changes (brightness/shadow/position) without being excessive.
- Active provides more explicit pressed feedback (for example `scale(0.98-0.99)` or a darker background).
- Same-kind controls behave consistently.

---

## Scenario 2: Page/Section Entrance Animations Are Not Excessive

### Goal
Verify page transitions or section entrance animations do not reduce efficiency or induce motion discomfort.

### Steps
1. Switch quickly between multiple pages (at least 5 times).
2. Observe whether the first paint / main content uses entrance animations.

### Expected
- Animation duration is around 200-300ms (follow the project implementation).
- Avoid sluggish experiences like 500ms+ large movements on every navigation.

---

## Scenario 3: `prefers-reduced-motion` Is Respected

### Goal
Verify animations are disabled or significantly reduced when the user enables reduced motion.

### Steps
1. Enable reduced motion in OS accessibility settings.
2. Reload the page.
3. Re-test hover, navigation transitions, and dialog open/close.

### Expected
- Animations are disabled or shortened.
- The UI remains usable and does not rely on animation completion to reveal content.

---

## Scenario 4: Performance Baseline (Avoid Animation Jank)

### Goal
Verify animations primarily use `transform/opacity` and avoid animating expensive properties (like `backdrop-filter`/`box-shadow`) in a way that causes FPS drops.

### Steps
1. Record a trace in DevTools Performance.
2. Repeatedly hover cards and open/close a dialog 10 times.
3. Stop recording and check FPS and Long Tasks.

### Expected
- No obvious interaction jank.
- No sustained Long Tasks during interaction.

---

## Scenario 5: Animations Do Not Cause Layout Shift

### Goal
Verify animations do not introduce unnecessary layout shift (especially in tables/lists and action bars).

### Steps
1. On a list page, quickly hover multiple rows and watch whether row height changes.
2. Open/close a dialog and watch whether the background content shifts.

### Expected
- Hover on list rows must not change row height.
- Dialog open/close does not cause background horizontal shift (handle scrollbar behavior).

