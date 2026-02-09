# UI/UX Doc Style Guide

This guide keeps `docs/uiux/**` consistent across projects so developers and agents can execute and reuse it.

## Structure

Each document should include:
- Document header (module, scope, scenarios)
- Source of constraints (usually `docs/design-system.md`, or project UI conventions)
- Scenario 1..N (numbered scenarios)

## Placeholders

Use `{placeholder}` for dynamic values, for example:
- `{route}`, `{list_route}`, `{form_route}`
- `{glass_selector}`, `{dialog_selector}`

Do not put real sensitive values into UI/UX docs (real tokens, real keys, etc.).

## Verification Tooling

Prefer runnable checks:
- DevTools Console: `getComputedStyle(...)`, DOM queries, `localStorage.getItem(...)`
- DevTools Performance: FPS drops / Long Tasks
- Accessibility: keyboard navigation, ARIA relationship checks

## Writing Principles (High Bar)

- **Reproducible**: clearly state how to reach the UI and how to trigger states (empty/error/long text).
- **Decidable**: avoid vague "looks good"; tie expectations to tokens, alignment, sizes, states, and interaction outcomes.
- **Design-system first**: expectations involving tokens/radius/spacing/animation must align with `docs/design-system.md`.
- **Avoid hardcoding numeric values**: since `docs/design-system.md` can evolve, prefer verifying "tokens/rules are used" rather than pasting old token numbers into docs.

