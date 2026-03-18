# QA Governance Checklist

## Mandatory Rules

1. Scenario count per file: `<=5` (`## Scenario N` or localized equivalent).
2. Checklist section required in every QA doc.
3. UI-facing docs must include at least one entry visibility scenario.
4. UI flow must start from visible entry points, not direct URL (except explicit negative tests).
5. Auth/session negative checks must be executable:
   - incognito/private window, or
   - clear session cookie, or
   - explicit sign out.
6. `docs/qa/README.md` index must match filesystem docs.
7. If present, `docs/qa/_manifest.yaml` must reflect current docs and scenario counts.
8. **Mock-fixture-only rule**: Every QA doc that runs orchestrator workflows MUST:
   - Reference a mock fixture from `fixtures/manifests/bundles/` (with deterministic `echo`/`exit` mock agents) — never a real workflow from `docs/workflow/` (which uses live AI agents and burns API credits).
   - Include the explicit `apply -f fixtures/...` command in its Preconditions or Common Preconditions block so the test agent does not need to guess which fixture to use.
   - Standalone scenario stub docs (e.g. `scenario*.md`) must either inline the full precondition commands or cross-reference the parent doc with an unambiguous path.
   - Violation severity: **P0** — using real agents in QA can exhaust API budgets in seconds.

## Unit-Test-Only Doc Convention

QA docs that use `## Scenarios` with `### S-01`…`### S-NN` subsections and verify via `cargo test` (unit-test-only) are marked with `-` in `docs/qa/README.md`. The scenario cap (`<=5`) is advisory for these docs; they are not enforced by the lint script.

Lint script pattern limitations:
- Counts `## Scenario N` and `## 场景 N` headings only.
- Does NOT count plain numbered lists (`1.`, `2.`…) under `## Scenarios`.
- Does NOT count `### S-NN` subsections.

When writing new QA docs, prefer `## Scenario N` / `## 场景 N` headings so the lint script can enforce the cap automatically.

## Recommended Split Strategy For Long Docs

1. Base + advanced split.
2. Split by capability (`api`, `ui`, `security`, `regression`).
3. Preserve scenario-number meaning with an explicit migration note.

## Report Format

1. Findings by severity: `P0/P1/P2`.
2. Fixed items list with file paths.
3. Remaining backlog with rationale.
4. Final validation output (`qa-doc-lint: PASSED`).
