---
name: fr-governance
description: Govern feature request (FR) documents through their full lifecycle — from planning to implementation to closure. Use when the user asks to govern/治理 a feature request, close an FR, check FR status, or says "治理FR", "/fr-governance". Scans docs/feature_request/ for open FR docs, plans implementation, executes governance, and self-checks closure.
---

# FR Governance Workflow

Execute feature request governance through its phases: Author, Select, Plan, Implement, Verify & Close, and — on request — Post-closure audit.

## Phase 0: Author (when creating a new FR)

An FR is a funded hypothesis, not a specification. Phase 2 step 0 will rebuild every factual claim it makes; write claims so they can be rebuilt *selectively* rather than distrusted wholesale.

1. **Every count carries its method and revision.** Write `200 references / 37 files (rust_source.rb scannable_source, at 2b5738ad)`, not `200 references across 37 files`. A number whose derivation is named can be re-derived alone; a bare number taints every other number in the document.
2. **Mark what was not verified.** `single-method, unverified` is a valid and useful annotation — it routes the implementer's attention. The corrections record across FR-127–144 shows the expensive errors were not the marked-uncertain claims but the confidently bare ones. A precedent cited as transferable ("do it the way batch X did") must name the premise that makes it transfer; FR-141 B5 found the B4a precedent did not transfer because its premise — imports resolving inside the layer — did not hold.
3. **Apply Phase 6's measurement discipline to every number you write.** The two rules there (second derivation, pinned revisions) exist because authoring-time numbers failed exactly those ways.

FRs are also created by `ticket-fix` and `design-governance`; this contract applies wherever the document is authored.

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
- Declare lifecycle metadata (`lifecycle: active`, plus `related_fr: FR-<N>`) — see §5.1

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

### 4.4 Assertion strength: never let a proxy be the only check

When an assertion's subject is **whether something actually happens** — a command executes, a surface is fully covered, a block really closes — text matching approximates it cheaply and wrongly. Four consecutive post-closure audits of governance FRs found the same three shapes, each written by someone who understood the domain and reached for the five-minute version:

1. **Text presence standing in for execution.** `grep -F "$script" "$job_block"` is satisfied by a commented-out step, by the script's name in a `name:` field, or by a mention in a heredoc. A `grep 'export PATH=...'` isolation check is satisfied by that line commented out. Both gates then certify an enforcement that does not run.
2. **Enumeration standing in for coverage.** A hand-listed set of scanned files, a declared list of mirror roots, a non-recursive `ls dir/*.sh` — each guards exactly what was known when it was written. The next instance lands outside it silently. The tell: the list grows by one entry per audit round.
3. **Counting or regex standing in for parsing.** Counting `{` and `}` per line to find a block's end breaks on a brace inside a string literal and silently swallows the rest of the file. Comparing whole-file totals ("as many `binary: fake-*` lines as `provider:` lines") does not establish the per-object association the contract claims.

Two more shapes joined the list during the FR-136–144 series:

4. **A text pattern standing in for a reachability property.** FR-144 counted the literal `done < <(jq` and found 17 feed sites; the property that mattered — *can this feed reach jq*, the transitive closure through same-file helper functions — gives 39 and inverts which gate is the epicentre. When the claim is about what *can happen*, a grep measures spelling. Compute the closure.
5. **A check that can report PASS having read no input.** A loop fed by `< <(jq ...)` whose exit status nobody observes reads zero rows when jq fails, never runs its body, and returns success — `set -euo pipefail` does not reach inside a process substitution, and condition-position invocation disables `set -e` for the whole call tree beneath it. Zero iterations and N passing iterations are identical in the exit code. The reader of a feed must observe the feed's status, and whether empty input fails open or closed is a per-check decision, never the loop's default. (FR-144: the enforcement-surface gate printed `13 passed, 0 failed` over a manifest jq could not parse.)

6. **A status field that reports something other than what you are asking.** `gh run view --json jobs` exposes `steps[].conclusion`, and for a step declared `continue-on-error: true` that field is `success` *whatever the step did* — it reports the step's effect on the job, which is "do not fail it". The step's own result lives in `steps.<id>.outcome`, readable only inside the workflow, which is why the `OUTCOMES` aggregation step exists. Verified against run `30288601535`, where the liveness gate genuinely failed, the job concluded `failure`, and every `steps[].conclusion` read `success`. The authoritative signals for a job whose steps swallow their own failures are the **job conclusion** and the **aggregation step's log** — never the per-step conclusion. (Found by an agent citing a green step list as evidence that a gate passed; the claim happened to be true, established by the job conclusion, and the evidence offered for it was worth nothing.)

The rule: **a proxy may be an additional condition, never the only one.** Pair it with something that observes the fact itself — parse the workflow step rather than grep the block, run the gate behind a stub that fails loudly if the real binary is reached, derive the covered set from the repository rather than from a list, associate per object rather than per file total.

Two habits that catch these before review does:

- **Pick the mutation the implementation is least likely to catch.** A negative fixture that deletes a line proves less than one that comments it out, because deletion is the case the author had in mind. State which mutation each fixture applies and why that one.
- **Ask what the assertion would still pass on.** If the answer includes a state you would consider broken, the assertion is a proxy and needs a second condition.

### 4.5 Execute QA verification

1. Run `cargo test --workspace` — all unit/integration tests pass
2. Run `cargo clippy --workspace --all-targets -- -D warnings` — no warnings
3. Run automated QA script if created — all scenarios pass
4. For scenarios that can't be automated, verify by code inspection or CLI spot-check

### 4.6 Certification validity conditions

A QA run only counts as closure evidence when all of these hold. If any fails, the run is void — fix the condition and re-run rather than reporting the result.

1. **Clean worktree** — `git status --porcelain` is empty at start and at end.
2. **Quiescent tree** — nothing else (another agent, a background job, an editor) is writing to the repo during the run. A shell script rewritten while `bash` is executing it produces garbled commands at shifted byte offsets.
3. **Pinned revision** — record `git rev-parse HEAD` before and after; they must match.
4. **True exit code** — invoke as `bash script > log 2>&1` and capture `$?` directly. Piping into `tail`/`head` reports the pager's status, masking a failed script.
5. **Complete output** — the script's final summary line must be present in the log. Its absence means the run terminated early regardless of the reported status.

### 4.7 QA safety principles

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
   - Apply the 4.4 proxy test to each new assertion: name a broken state it would still pass on. Text-presence, enumeration and count-based checks fail this most often, and a gate that certifies enforcement it cannot observe is worse than no gate — it converts an unknown into a false assurance.
   - Check whether the change **falsified any existing statement** elsewhere in the repo — user guides, design records, showcases, README indexes, CHANGELOG. A closed FR that leaves the documentation describing the removed mechanism is not closed. When the change alters a *semantic* rather than adding one — monotonic becoming exact, advisory becoming blocking — search for the old term by name; prose describing the previous rule is the most common survivor.
   - If a defect in shared tooling was reported by a prior audit and this FR touches that tooling, either fix it or record it in the design record's known limits. Extracting a known-defective function into a shared library multiplies its blast radius; inheriting it silently is a regression even when nothing breaks today.
3. Classify the FR:
   - **Closed**: all acceptance criteria met, all QA scenarios pass, no open items
   - **Partially done**: some criteria met, others remain

### 5.1 Lifecycle metadata (mandatory for every DD, QA and security document)

Every file under `docs/design_doc/**` and `docs/qa/**` carries YAML frontmatter declaring its
lifecycle. `scripts/qa/doc-lifecycle.rb` enforces this in CI, and a new document without it fails
the build.

```yaml
---
lifecycle: active          # active | superseded
related_fr: FR-132         # optional; omit rather than guess
---
```

When this FR **supersedes** an existing DD or QA document — the mechanism it describes was replaced,
not merely extended — flip that document to `lifecycle: superseded` and add
`superseded_by: <repo-relative path>` naming the successor. Leave the document's prose fence in
place: the gate reads metadata, a human reads the banner, and neither replaces the other. Do not
mark a document superseded because it received a post-release update; that is still the live path.

Regenerate the reverse index in the same commit:

```bash
ruby scripts/qa/doc-lifecycle.rb --emit-index --write   # then read the diff
```

`lifecycle` is deliberately not `status`: many design docs carry a `**Status**:` header meaning
implementation maturity, which is an independent axis. A document can be `Released` and superseded
at once.

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
6. Regenerate `config/governance/doc-lifecycle-index.json` (§5.1) so the new DD/QA pair is indexed
7. Commit all closure artifacts together

### If partially done:

1. Update the FR document to reflect current implementation status
2. Mark completed requirements with checkmarks
3. Update status from `Proposed` to `In Progress`
4. Update `docs/feature_request/README.md` table status to `In Progress`
5. Summarize remaining work and failing QA scenarios to the user

## Phase 6: Post-closure audit (on request)

Closure certifies that the FR's acceptance criteria were met. It does not certify that they were the right criteria. The highest-yield findings in this repository's governance record — the unguarded `OUTCOMES` aggregation, the two extra doors the connection-boundary FR could not see, the history limit that had never once applied, the gate that printed PASS over input it could not read — all came from audits performed *after* closure, by re-deriving the closed FR's claims against the tree rather than re-reading its evidence.

The audit is itself a measurement exercise, and this repository's audit trail records the auditor failing the same ways the authors did. Hence:

1. **Re-derive every number by a second route before reporting it** — a different tool, or the same tool at a different unit. The recorded single-derivation failures: `grep -c` counts lines, not occurrences (74 migrations that were 37; 2 CASCADE clauses that were 3); `git log --reverse -1` returns the newest match, not the oldest; a text pattern is not a reachability property (17 jq feeds that were 39).
2. **Pin both revisions before comparing two states.** "Passes locally, fails in CI" means nothing until both sides name their commit — the local tree may simply be newer than the run being compared against.
3. **Attack the acceptance criteria, not the implementation.** The implementation was verified once already; the criteria were only ever self-certified by the effort that wrote them. Apply §4.4's question one level up: what state would satisfy every criterion while the goal is unmet?
4. **A finding that generalizes goes into §4.4 or a new FR, not only into a DD's known limits.** Design records are write-once archives that no later FR's working set re-reads; skills are demonstrably in the working set — implementing agents cite §4.4 by name, unprompted. Knowledge parked only in a DD is parked where the next author will not find it.
