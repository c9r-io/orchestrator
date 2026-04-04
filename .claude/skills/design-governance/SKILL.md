---
name: design-governance
description: "Govern Design Brief documents through their full lifecycle — from design review to implementation to visual validation and closure. Use when the user asks to govern a design brief, check design brief status, validate a UI implementation against its brief, or says '治理设计', '/design-governance'. Scans docs/design_brief/ for open briefs, orchestrates review, implementation (pure-frontend or cascading to FR for backend), visual validation via UIUX tests and Playwright, and closure."
---

# Design Governance Workflow

Govern a Design Brief from draft through implementation to visual validation and closure. This is the design-first counterpart to `fr-governance` — where FR governance drives functional implementation, design governance drives user experience implementation.

## When to Use

- A Design Brief exists under `docs/design_brief/` and needs to be implemented
- The user wants to validate that a UI implementation matches its design intent
- The user says "治理设计" or asks to govern/close a design brief

## Phase 1: Select Design Brief

1. Scan `docs/design_brief/` recursively for `*.md` files (exclude `README.md`)
2. If no brief files found, inform the user and stop
3. If exactly one brief, auto-select and confirm with user
4. If multiple briefs, present a numbered list with title, module, status, and target, then ask which one to govern
5. Read the selected brief fully to understand screens, flows, components, and constraints

## Phase 2: Design Review

Enter plan mode. Audit the brief for completeness and alignment before implementation.

### 2.1 Completeness Check

Verify the brief covers all required sections:

| Section | Required | Check |
|---------|----------|-------|
| Design Intent (problem, users, success criteria) | Yes | Has concrete problem statement, not just "we need X" |
| Screens (layout, components, interactions, states) | Yes | Every screen has normal + empty + loading + error states |
| User Flows | Yes | At least one happy-path flow documented |
| Design Constraints | Yes | References `docs/design-system.md` tokens, not hardcoded values |
| Navigation entry points | Yes | Every screen specifies how users reach it |
| Accessibility notes | Recommended | ARIA roles, keyboard nav, contrast |

Flag missing sections and propose additions. The user decides whether to fill gaps now or proceed.

### 2.2 Design System Alignment

Read `docs/design-system.md` and cross-check:

- Are referenced tokens valid? (token names exist in the design system)
- Do component variants match what the design system defines?
- Are responsive breakpoints consistent with the system's breakpoint strategy?
- Are accessibility requirements at least as strict as the system baseline?

Report any misalignments. The user decides resolution.

### 2.3 Implementation Scope Classification

Classify the brief into one of:

- **Frontend-only**: Pure UI, no new API/backend changes needed. Proceed directly to Phase 3.
- **Full-stack**: Needs backend support (new API endpoints, data models, etc.). Identify what backend work is needed and recommend creating FR documents for the backend portion.

Present the plan to the user for approval before proceeding.

## Phase 3: Implement

### 3A: Frontend-Only Path

1. Identify the target directory (`portal/`, `site/gui/`, or as specified in the brief)
2. Implement screen by screen, following the brief's layout and component specs
3. Apply design system tokens — use CSS variables from `docs/design-system.md`, not hardcoded values
4. Implement all documented states: normal, empty, loading, error
5. Implement responsive behavior per the brief's breakpoint specs
6. After each screen, run type-check and lint:
   - `npm run typecheck` or `npx tsc --noEmit` (if TypeScript)
   - `npm run lint` (if configured)

### 3B: Full-Stack Path (Cascade to FR)

When backend work is needed:

1. **Create FR for backend**: Generate an FR document under `docs/feature_request/` covering the API/data requirements derived from the brief. Link it back to the Design Brief.
2. **Implement frontend with mocks**: Build the UI against mock data or stub APIs so visual validation can proceed independently.
3. **Track dependency**: Note in the brief's status that backend FR(s) are pending: `Status: In Progress (backend: FR-XXX pending)`
4. **Reconnect**: Once backend FR is implemented (via `fr-governance`), replace mocks with real API calls.

The key principle: UI implementation and visual validation should not be blocked by backend work. Build the frontend first with mocks, validate visually, then integrate.

## Phase 4: Visual Validation

This is the design-governance equivalent of FR governance's QA phase — but focused on visual/interaction correctness rather than functional correctness.

### 4.1 UIUX Test Execution

Locate the UIUX test documents that were generated alongside the brief (or by `uiux-test-doc-gen`):

1. Read each relevant `docs/uiux/*.md` test scenario
2. For scenarios with verification tooling (DevTools snippets, computed CSS checks), execute them
3. For scenarios requiring visual inspection, use Playwright to capture screenshots

### 4.2 Playwright Visual Checks

For each screen documented in the brief:

1. Navigate to the screen's route
2. Capture screenshots at each documented breakpoint (mobile, tablet, desktop)
3. Verify against brief expectations:
   - Layout structure matches (sidebar present, grid arrangement, etc.)
   - Key components are visible and correctly positioned
   - Empty/loading/error states render correctly
   - Theme switching works (if documented)
4. Save screenshots to `docs/design_brief/_validation/{brief-name}/`

### 4.3 Design System Compliance

Verify implementation against `docs/design-system.md`:

- Token usage: Are CSS variables used (not hardcoded colors/spacing)?
- Glass effect: Backdrop-filter applied with fallback?
- Focus ring: Visible on keyboard navigation?
- Contrast: Text meets WCAG AA (4.5:1 normal, 3:1 large)?
- Reduced motion: Animations respect `prefers-reduced-motion`?

### 4.4 Navigation and Entry Point Verification

- Verify each screen is reachable from its documented entry point (not just by direct URL)
- Verify deep-link works (direct URL loads correctly)
- Verify breadcrumb/back navigation if documented

### 4.5 Validation Report

Produce a summary:

```
## Visual Validation Report — {Brief Title}

### Screens Validated
| Screen | Route | Mobile | Tablet | Desktop | States | Pass |
|--------|-------|--------|--------|---------|--------|------|
| ... | ... | OK/FAIL | OK/FAIL | OK/FAIL | 4/4 | Yes/No |

### Design System Compliance
- Token usage: PASS/FAIL (details)
- Accessibility: PASS/FAIL (details)
- Responsive: PASS/FAIL (details)

### Issues Found
1. {issue description} — {severity: critical/minor}

### Screenshots
- `docs/design_brief/_validation/{brief-name}/`
```

If issues are found, fix them and re-validate. Repeat until all checks pass or user accepts remaining issues.

## Phase 5: Close

After visual validation passes:

### Self-check procedure

1. Re-read the Design Brief
2. For each screen: verify it was implemented and validated
3. For each user flow: verify the journey works end-to-end
4. For each design constraint: verify compliance
5. Classify:
   - **Closed**: All screens implemented, all validations pass
   - **Partially done**: Some screens remain or backend FRs are still pending

### If closed (all screens validated):

1. **Create design doc**: Generate `docs/design_doc/{module}/{NN}-{name}.md` documenting the implemented design decisions (using the existing design-doc-template from qa-doc-gen)
2. **Finalize UIUX test docs**: Ensure all UIUX test scenarios are up-to-date with the actual implementation
3. **Delete the brief**: Remove the file from `docs/design_brief/`
4. **Update indexes**:
   - Update `docs/design_brief/README.md`: remove the row, add closure note following the pattern: `DB-XXX closed; design decisions in docs/design_doc/..., visual tests in docs/uiux/...`
   - Update `docs/design_doc/README.md`: add the new design doc
   - Update `docs/uiux/README.md` if test docs were modified
5. **Commit** all closure artifacts together

### If partially done:

1. Update the brief's `Status` to `In Progress`
2. Note which screens are done and which remain
3. If backend FRs are blocking, list them with their current status
4. Summarize remaining work to the user

## Relationship to Other Skills

```
design-brief-gen ──→ design-governance ──→ design_doc (closure)
                          │                     ↑
                          ├──→ fr-governance ────┘  (if backend needed)
                          │
                          └──→ uiux-test-doc-gen    (test scenarios)
```

- `design-brief-gen`: Creates the brief and initial UIUX test docs (upstream)
- `fr-governance`: Governs backend FRs that this brief spawns (parallel track)
- `uiux-test-doc-gen`: Refines UIUX test scenarios (Phase 4 input)
- `design-system-guidance`: Constraint source throughout the process
- `qa-doc-gen`: Design doc template reused at closure
