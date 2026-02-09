# UI/UX Tests

This directory contains reproducible, verifiable UI/UX test documents. Use them during development to continuously align implementation with the design system and usability constraints, reducing UI rework.

Conventions:
- Write everything in English. Keep technical details (CSS tokens, selectors, paths, ARIA) as-is.
- Use `{placeholder}` for dynamic values (for example `{route}`, `{dialog_selector}`).
- Keep each document to at most 5 numbered scenarios. Split into multiple documents if needed.

## Design System (Source Of Constraints)

- `docs/design-system.md` (design tokens, component standards, accessibility, animation, fallbacks)

## Environment

```bash
PORTAL_BASE_URL="http://localhost:3000"   # If there is a Web UI
```

## Index

### Visual Consistency
| Doc | Description | Scenarios |
|------|------|--------|
| `docs/uiux/01-design-consistency.md` | Design token usage, Liquid Glass consistency, fallbacks | 5 |
| `docs/uiux/02-theme-switching.md` | Light/dark switching, persistence, no flash | 4 |
| `docs/uiux/03-visual-hierarchy.md` | Typography hierarchy, spacing, layout boundaries | 4 |

### Interaction Experience
| Doc | Description | Scenarios |
|------|------|--------|
| `docs/uiux/04-animations.md` | Duration/easing, reduced motion, performance | 5 |
| `docs/uiux/05-responsive-layout.md` | Breakpoints, touch targets, layout stability | 5 |
| `docs/uiux/06-navigation-ia.md` | Navigation consistency, deep links, back behavior, page titles | 5 |

### Accessibility
| Doc | Description | Scenarios |
|------|------|--------|
| `docs/uiux/07-accessibility.md` | Keyboard navigation, focus, contrast, ARIA | 5 |

### Common Components And States
| Doc | Description | Scenarios |
|------|------|--------|
| `docs/uiux/08-forms-validation.md` | Labels/validation/errors/submission states | 5 |
| `docs/uiux/09-lists-tables.md` | Lists/tables, pagination, sorting, empty states | 5 |
| `docs/uiux/10-dialogs-notifications.md` | Dialog/drawer/toast, focus trap, confirmation flows | 5 |

## Execution Guidance (During Development)

1. New pages/routes: run at least `06-navigation-ia.md` + `05-responsive-layout.md`.
2. New forms/create-edit flows: run at least `08-forms-validation.md` + `07-accessibility.md`.
3. Visual/token changes: run at least `01-design-consistency.md` + `02-theme-switching.md`.
4. Complex animations/glass effects: run at least `04-animations.md` (including performance and reduced motion).
