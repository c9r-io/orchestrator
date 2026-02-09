# Design Doc Template

## Document Header

```markdown
# {Module Name} - {Feature/Change Title}

**Module**: {Module Name}
**Status**: Draft / Approved
**Related Plan**: {Plan summary or key bullets (from plan mode output)}
**Related QA**: `docs/qa/{module}/{NN}-{name}.md`
**Created**: {YYYY-MM-DD}
**Last Updated**: {YYYY-MM-DD}
```

## Background And Goals

```markdown
## Background
{Why this work is needed; taken from the plan mode problem statement}

## Goals
- {Goal 1}
- {Goal 2}

## Non-goals
- {Explicitly out of scope}
```

## Scope And User Experience (If Applicable)

```markdown
## Scope
- In scope: ...
- Out of scope: ...

## UI Interactions (If Applicable)
- Pages/routes: `{route}`
- Key buttons/forms: "Create", "Save", ...
```

## Interfaces And Data (If Applicable)

```markdown
## API (If Applicable)
- `METHOD /api/v1/...`
- Request: {fields}
- Response: {fields}
- Error codes: {list}

## Database Changes (If Applicable)
- Tables/columns: `{table}.{column}`
- Constraints: unique/index/foreign key
- Migration strategy: {forward/backward compatibility}
```

## Key Design And Tradeoffs

```markdown
## Key Design
1. {Design point 1}
2. {Design point 2}

## Alternatives And Tradeoffs
- Option A: {pros/cons}
- Option B: {pros/cons}
- Why we chose: {why}

## Risks And Mitigations
- Risk: {risk}
  - Mitigation: {mitigation}
```

## Observability And Operations (Required)

```markdown
## Observability
- Logs: {key events and fields (request_id/tenant_id, etc.)}
- Metrics: {metric names and meaning, p95/p99, etc.}
- Tracing: {spans/attributes (if applicable)}

## Operations / Release
- Config: {env vars}
- Migration / rollback: {steps}
- Compatibility: {backward/forward}
```

## Testing And Acceptance

```markdown
## Test Plan
- Unit tests: {which business logic and boundaries}
- Integration tests (if any): {dependency boundary validation}
- E2E (if any): {critical user journeys}

## QA Docs
- `docs/qa/{module}/{NN}-{name}.md`

## Acceptance Criteria
- {Acceptance criteria from the plan}
```

