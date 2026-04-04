---
name: design-brief-gen
description: "Generate a Design Brief and corresponding UIUX test documents from user design intent. Use when a user describes a UI/UX idea, wants to design a new screen/feature/app from scratch, asks to create a design brief, or says 'design from UIUX'. This skill turns conversational design intent into structured Design Brief documents (docs/design_brief/) and simultaneously produces matching UIUX test scenarios (docs/uiux/). Triggers on: design intent description, 'create design brief', 'design-first workflow', UI/UX planning requests, or when the user provides wireframes/mockups and wants structured documentation."
---

# Design Brief Gen

Turn conversational design intent into a structured Design Brief and matching UIUX test documents — the design-first entry point for feature development.

## Why This Skill Exists

Feature requests (FR) start from behavior ("what should the system do?"). Design Briefs start from user experience ("what should the user see and feel?"). This skill captures design intent through conversation — the same low-friction model as FR — and produces two outputs:

1. **Design Brief**: Structured screen/flow/component documentation under `docs/design_brief/`
2. **UIUX Test Doc**: Testable scenarios aligned to the design, under `docs/uiux/`

The Design Brief becomes the upstream artifact that drives implementation, much like an FR drives code.

## Workflow

```
1. Interview: Clarify design intent through conversation
2. Discover constraints from design system
3. Generate Design Brief document
4. Generate UIUX test scenarios from the brief
5. Cross-check existing UIUX docs for impact
6. Update indexes
```

## Step 1: Interview — Clarify Design Intent

Extract the following from the conversation. Ask for missing pieces — but keep it conversational, not a form.

**Required**:
- **What**: What screens/pages/flows are being designed?
- **Why**: What user problem does this solve?
- **Who**: Who are the target users?
- **Where**: Where does this live? (portal, standalone app, component library)

**Helpful but optional** (infer from context if not stated):
- Layout preferences (sidebar + content, full-width, split panel, etc.)
- Key components (tables, charts, forms, cards, etc.)
- Interaction patterns (CRUD, dashboard, wizard, etc.)
- Responsive requirements
- Reference designs or existing screens to emulate

If the user provides design images or mockups, reference them in the brief's Assets section. Images are input context, not hard requirements — the brief should describe intent, not pixel-match an image.

## Step 2: Discover Design System Constraints

Read `docs/design-system.md` to extract applicable constraints:
- Token palette (light/dark)
- Glass/surface treatment rules
- Spacing, radius, typography scales
- Accessibility requirements (contrast, focus ring, ARIA)
- Animation and reduced-motion policy
- Responsive breakpoint strategy

These constraints become the "Design Constraints" section of the brief and inform UIUX test expectations.

If no `docs/design-system.md` exists, note "No design system defined" in the constraints section and use sensible defaults.

## Step 3: Generate Design Brief

Read `references/design-brief-template.md` for the exact format template.

### Placement

- `docs/design_brief/{module}/{NN}-{descriptive-name}.md`
- Module mapping follows the same convention as `docs/design_doc/` and `docs/qa/`
- Check existing files to determine the next available number

### Content Rules

1. **Language**: English throughout. Keep technical identifiers (routes, component names, CSS tokens) as-is.
2. **Screens**: Each screen gets its own section with layout, components table, interactions, and navigation.
3. **Flows**: Describe user journeys across screens as numbered steps.
4. **Constraints**: Always reference `docs/design-system.md` — never hardcode token values, reference the token names.
5. **States**: Every screen must address: normal, empty, loading, and error states.
6. **Entry points**: Every screen must specify how users reach it (not just direct URL).

### Screen Description Guidelines

For each screen, think through:

| Aspect | What to Document |
|--------|-----------------|
| Layout | Structure and responsive behavior |
| Components | What's on screen, which variants, behavior |
| Data | What data is shown, where it comes from |
| Interactions | Click, hover, submit, drag — and their results |
| States | Normal / empty / loading / error / disabled |
| Navigation | How to get here, where to go next |
| Accessibility | Keyboard nav, screen reader, ARIA roles |

## Step 4: Generate UIUX Test Scenarios

From the Design Brief, generate matching UIUX test scenarios under `docs/uiux/`.

### Mapping Rules: Brief → Test Scenarios

| Design Brief Element | UIUX Test Coverage |
|---------------------|-------------------|
| New screen/route | Navigation + deep-link test in `06-navigation-ia.md` |
| Layout description | Responsive breakpoint test in `05-responsive-layout.md` |
| Component list | Design consistency check in `01-design-consistency.md` |
| Theme constraints | Theme switching test in `02-theme-switching.md` |
| Interactions | Animation/transition test in `04-animations.md` |
| Form components | Form validation test in `08-forms-validation.md` |
| Table/list components | List/table test in `09-lists-tables.md` |
| Dialog/notification | Dialog test in `10-dialogs-notifications.md` |
| Accessibility notes | Accessibility test in `07-accessibility.md` |
| Visual hierarchy | Visual hierarchy check in `03-visual-hierarchy.md` |

### Scenario Generation Rules

1. Each new or updated UIUX doc should have <= 5 numbered scenarios (split if needed).
2. Scenarios must include: Goal, Steps, Expected Results, and Verification tooling (DevTools snippets, computed CSS checks, etc.).
3. Reference design token names from `docs/design-system.md` — do not hardcode numeric values.
4. For new screens: always include at least one scenario for responsive layout and one for navigation entry.
5. Follow the writing style in `.claude/skills/uiux-test-doc-gen/references/uiux-doc-style.md` if it exists.

## Step 5: Cross-Check Existing UIUX Docs

Scan existing `docs/uiux/*.md` for:
- Navigation scenarios that need updating (new routes/entry points)
- Responsive scenarios that need new breakpoint coverage
- Theme scenarios that need new component coverage

For each impacted doc: patch the affected scenarios or add a note about the new screens.

## Step 6: Update Indexes

1. Create `docs/design_brief/README.md` if it doesn't exist, or update it with the new brief.
2. Update `docs/uiux/README.md` if UIUX docs were added or modified.

## Output Summary

In the final response, always include:

1. **Design Brief**: Path and summary of what was documented
2. **UIUX Test Docs**: List of new/updated test documents with scenario counts
3. **Cross-doc impact**: Any existing UIUX docs that were updated
4. **Open questions**: Design decisions that need user input before implementation

## References

- Design Brief template: `references/design-brief-template.md`
- Design system constraints: `docs/design-system.md`
- UIUX test doc conventions: `.claude/skills/uiux-test-doc-gen/references/uiux-doc-style.md`
- Existing UIUX docs: `docs/uiux/README.md`
