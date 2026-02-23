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

## Recommended Split Strategy For Long Docs

1. Base + advanced split.
2. Split by capability (`api`, `ui`, `security`, `regression`).
3. Preserve scenario-number meaning with an explicit migration note.

## Report Format

1. Findings by severity: `P0/P1/P2`.
2. Fixed items list with file paths.
3. Remaining backlog with rationale.
4. Final validation output (`qa-doc-lint: PASSED`).
