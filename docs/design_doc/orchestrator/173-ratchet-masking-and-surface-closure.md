---
lifecycle: active
related_fr: FR-158
---

# DD-173: Ratchets that count code, and a surface that covers its own engine

**Status**: Released

Companion to [DD-172](172-governance-expansion-boundary.md), which carries the
rule this work was done under. This document records the three repairs FR-158
made and the two of its five requirements that turned out not to need making.

## Requirement 1 was already closed before the FR was filed

FR-158 §1 reported that the `governance` job had 36 `continue-on-error` steps
against a hand-maintained `OUTCOMES` list of 35, and asked for the list to be
derived.

Both halves were wrong, and the way they were wrong is on the record already.
At the FR's own pinned revision `9bcfaa96` the counts were **35 and 35, in
sync**. The 36 came from `grep -c 'continue-on-error'`, which matched a *comment*
at `ci.yml:167` — the single-derivation failure §6 rule 1 of the `fr-governance`
skill lists by name, committed by the auditor writing the FR.

The requirement was also already implemented. `check_continue_on_error_aggregated`
has existed since FR-137, derives its facts from `workflow_model.rb outcome-facts`
rather than from any list, covers **every job of every workflow** rather than the
one job that has such steps today, and asserts three failure directions where the
FR named one — including the step with no `id`, which cannot be aggregated by
construction and which the FR's framing does not reach. Fixtures 22, 23 and 24
cover all three, and **fixture 22 is FR-158's acceptance criterion 1 verbatim**.
Fixture 22b is its positive control: the same step with its `OUTCOMES` line added
must pass, so the case is about the omission and not about the edit.

Recorded here rather than quietly dropped, because "the gate you are asking for
exists and your evidence for needing it was a miscount" is the most useful thing
this FR found, and it is invisible from the FR document alone.

## Requirement 3: the ratchets count code now, not prose

### What was wrong

Three gates counted a Rust identifier by scanning source with comments and string
literals intact. DD-142 recorded it as a known limit of the `rusqlite` ruler — "a
comment mentioning rusqlite counts" — and DD-148 recorded the instance that makes
it concrete: a doc comment written to explain that the driver conversion had been
*removed* named the impl, and put the file back on the ledger. The workaround at
the time was to stop spelling the type's path, which is precision traded for a
metric.

DD-140 recorded a second, independent defect in the same family: the four
coordination coordinates were **line**-count regexes, `counts[X] += 1 if
line.match?(...)`, blind to a second reference added to a line that already has
one.

FR-158's own §3 merged the two families and then named the token-counting gate as
the "line-count regex" pilot. They are different gates with different defects and
both are fixed.

### Measurements

Prose was not a rounding error:

| coordinate | before | after | ruler change |
|---|---|---|---|
| `capturesOrJsonPath` | 23 | **12** | masked + occurrences |
| `pipelineVariables` | 29 | 26 | masked + occurrences |
| `celInterpreter` | 9 | 8 | masked + occurrences |
| `legacyRunnerSelection` | 0 | 0 | masked + occurrences |
| persistence `rusqlite` | 215 | **207** | masked |
| persistence `sql` | 597 | 597 | **deliberately unmasked** |

`capturesOrJsonPath` was 52% prose. The reason is worth stating because it is not
an accident of style: the validator that *rejects* `behavior.captures` names the
field in its rejection message, so the code deleting the retired surface counted
as code using it. Every removed hit was verified individually as a comment, a doc
comment, or a diagnostic string; the genuine consumers — `step.behavior.captures`
at `workflow_steps.rs:78` and `:459` — are code and survive masking.

### The part that must not be masked

`persistence-dependency.rb`'s `SQL_STATEMENT` anchors on the opening quote: its
entire subject is the string literal that masking blanks. Counting it on the
masked source yields **0 statements for every file in the workspace**, measured.

This is §4.4 shape 10 — the repair that fixes the under-reach opens the
over-reach — and the failure it would produce is quiet rather than loud. The gate
compares against a ledger, so an implementer who unified the two rulers and then
ran `--emit-baseline --write` would commit a ledger recording zero SQL and a
green gate, having deleted the residual that condition 2 exists to hold down. So
the two rulers read two different sources from one masked copy, and case 21 fails
on the unified version.

`core-boundary.rb` is converted although nothing moves: core references the
driver 0 times now that the extraction has completed, and `publicItems` reads 611
either way. It is converted because it counts the same token as
`persistence-dependency.rb`, and a divergence introduced while both sides read
zero is one nobody would be looking for — the premise `scripts/lib/rust_source.rb`
exists to protect (DD-142).

### Fixtures, and what mutating them proved

- **Case 20** spells the driver three ways in one file — a doc comment, a line
  comment, and inside a string literal — then adds a real reference to the *same*
  file and requires the count to move by exactly one. The second half is what
  makes the first half mean anything: "stayed green after I added prose" is also
  satisfied by the file never being read.
- **Case 21** pins the SQL ruler to the unmasked source.

Both were mutation-tested rather than assumed:

| mutation | result |
|---|---|
| revert driver masking | case 20 fails alone — 23 passed, 1 failed |
| unify both rulers onto masked | case 21 fails alone — 17 passed, 7 failed |

Two different signatures, so the log says which ruler broke.

## Requirement 4: the surface covers the engine

`check_surface_complete` scanned `find scripts/qa`. 28 of 122 tracked scripts sat
outside that root, and they included **every shared library the ci-required gates
source**: `rust_source.rb`, `rust_lexer.rb`, `workflow_model.rb`, `gate_jq.sh`,
`provider_isolation.sh` and seven more. The gates were governed; the engine they
run on was not. A defect in a library reaches every caller at once, which is the
one place where being ungoverned costs the most.

The root is now `scripts`. All 28 are classified per path, and
`scripts/spikes` is deliberately **not** a subtree exemption: a recursive ignore
absorbs files that do not exist yet and never produces a line in any log (§4.4
shape 8), so the three spike files are declared individually and a fourth fails
the gate.

### The extension list was a hand-written list

`WorkflowModel::SCRIPT_TOKEN` matched `.sh` and `.rb`. FR-147 had carefully
*derived* the executed set instead of listing it — and then listed the alphabet.
`scripts/sync-docs.mjs` is executed by `docs.yml` on every push to main and again
by the ci-required `test-docs-publishing-integrity.sh`, and check 14 could not see
it in either place.

Widening the token to `.mjs` surfaced a file that fits no existing role: not
`library`, which states the file is never invoked as a gate, and not
`release-tooling`, whose entire discipline is that every executing workflow be
tag- or dispatch-triggered — `docs.yml` runs on a branch push. Hence the
`generator` role, whose discipline is `verifiedBy`: a named gate that regenerates
the artifact and compares, which the check requires to be present **and**
`ci-required`. Without that field the role is `release-tooling` with its one
condition removed, which is the amnesty shape 8 describes.

### watchdog

`docs/architecture.md` §7 described Layer 4 as monitoring the live system and
restoring the `.stable` snapshot "and restarts the service". It does neither.
`--help` health-checks the executable on disk, so a daemon that is wedged,
deadlocked or serving errors passes every poll; a restore is `cp` plus
`chmod +x`, with nothing restarted. The design record had this right the whole
time — `DD self-bootstrap/01` lists watchdog-managed process restarts as a
non-goal — so the architecture document was corrected to match the design it was
describing, and the script is now declared `manual-runbook` under QA
self-bootstrap/02, which already carried its runbook.

## Requirement 2: freshness for the 35 human-run gates

See `scripts/lib/gate_runlog.sh` and
`config/governance/manual-gate-freshness.json`. The design decisions worth
recording:

- **Composition, not replacement.** 30 of the 35 gates run `trap cleanup EXIT`,
  and a second bare `trap ... EXIT` discards the first silently — in these
  scripts, a leaked daemon on a bound port or a leaked data directory. The
  library reads `trap -p EXIT` back and re-installs the previous handler behind
  the recorder, record first so `$?` is still the gate's own status.
- **Six gates refuse to run on a dirty worktree**, and recording dirties it.
  They share `gate_runlog_worktree_status`, which excludes the ledger by
  pathspec — one predicate rather than six copies that would drift.
- **The ledger starts empty and the first report says 35 of 35 stale.** That is
  the honest starting state rather than a backfilled fiction, and it is what the
  acceptance criterion asked for.
- **An unrecorded run leaves `null`.** If ruby is missing the gate still runs and
  a line goes to stderr; the ledger is untouched. Freshness fails closed — an
  unrecorded run reads as not-run, never as fresh.

### The scope predicate that was a fact about the set I looked at

Six gates refuse to run on a dirty worktree and were repaired to read the tree
through the shared predicate. The set was derived by scanning the
**manual-runbook** gates for `git status --porcelain`, which found six — and
"refuses to run on a dirty worktree" is not a property of that classification.
Six *more* gates have it, all `ci-required`:
`test-agent-driver-execution-migration.sh`,
`test-agent-driver-production-parity.sh`, `test-coordination-strangler.sh`,
`test-legacy-coordination-decommission.sh`, `test-persistence-extraction.sh`
and `test-pipeline-variable-retirement.sh`.

This is §4.4 shape 9's third premise — a scope predicate sufficient for the set
in front of you is a fact about that set, not about the check — and it was found
the way that premise says it usually is not: by running the thing. **FR-158's own
first certification sweep was voided by it.** Two ci-required gates invoke a
manual-runbook gate; the invoked gate recorded a run, wrote the tracked ledger
mid-sweep, and `test-agent-driver-production-parity.sh` went red on the resulting
diff — failing for the recorder's reason rather than its own, which is exactly
the pathology the six manual gates were patched to avoid, landing on the six
nobody had looked at.

The repair has two independent halves, because either alone leaves a hole:

- **`gate_runlog.sh` records nothing when no human is present**, via `CiEnv` —
  the repository's single answer to "am I unattended", rather than a second and
  narrower `ENV.key?("CI")`. This is the ledger's subject and not an
  optimisation: `manual-runbook` means *executed by a person following the owner
  QA document*, so a nested invocation inside CI is not the thing being measured.
  It fails closed, leaving `null`, which reads as not-run.
- **All twelve cleanliness checks now share one predicate.** The CI guard alone
  would leave a developer running two gates locally in the same state.

### Three defects in this work, caught by the suite rather than by review

Recorded because two of them are the shapes this repository's own skill warns
about, committed while implementing the warning:

1. Check 15 first matched the arming with `grep -F`, which a commented-out call
   satisfies. **§4.4 shape 1**, in the code written to enforce §4.4. Fixture 29
   comments the call out rather than deleting it — deletion being the case the
   author has in mind — and caught it. The match is anchored to line start now.
2. The behavioural block ran a probe that exits 7 without disabling `set -e`, so
   the suite ended there and the summary line never printed. **§4.4 shape 7**: a
   truncated run reads exactly like a complete one.
3. Check 15 also reported "declared but absent from disk", which check 1 already
   owns, and fixture 2 stopped isolating. Removed rather than deduplicated.

## Known limits

- **Cargo.toml manifests are read unmasked.** There is no TOML lexer here, so a
  `#` comment naming a coordinate in a manifest still counts. `celInterpreter`
  is the only coordinate that reads manifests, and a dependency line naming
  `cel-interpreter` is a genuine reference, which is why the manifests are in
  scope at all. Stated in the ledger's `scope` prose.
- **Mutation-testing a ratchet destroys its ledger's reviewed half.** Running a
  mutated gate with `--emit-baseline --write` drops entries whose counts fall to
  zero, and their reviewed `category` fields go with them; regenerating
  afterwards cannot reinvent them. Restore the ledger from git rather than by
  re-emitting. Found the hard way during this FR.
- **A nested invocation records as a run, which is true of the script and not of
  the runbook.** Seven ci-required call sites invoke a manual-runbook gate —
  `certify-slack-managed-live.sh` reaches three, `test-qa-gate-surface.sh` three,
  `test-agent-driver-execution-migration.sh` one. Under CI nothing is recorded,
  so this only arises locally, and it is left in place deliberately: the ledger's
  purpose is to distinguish a gate nobody has run in a year from one that ran
  this morning, and a nested execution genuinely does prove the script still
  runs, which is exactly the class of decay FR-148 and FR-149 found by hand. What
  it does not prove is that a human followed the procedure around it. The
  alternative — a list of invoking call sites — is the enumeration §4.4 shape 2
  condemns, and the set is already seven and growing.

  The consequence for **certification**: a local sweep of the derived ci-required
  set must run with `CI=1`, or a nested manual gate writes the tracked ledger
  mid-run and the sweep cannot end on a clean worktree as §4.6 requires. That is
  also the more faithful environment, since CI is where these gates actually run.
  FR-158's own second and third sweeps were voided this way before the
  precondition was written down.
- **The freshness ledger cannot see a gate run from a different checkout.** It
  records `git rev-parse HEAD` of the tree the gate ran in; a run against a
  worktree or a clone writes into that copy's ledger and is lost unless
  committed from there.
- **`check_manual_gates_record_runs` reads text.** It cannot see whether arming
  fires. The behavioural half is the probe case in fixture mode, which asserts
  the record, the cleanup marker and the exit status together — any one alone
  passes on a broken composition.
- **`ci-liveness.rb` is red and was red before this FR.** Its job records were
  taken at `45fbf3c4`, before `.github/workflows/ci.yml` last changed at
  `ceccf4f5`; both predate this work. Already recorded in
  [DD-171](171-interactive-session-process-reclamation.md). Refreshing it needs a
  real CI run.
