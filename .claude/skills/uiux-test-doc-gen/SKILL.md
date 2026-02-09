---
name: uiux-test-doc-gen
description: Generate and maintain reusable UI/UX test documents under docs/uiux/ based on the current project UI implementation and the design-system constraints. Use when a developer asks to complete UI/UX test docs, add missing UI/UX scenarios, align UI constraints, or wants UI consistency/a11y/responsive regression checks during development.
---

# UI/UX Test Doc Gen

Generate/complete `docs/uiux/**` so it evolves from a generic checklist into a set of project-aligned, reproducible, executable UI/UX test scenarios.

## Source Of Truth (Mutable)

`docs/design-system.md` is a developer-maintained and evolving source of design language and constraints. When using this skill:
- Always use the current repo's `docs/design-system.md` (do not rely on memory).
- Avoid hardcoding token numeric values into test expectations. Prefer verifying "tokens/rules are used" and reference the latest values in `docs/design-system.md` only when numeric thresholds are necessary.

## Inputs

- Project design system: `docs/design-system.md`
- Frontend code (if present): `portal/`, `frontend/`, `web/`, `ui/`, etc.
- If available: a confirmed plan or the current diff (new routes, forms, components)

## Outputs

- Update/add `docs/uiux/**` docs (only what applies to the current project)
- Generate/update `docs/uiux/_surface/*` (inputs like UI routes)
- Update `docs/uiux/README.md` index (keep it lightweight; do not hardcode totals)

## Workflow

1. **Define scope**
   - `feature-only`: align only UI/UX constraints related to the current feature (default)
   - `system-baseline`: baseline regression across the whole UI (pre-release)

2. **Discover UI surface (from code/config)**
   - Prefer running the UI route extraction script:
     - `.claude/skills/uiux-test-doc-gen/scripts/extract_surface.sh`
     - Output defaults to `docs/uiux/_surface/`
     - Optional override:
       - `PORTAL_DIRS=portal,frontend,web` (comma-separated)
   - If output is empty: the project likely has no frontend directory yet. Maintain only generic docs and constraint notes, and clearly state "no UI integrated" in doc headers.

3. **Update scenarios against the design system**
   - Any expectation involving visuals/interactions must align with `docs/design-system.md`:
     - tokens (light/dark)
     - glass/fallback behavior
     - spacing/radius
     - focus ring/a11y
     - animation duration and reduced motion

4. **Align and extend by module (prefer few, high-signal, reproducible scenarios)**
   - Visual consistency: `docs/uiux/01-03-*.md`
   - Interaction experience: `docs/uiux/04-06-*.md`
   - Accessibility: `docs/uiux/07-accessibility.md`
   - Forms/lists/dialogs: `docs/uiux/08-10-*.md`
   - For each scenario:
     - Replace `{placeholder}` with real routes/selectors/component names where possible (keep a small number of placeholders for tester-supplied values)
     - Provide runnable verification methods (DevTools console snippets, computed CSS checks, a11y tree checks)

5. **Feature-only mapping: "change -> scenarios"**
   - New pages/routes:
     - Update deep-link and active-nav cases in `docs/uiux/06-navigation-ia.md`
     - Update breakpoint coverage in `docs/uiux/05-responsive-layout.md` (at least mobile/tablet/desktop)
   - New forms:
     - Update `docs/uiux/08-forms-validation.md` (labels, errors, submission state, cancel behavior)
     - If dialogs are involved: update `docs/uiux/10-dialogs-notifications.md`
   - New data lists/tables:
     - Update `docs/uiux/09-lists-tables.md` (density, pagination/sort, empty/error states)
   - Theme/style changes:
     - Update `docs/uiux/01-design-consistency.md` and `docs/uiux/02-theme-switching.md`

6. **Update index**
   - Update `docs/uiux/README.md` index table:
     - If some modules do not apply to the current project, remove them from the index (preferred: cleaner index)

## Doc Format Rules

- Writing style reference: `.claude/skills/uiux-test-doc-gen/references/uiux-doc-style.md`
- Language: English throughout. Keep technical identifiers (tokens/selectors/ARIA) as-is.
- Each doc should have <= 5 numbered scenarios (split if needed).
- Each scenario must include:
  - Goal
  - Steps
  - Expected results (behavior/visual expectations)
  - Verification tooling (runnable snippets)

## References

- Design system constraints: `docs/design-system.md`
- UI/UX baseline entry: `docs/uiux/README.md`
- Writing style guide: `.claude/skills/uiux-test-doc-gen/references/uiux-doc-style.md`
- UI surface extraction script: `.claude/skills/uiux-test-doc-gen/scripts/extract_surface.sh`

