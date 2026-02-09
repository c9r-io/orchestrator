# QA Test Document Template

## Document Header

```markdown
# {Module Name} - {Scope Description}

**Module**: {Module Name}
**Scope**: {Short Scope Summary}
**Scenarios**: {N}
**Priority**: High / Medium / Low
```

## Optional: Background

Add this only when the feature involves API endpoints or non-UI interactions:

```markdown
---

## Background

{Feature background}

Endpoint: `METHOD /api/v1/...`

Request/response examples (if applicable)
```

## Optional: Database Schema Reference

Add this only when DB verification is required:

```markdown
---

## Database Schema Reference

### Table: {table_name}
| Column | Type | Notes |
|--------|------|-------|
| id | CHAR(36) | UUID primary key |
| ... | ... | ... |
```

## Scenario Structure (Per Scenario)

```markdown
---

## Scenario {N}: {Scenario Title}

### Preconditions
- {Precondition 1}
- {Precondition 2}

### Goal
{What this scenario validates}

### Steps
1. {Step 1}
2. {Step 2}
   - Sub-step (for example filling a form field)
3. {Step 3}

### Expected
- {UI expected outcome 1}
- {UI expected outcome 2}

### Expected Data State
```sql
{Verification SQL}
-- Expected: {expected_value}
```
```

## Optional: General Scenario

You may add one unnumbered "General Scenario" at the end (does not count toward the numbered limit). Typical uses: authentication checks, general error handling.

```markdown
---

## General Scenario: {Title}

### Steps
1. ...

### Expected
- ...
```

## Checklist

```markdown
---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | {Scenario 1 title} | ☐ | | | |
| 2 | {Scenario 2 title} | ☐ | | | |
| ... | ... | ☐ | | | |
```

## Key Rules

1. **Max 5 numbered scenarios per document** (general scenario does not count) to enable parallel testing.
2. **Scenario coverage**: happy path + error/rejection cases + boundary conditions.
3. Use `{placeholder}` for dynamic values in SQL and curl commands.
4. For UI operations, reference buttons/menus by quoted UI labels (for example "Create", "Save").
5. For API-tested scenarios, provide complete curl commands.
6. Use English throughout; keep technical identifiers (SQL, API paths, field names) as-is.

