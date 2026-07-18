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
| `docs/uiux/06-navigation-ia.md` | Attention-first left navigation, stable process/session/source deep links, New Process, page titles | 5 |

### Accessibility
| Doc | Description | Scenarios |
|------|------|--------|
| `docs/uiux/07-accessibility.md` | Keyboard navigation, focus, contrast, ARIA, including process-console/handoff/session/source overlays | 5 |

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
4. For the FR-097 consequence dialog, run `07-accessibility.md` + `08-forms-validation.md` + `10-dialogs-notifications.md` with `docs/qa/orchestrator/144-handoff-and-safe-resume.md`.
5. For the FR-098/FR-102 Session Inspector and Process Workspace session panel, run `07-accessibility.md` with `docs/qa/orchestrator/149-agent-session-control-plane-hardening.md`; QA-145 is retained as the original specification.
6. For the Sources page and Process Workspace provenance panel, run `06-navigation-ia.md` + `07-accessibility.md` with `docs/qa/orchestrator/146-source-events-and-slack-binding.md`; reaction-card changes also run `docs/qa/orchestrator/155-slack-reaction-source-event-contract.md`, and role-aware Slack deep links run `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md`.
7. For the FR-100 console shell and Attention/process vertical flow, run `05-responsive-layout.md` + `06-navigation-ia.md` + `07-accessibility.md` with `docs/qa/orchestrator/147-process-console-ui.md`.
8. For FR-103 reviewed recovery and native Attention notifications, run `07-accessibility.md` + `10-dialogs-notifications.md` with `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md`.
9. Complex animations/glass effects: run at least `04-animations.md` (including performance and reduced motion).
10. Before a Process Console v1 release, run `docs/qa/orchestrator/153-process-console-release-acceptance.md`; its aggregate gate includes the Console Vitest/Playwright suite, real Tauri/gRPC recovery flow, accessibility, responsive, reduced-motion, and reduced-transparency checks.
11. For FR-112 Sources → Automations editors and route workbench, run `05-responsive-layout.md`, `06-navigation-ia.md`, `07-accessibility.md`, `08-forms-validation.md`, and `10-dialogs-notifications.md` with `docs/qa/orchestrator/160-process-console-source-automation-ui.md`.
12. Before the Slack Reaction Skill Automation release, run `docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md` Scenario 4 to aggregate visible Sources entry, real Tauri provenance, ReadOnly/Operator behavior, axe, focus, privacy, and 640 px acceptance.
