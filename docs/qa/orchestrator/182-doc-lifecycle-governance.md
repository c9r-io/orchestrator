---
lifecycle: active
related_fr: FR-132
self_referential_safe: true
---

# Orchestrator - Design Doc And QA Doc Lifecycle Governance

**Module**: Governance / Documentation
**Scope**: lifecycle frontmatter completeness, supersession referential integrity, reverse-index consistency, and compatibility with the existing documentation gates
**Scenarios**: 5
**Priority**: High

## Background

FR-132 gives every file under `docs/design_doc/` and `docs/qa/` a machine-readable lifecycle, so
"does this document still describe the system?" stops being a question that requires reading the
whole document against the code. FR-126 needed four manual audit rounds to find that DD-101/102/103
described a deleted execution seam; nothing had marked them.

`scripts/qa/doc-lifecycle.rb` enforces the metadata and generates
`config/governance/doc-lifecycle-index.json`, which carries the two reverse directions
`docs/feature_request/README.md` cannot: document → feature request, and superseded → successor.
Current state: 380 governed documents, 377 active, 3 superseded, 244 with `related_fr` across 121
feature requests.

All scenarios below are read-only against the working tree or operate on copies under `$TMPDIR`.
None starts a daemon, touches the runtime database, or invokes a provider. See DD-144.

Primary entry points:

```bash
./scripts/qa/test-doc-lifecycle.sh                    # all twelve gate cases
ruby scripts/qa/doc-lifecycle.rb                      # the gate itself
ruby scripts/qa/doc-lifecycle.rb --emit-index         # the regeneration path
```

---

## Scenario 1: Every Governed Document Is Classified, And The Index Matches

### Preconditions

- Clean worktree, repository root is the working directory.

### Steps

1. `ruby scripts/qa/doc-lifecycle.rb`
2. `ruby scripts/qa/doc-lifecycle.rb --emit-index > /tmp/index.json`
3. `diff /tmp/index.json config/governance/doc-lifecycle-index.json`
4. Confirm coverage is derived rather than listed:
   `grep -c "governed_documents" scripts/qa/doc-lifecycle.rb` and read the function — it must glob
   `DOC_ROOTS`, and no file in the repository may enumerate the scanned document set.

### Expected Result

- Step 1 exits 0 and prints `Doc lifecycle: PASS` with `380 governed document(s): 377 active, 3
  superseded`.
- Step 3 produces no output: the regeneration path reproduces the committed index byte for byte.
- Step 4 confirms the scan walks the filesystem. A roster would guard only what existed when it was
  written.

---

## Scenario 2: The Three Superseded Documents Point At Their Actual Successor

This is the human half of the check. The gate proves `superseded_by` resolves to a file; it cannot
prove the named file describes the replacement.

### Preconditions

- None.

### Steps

1. `grep -l "^lifecycle: superseded" -r docs/design_doc docs/qa`
2. For each result, read its `superseded_by` value and open that document.
3. Confirm the successor describes the mechanism that replaced the one in the superseded document.
4. Confirm the superseded document still carries its prose banner.
5. Confirm `docs/design_doc/orchestrator/127-agent-driver-abstraction.md` is **not** in the list.

### Expected Result

- Step 1 returns exactly three documents: DD-101 (`101-streaming-agent-runner-architecture-pivot`),
  DD-102 (`102-stream-json-event-ingestion`), DD-103 (`103-cel-stream-run-signals`).
- All three name `docs/design_doc/orchestrator/138-agent-driver-execution-migration.md`, which
  records the migration of every production Agent and the deletion of the global streaming executor
  — the successor their own banners already name.
- Step 4 passes: the metadata did not replace the prose. The gate reads one, a human reads the other.
- Step 5 passes. DD-127 is `**Status**: Released` and is cited as the current driver abstraction by
  DD-129, DD-130, DD-138 and `docs/architecture.md`. Its banner is a post-release update, not a
  supersession fence; FR-132's background listed it in error and DD-144 records the correction.

---

## Scenario 3: Each Rejection The Gate Claims Is Real

### Preconditions

- `ruby` available. Every mutation lands in a copy under `$TMPDIR`.

### Steps

1. `bash scripts/qa/test-doc-lifecycle.sh > /tmp/lifecycle.log 2>&1; echo $?`
2. Read the log and confirm all twelve cases are named and passing.
3. Confirm each case is isolated by a targeted defect, per the mutation table in DD-144: removing
   frontmatter (case 3), dropping a successor (4), dangling pointer (5), self-reference (6), a
   two-document cycle (7), an out-of-vocabulary value (8), a free-text `related_fr` (9), a document
   in a brand-new subdirectory (10), `--write` under `CI` (11), and a stale committed index (12).

### Expected Result

- Exit 0 with `FR-132 doc lifecycle: 12 passed, 0 failed`.
- Cases 6 and 7 are the ones an existence check alone would not catch: in both, every pointer
  resolves to a real file.
- Case 10 is the coverage assertion — the document lands in a directory that did not exist when the
  gate was written, so an enumerated roster would miss it.
- Case 9 asserts the gate's *diagnostic*, not just its exit code. Editing `related_fr` also moves
  the index, so a gate with no format check still exits non-zero; during the FR-132 mutation run
  this case passed for exactly that wrong reason before it was corrected.

---

## Scenario 4: The Index Follows The Frontmatter

### Preconditions

- Clean worktree.

### Steps

1. Note the current `counts` block in `config/governance/doc-lifecycle-index.json`.
2. In a scratch copy of the repository subset, change one document's `related_fr` and run
   `ruby scripts/qa/doc-lifecycle.rb --emit-index`.
3. In the same copy, run `ruby scripts/qa/doc-lifecycle.rb` against the unchanged committed index.
4. `CI=1 ruby scripts/qa/doc-lifecycle.rb --emit-index --write` in the copy; check the index digest.
5. Confirm `byFeatureRequest` and `supersedes` in the committed index are non-empty.

### Expected Result

- Step 2's output differs from step 1 and contains the new FR id.
- Step 3 exits 1: a stale index is a failure, not a warning. The comparison is exact in both
  directions, so a *removed* document fails it too — a monotonic rule would let that pass while the
  index went on asserting a document the repository no longer has.
- Step 4 exits non-zero with `refusing --write under CI` and the index digest is unchanged. An
  automatic rewrite in CI would turn the review gate into decoration.
- Step 5 shows 121 feature requests indexed and the DD-138 supersession edge present — the reverse
  directions that motivated the FR.

---

## Scenario 5: The Existing Documentation Gates Are Unaffected

The backfill gave 378 documents a frontmatter block in one commit. This is the compatibility
regression FR-132 asked for.

### Preconditions

- Clean worktree, committed state.

### Steps

1. `bash scripts/qa-doc-lint.sh`
2. `bash scripts/qa/test-agent-driver-documentation-alignment.sh` and again with `--fixture-test`
3. `bash scripts/qa/test-markdown-link-integrity.sh`
4. `bash scripts/qa/test-docs-publishing-integrity.sh`
5. `bash scripts/qa/test-qa-gate-surface.sh` and again with `--fixture-test`
6. Confirm `docs/design_doc/**` and `docs/qa/**` are absent from
   `config/governance/docs-publishing.json`.

### Expected Result

- Steps 1–5 all exit 0. In particular `qa-doc-lint.sh` still finds every scenario count, checklist
  section and README index entry — the frontmatter did not shift what those scans read.
- Step 3 passes and is not doing this work for us: the link gate resolves relative Markdown links in
  document *bodies*, and a `superseded_by:` frontmatter scalar is not a Markdown link. The existence
  check in `doc-lifecycle.rb` is the only thing that sees it.
- Step 5 reports `20 of 53 gates are ci-required`, including both new entries.
- Step 6 confirms the publish set is `docs/guide` and `docs/showcases` only, so the new frontmatter
  cannot reach the VitePress site.

---

## Checklist

| # | Scenario | Result | Date | Tester |
|---|----------|--------|------|--------|
| 1 | Every governed document is classified, and the index matches | ☑ PASS | 2026-07-25 | Claude |
| 2 | The three superseded documents point at their actual successor | ☑ PASS | 2026-07-25 | Claude |
| 3 | Each rejection the gate claims is real | ☑ PASS | 2026-07-25 | Claude |
| 4 | The index follows the frontmatter | ☑ PASS | 2026-07-25 | Claude |
| 5 | The existing documentation gates are unaffected | ☑ PASS | 2026-07-25 | Claude |

## Certification Conditions

A run of these scenarios counts as closure evidence only when `git status --porcelain` is empty at
start and at end, `git rev-parse HEAD` matches across the run, nothing else is writing to the
repository, each script is invoked as `bash <script> > log 2>&1` with `$?` captured directly rather
than through a pager, and each log ends with its own summary line.
