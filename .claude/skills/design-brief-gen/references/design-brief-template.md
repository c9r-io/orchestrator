# Design Brief Template

## Document Header

```markdown
# {Feature/App Title} — Design Brief

**Module**: {module name}
**Status**: Draft / Confirmed
**Target**: {portal / standalone / component}
**Design System**: `docs/design-system.md`
**Related UIUX Tests**: `docs/uiux/{NN}-{name}.md`
**Created**: {YYYY-MM-DD}
**Last Updated**: {YYYY-MM-DD}
```

## Design Intent

```markdown
## Design Intent

### Problem
{What user problem does this UI solve? Why is this screen/flow needed?}

### Target Users
{Who will use this? What are their expectations and context?}

### Success Criteria
- {How do we know the design works? Measurable or observable outcomes.}
```

## Screens And Flows

```markdown
## Screens

### Screen: {Screen Name}

**Route**: `/{route-path}`
**Purpose**: {One-line description of what this screen does}

#### Layout
- Structure: {e.g., "sidebar + main content area", "full-width single column", "split panel"}
- Breakpoints: mobile (< 768px) / tablet (768-1024px) / desktop (> 1024px)

#### Components
| Component | Variant | Behavior | Notes |
|-----------|---------|----------|-------|
| {e.g., Stats Card} | {glass / solid} | {Shows metric + trend} | {x4 in grid} |
| {e.g., Line Chart} | {responsive} | {Time series, hover tooltip} | {Recharts/ECharts} |

#### Interactions
- {Click action → result}
- {Hover state → visual feedback}
- {Empty state → what users see when no data}
- {Error state → what users see on failure}
- {Loading state → skeleton / spinner / progressive}

#### Navigation
- Entry point: {How users reach this screen — sidebar link, button, redirect}
- Exit points: {Where users can go from here}
```

Repeat the Screen section for each screen in the feature.

## User Flows

```markdown
## User Flows

### Flow: {Flow Name}
{Describe the step-by-step journey across screens}

1. User lands on {Screen A} via {entry point}
2. User performs {action} → sees {feedback}
3. System navigates to {Screen B}
4. ...

### Flow: {Error/Edge Case Flow Name}
1. ...
```

## Design Constraints (From Design System)

```markdown
## Design Constraints

Reference: `docs/design-system.md`

- Theme: {light/dark/both — specify which tokens apply}
- Glass effect: {yes/no — with fallback behavior}
- Spacing: {which spacing scale — compact/normal/spacious}
- Typography: {heading/body font rules}
- Accessibility: {contrast ratio target, focus ring, ARIA requirements}
- Animation: {duration constraints, reduced-motion handling}
- Responsive: {mobile-first? breakpoint strategy}
```

## Assets And References (Optional)

```markdown
## Assets

- Design images: `assets/design/{feature-name}/` (if any)
- Figma export: `assets/design/{feature-name}/` (PNG/SVG exports, not Figma links)
- Wireframes: {text-based wireframe or reference path}

## References
- Similar patterns: {existing screens or external references for inspiration}
- Prior art: {links to existing components that can be reused}
```

## Open Questions

```markdown
## Open Questions

- [ ] {Question about design decision not yet resolved}
- [ ] {Question about scope or priority}
```
