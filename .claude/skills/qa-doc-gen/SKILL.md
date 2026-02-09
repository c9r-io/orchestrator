---
name: qa-doc-gen
description: "Generate QA test case documents from confirmed feature implementation plans. Use this skill AFTER a feature plan has been approved by the user (via plan mode or explicit confirmation). Converts the agreed feature behavior, acceptance criteria, and edge cases into structured QA test documents under docs/qa/. Triggers when (1) user says to generate QA docs after plan approval, (2) user asks to create test cases for a newly planned feature, (3) user asks to turn a feature plan into QA testing content."
---

# QA Doc Gen

After a feature plan is confirmed, generate QA test case documents that capture the feature's expected behavior for manual testing.

## Workflow

```
1. Extract feature details from the confirmed plan (plan mode output)
2. Determine module classification and file naming
3. Generate design doc(s) under docs/design_doc/ (platform doc, before QA)
4. Generate QA test document(s) following project format
5. Update docs/qa/README.md index
6. Update docs/design_doc/README.md index
```

## Step 1: Extract from Confirmed Plan

From the confirmed plan, extract:

- **Feature name**: What the feature is called
- **Module**: Which module it belongs to (used as the folder under `docs/qa/`, e.g. `docs/qa/{module}/`)
- **Behavior**: Normal flow, error cases, edge cases
- **UI interactions**: Pages, buttons, forms involved
- **API endpoints**: If applicable, include method, path, request/response
- **Database changes**: New tables/columns, expected data states
- **Acceptance criteria**: What constitutes correct behavior
- **Non-functional requirements**: Security/perf/availability/observability notes if present in the plan

## Step 2: Determine File Naming

### Module mapping

Place the QA doc under the matching `docs/qa/{module}/` directory. If the feature spans multiple modules, create separate documents per module.

For design docs, use the same module mapping under `docs/design_doc/{module}/` so the docs mirror QA structure and are easy to trace.

### File numbering

Check existing files in the target directory:

```
Glob: docs/qa/{module}/*.md
```

Use the next available number: `{NN}-{descriptive-name}.md`

Example: If `docs/qa/tenant/` has `01-crud.md`, `02-list-settings.md`, `03-status-lifecycle.md`, the next file is `04-{name}.md`.

### Scenario count rule

Each document has **at most 5 numbered scenarios**. If a feature needs more than 5 scenarios, split into multiple documents.

## Step 3: Generate Design Doc (New)

Generate a design doc from the confirmed plan so future contributors can understand intent and tradeoffs.

Read `references/design-doc-template.md` for the exact format template.

### Placement

- `docs/design_doc/{module}/{NN}-{descriptive-name}.md`
- If a feature spans multiple modules, create separate design docs per module (same split rule as QA docs).

### Content rules

1. **Language**: Write everything in English. Keep technical identifiers (API paths, field names, SQL, metric names) as-is.
2. **Traceability**: Must include links (paths) to the generated QA docs and any major code touchpoints if present in the plan.
3. **Decision capture**: Record assumptions, in-scope/out-of-scope, and the minimal rollback strategy.
4. **Observability**: If the plan mentions it, include logs/metrics/tracing hooks; otherwise add a brief "Default Recommendations" section.

## Step 4: Generate QA Document

Read `references/qa-doc-template.md` for the exact format template.

### Content generation rules

1. **Language**: Write everything in English. Keep technical identifiers (SQL, API paths, field names) as-is.
2. **Scenarios must cover**: Happy path + error/rejection cases + boundary conditions
3. **UI button/menu names**: Use quoted UI labels, for example "Create", "Save".
4. **Dynamic values**: Use `{placeholder}` syntax in SQL and curl commands
5. **curl examples**: Provide complete commands for API-tested scenarios
6. **SQL verification**: Every scenario with data mutations needs an "Expected Data State" section with verification SQL
7. **Checklist**: End with a checklist table listing all scenarios (including any optional "General Scenario")

### Scenario design guidelines

For each feature behavior in the plan, generate scenarios covering:

| Type | Example |
|------|---------|
| Normal flow | Create a resource with valid data |
| Duplicate/conflict | Create with existing unique key |
| Invalid input | Missing required fields, bad format |
| Permission | Unauthorized user attempts action |
| Boundary | Max length, empty string, special chars |
| Cascade effects | Delete with dependent data |

Not every type is needed for every feature - select the relevant ones.

## Step 5: Update QA README Index

After creating the QA document(s), update `docs/qa/README.md`:

1. Add the new document to its module's index table
2. Keep the README lightweight and project-agnostic (avoid hardcoded totals unless the project explicitly maintains them)

## Step 6: Update Design Doc README Index

After creating the design doc(s), update `docs/design_doc/README.md`:

1. Add the new document to its module's index table
2. Keep the README lightweight and project-agnostic
