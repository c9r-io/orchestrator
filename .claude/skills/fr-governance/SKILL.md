---
name: fr-governance
description: Govern feature request (FR) documents through their full lifecycle — from planning to implementation to closure. Use when the user asks to govern/治理 a feature request, close an FR, check FR status, or says "治理FR", "/fr-governance". Scans docs/feature_request/ for open FR docs, plans implementation, executes governance, and self-checks closure.
---

# FR Governance Workflow

Execute feature request governance in four phases: Select, Plan, Implement, Verify & Close.

## Phase 1: Select FR

1. Scan `docs/feature_request/` for `FR-*.md` files (exclude `README.md`)
2. If no FR files found, inform the user and stop
3. If exactly one FR file, auto-select it and confirm with user
4. If multiple FR files, present a numbered list with ID, title, priority, and status, then ask the user which one to govern
5. Read the selected FR document fully to understand scope, requirements, and acceptance criteria

## Phase 2: Plan (enter plan mode)

Enter plan mode and produce a governance plan that covers:

0. **FR fact verification (do this first)** — the FR document is a proposal, not ground truth. Rebuild every factual claim it makes (inventories, counts, "N consumers remain", "workflow X still uses Y", named retirement targets) directly from the codebase, and compare. Two failure shapes to look for specifically:
   - **Category conflation** — the FR counts two different constructs as one (e.g. step-level `command:` with no agent vs Agent-level legacy `command:` templates). This silently turns requirements into no-ops.
   - **Inverted retirement target** — the FR names a symbol to remove that is still the live path, while the actually-dead branch is something else. Derive which branch has zero production consumers from code, not from the FR.

   If claims do not survive verification, correct the FR document and tell the user what was wrong before continuing. Do not plan against unverified claims.

1. **Current state audit** — grep/search the codebase to understand what has already been implemented vs what remains
2. **Implementation plan** — concrete steps to fulfill each requirement in the FR, ordered by dependency
3. **Acceptance mapping** — map each acceptance criterion to specific implementation actions and verification methods
4. **Risk mitigation** — address risks listed in the FR with concrete countermeasures
5. **QA test plan** — identify which scenarios need automated QA scripts (`scripts/qa/`) and which are verified by unit tests or code inspection
6. **Artifact plan** — identify design docs (`docs/design_doc/`) and QA docs (`docs/qa/`) to create upon closure

Present the plan to the user for approval before proceeding.

## Phase 3: Implement

After plan approval, execute the implementation:

1. Implement code changes following the plan
2. Run `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` after each major change
3. Commit incrementally with descriptive messages

## Phase 4: Verify (QA)

After implementation is complete, execute QA verification:

### 4.1 Create QA test document

Create `docs/qa/orchestrator/<N>-<topic>.md` with verification scenarios following existing conventions:

- Each scenario has: Steps, Expected result
- Steps use concrete CLI commands, `cargo test` invocations, or `grep`/`rg` checks
- Mark the document with appropriate safety frontmatter (`self_referential_safe: true/false`)

### 4.2 Create automated QA script (when applicable)

If the FR involves daemon behavior, CLI output, or HTTP endpoints, create `scripts/qa/test-<topic>.sh`:

- Use the standard pattern: `set -euo pipefail`, `pass()`/`fail()` helpers, cleanup trap
- Use non-standard ports (19xxx) to avoid conflicts with running daemons
- Start/stop daemon instances within the script with proper cleanup
- Exit 0 on all pass, exit 1 on any failure

### 4.3 Retirement and migration parity evidence (mandatory for removal/migration FRs)

When the FR removes or migrates an existing execution path, structural evidence is not sufficient. Counting consumers and asserting that a symbol no longer appears proves the old path is gone; it does not prove the new path still works. Produce all three:

1. **Recorded pre-migration baseline** — capture the old path's observable contract (terminal state, exit code, exact output, normalized events) as a committed fixture, before removal.
2. **Per-object post-migration comparison** — replay each migrated production object against its baseline. One aggregate "the suite passes" is not per-object evidence.
3. **Reverse-applicable removal patch** — prove the removal commit can be mechanically reverted, and record where that evidence lives.

Also assert at least one **end-to-end behavior** that depended on the old path and must still hold — typically the thing the migrated object exists to do. A migration whose acceptance criteria are entirely counts and symbol-absence checks will pass while the migrated feature is broken.

### 4.4 Execute QA verification

1. Run `cargo test --workspace` — all unit/integration tests pass
2. Run `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
3. Run automated QA script if created — all scenarios pass
4. For scenarios that can't be automated, verify by code inspection or CLI spot-check

### 4.5 Certification validity conditions

A QA run only counts as closure evidence when all of these hold. If any fails, the run is void — fix the condition and re-run rather than reporting the result.

1. **Clean worktree** — `git status --porcelain` is empty at start and at end.
2. **Quiescent tree** — nothing else (another agent, a background job, an editor) is writing to the repo during the run. A shell script rewritten while `bash` is executing it produces garbled commands at shifted byte offsets.
3. **Pinned revision** — record `git rev-parse HEAD` before and after; they must match.
4. **True exit code** — invoke as `bash script > log 2>&1` and capture `$?` directly. Piping into `tail`/`head` reports the pager's status, masking a failed script.
5. **Complete output** — the script's final summary line must be present in the log. Its absence means the run terminated early regardless of the reported status.

### 4.6 QA safety principles

Follow these principles from past governance experience:

- **Avoid self-referential unsafe tests**: Do NOT create tests that modify the orchestrator's own database, kill its own daemon, or alter its own config in ways that could corrupt state
- **Use isolated instances**: QA scripts should start their own daemon instances with separate data directories when possible
- **Prefer unit tests**: If a behavior can be tested with `cargo test`, prefer that over daemon-based integration tests
- **Prefer read-only verification**: Use `grep`, `rg`, `cargo test --lib`, `orchestrator manifest validate` over stateful operations
- **Mark safety level**: QA documents that test self-referential behavior must have `self_referential_safe: true` frontmatter and limit scenarios to safe operations

## Phase 5: Close

After QA verification passes:

### Self-check procedure

This step is self-referential by construction: the same effort that authored the evidence is now judging it. Compensate by attacking each criterion rather than confirming it.

1. Re-read the FR document
2. For each acceptance criterion, verify implementation by referencing QA test results, then adversarially:
   - Ask **"what input or state would make this assertion false?"** and confirm a test exists that would actually fail in that case (a negative fixture, not only a positive one).
   - Reject criteria satisfied purely by structural checks (counts equal, symbol absent, file present). Each needs at least one behavioral assertion alongside it.
   - Check whether the change **falsified any existing statement** elsewhere in the repo — user guides, design records, showcases, README indexes, CHANGELOG. A closed FR that leaves the documentation describing the removed mechanism is not closed.
3. Classify the FR:
   - **Closed**: all acceptance criteria met, all QA scenarios pass, no open items
   - **Partially done**: some criteria met, others remain

### If closed (all criteria met):

1. Create design doc under `docs/design_doc/orchestrator/` documenting the design decisions
2. QA doc should already exist from Phase 4
3. Update `CHANGELOG.md` under `[Unreleased]` — the repo declares Keep a Changelog and SemVer, so this is part of closure, not an optional extra:
   - Anything deleted goes under `### Removed` (create the section if absent). Removed types, config fields, and execution paths all qualify.
   - Any user-visible incompatibility goes under `### Compatibility And Migrations` with the exact diagnostic a user will hit (e.g. a manifest field that is now rejected at apply time). Do not leave a blanket "existing clients remain compatible" line standing when it is no longer true.
   - Re-read the existing `[Unreleased]` entries and **fix any that this FR falsified**. Unreleased entries are not historical records — a stale one ships as a false statement.
4. Delete the FR file from `docs/feature_request/`
5. Update `docs/feature_request/README.md`:
   - Remove the FR row from the table (or mark status)
   - Add a closure note following the existing pattern: `FR-XXX 已闭环删除；其设计与验证信息现由 docs/design_doc/... 与 docs/qa/... 承载`
6. Commit all closure artifacts together

### If partially done:

1. Update the FR document to reflect current implementation status
2. Mark completed requirements with checkmarks
3. Update status from `Proposed` to `In Progress`
4. Update `docs/feature_request/README.md` table status to `In Progress`
5. Summarize remaining work and failing QA scenarios to the user
