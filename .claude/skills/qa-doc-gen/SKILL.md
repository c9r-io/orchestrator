---
name: qa-doc-gen
description: "Generate or update QA/security/UIUX test documentation after confirmed feature implementation plans or completed refactors. Use this skill AFTER plan approval or code completion to: (1) add new QA test docs for new behavior, (2) generate design docs, and (3) run cross-doc impact analysis across docs/qa/, docs/security/, and docs/uiux/ to update stale steps, expectations, and assertions. Triggers when users ask to create QA docs, update test docs after implementation, or sync QA/security/UIUX docs after behavior changes."
---

# QA Doc Gen

After a feature plan is confirmed or a refactor is completed, generate and synchronize test documentation so all QA/security/UIUX docs match real behavior.

## Workflow

```
1. Extract feature details and behavior deltas from confirmed plan / implemented code
2. Determine module classification and file naming
3. Generate design doc(s) under docs/design_doc/ (platform doc, before QA)
4. Generate QA test document(s) following project format
5. Run cross-doc impact scan on docs/qa, docs/security, docs/uiux
6. Patch all impacted existing docs to remove stale steps/assertions
7. Update docs/qa/README.md index
8. Update docs/design_doc/README.md index
9. Update docs/security/README.md and docs/uiux/README.md when changed
```

## Step 1: Extract from Confirmed Plan

From the confirmed plan and merged implementation, extract:

- **Feature name**: What the feature is called
- **Module**: Which module it belongs to (used as the folder under `docs/qa/`, e.g. `docs/qa/{module}/`)
- **Behavior**: Normal flow, error cases, edge cases
- **Behavior deltas**: What changed compared to old docs (auth rules, token types, permission boundaries, UI routes, API contracts, redirects)
- **UI interactions**: Pages, buttons, forms involved
- **UI entry points**: Navigation links, quick links, sidebar items, or buttons that lead to the feature
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

### UI Entry Visibility Rule (Mandatory for UI-facing changes)

If a change adds or modifies user-facing UI behavior, include at least one scenario that verifies users can discover and reach the feature from visible entry points.

Required checks:

1. **New feature entry**: Verify the entry exists and is visible in normal navigation flow (sidebar/tab/menu/button/quick links), not only via direct URL.
2. **Redirect/navigation changes**: Verify updated routes and redirects from the old and new entry paths.
3. **Entry consistency**: If entry UI shows counters/status badges, verify they match destination page state.
4. **Navigation-first UI steps**: Subsequent CRUD scenarios should navigate from visible entry points before performing actions.

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
| UI entry point | Verify navigation entry exists and reaches the target page |

Not every type is needed for every feature. UI entry point is mandatory for UI-facing changes.

## Step 5: Run Cross-Doc Impact Analysis (Mandatory)

After creating/updating the primary docs, always scan and classify potential impacts in:

- `docs/qa/**/*.md`
- `docs/security/**/*.md`
- `docs/uiux/**/*.md`

Use search patterns based on behavior deltas, for example:

- route changes (`/dashboard`, `/settings`, `/auth/callback`)
- token model changes (`id_token`, `access_token`, `token exchange`)
- permission/auth changes (`401`, `403`, `scope`, `audience`, `role`)
- UI navigation text and expected redirect paths

For each impacted document, do one of:

1. **Patch required**: update steps/expected results/security assertions/UI flow
2. **Note required**: add prerequisite note or branch-path note to avoid tester confusion
3. **No change**: explicitly record why unaffected

Never stop at creating only new docs when old docs are stale.

## Step 6: Update QA README Index

After creating the QA document(s), update `docs/qa/README.md`:

1. Add the new document to its module's index table
2. Keep the README lightweight and project-agnostic (avoid hardcoded totals unless the project explicitly maintains them)

## Step 7: Update Design Doc README Index

After creating the design doc(s), update `docs/design_doc/README.md`:

1. Add the new document to its module's index table
2. Keep the README lightweight and project-agnostic

## Step 8: Update Other Indexes When Changed

If the cross-doc impact analysis (Step 5) modified files under `docs/security/` or `docs/uiux/`, update their respective `README.md` indexes as well.

## Output Requirements

In the final response, always include:

1. New docs created/updated for the feature itself
2. Cross-doc impact list grouped by `qa/security/uiux`
3. Updated files and rationale per file
4. Remaining docs reviewed but unchanged (with reason)
