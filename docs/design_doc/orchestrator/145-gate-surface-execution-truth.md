---
lifecycle: active
related_fr: FR-134
---

# DD-145: Gate Surface Execution Truth

**Status**: Implemented (FR-134)
**Related**: [DD-139](139-qa-gate-enforcement-surface.md) (the surface this repairs),
[DD-140](140-governance-ledger-regeneration.md) (the ledger tooling that could not run in CI),
[DD-141](141-skill-mirror-integrity.md) (mirror roots), [DD-142](142-core-boundary-freeze.md)
(the shared scanner), [DD-144](144-doc-lifecycle-governance.md)

## The defect

FR-127 built a ledger of how every `scripts/qa` gate is enforced, and a gate to verify it. A
post-closure audit mutation-tested that gate and found four ways to break the repository while it
reported **5 passed, 0 failed**. Three shared one root cause, and it is the cause FR-127 was filed
to eliminate: **text that describes a fact standing in for the fact**.

| Mutation | What the check asked | What it should have asked |
|---|---|---|
| `run: ./scripts/qa/test-filesystem-trigger.sh` → `# disabled: … was flaky` | does the job block contain this string | does a live step execute it |
| the parity gate's `export PATH` commented out | does the file contain this line | is the shadow in effect when it runs |
| unpinned `provider: claude` + `binary: fake-decoy` on a different agent | are there as many pins as providers | is *this* agent pinned |
| a CI-enforcement claim planted in `README.md` | does `docs/` or `.claude/skills/` say this | does any tracked Markdown say this |

A commented-out line contains the same characters as a live one. That is the whole of it.

Then the first real CI runs after FR-127 disproved a second premise. FR-127's case was "46 gates
and only 3 run in CI". At least two of those three had **never executed an assertion**: the
`coordination-strangler` and `slack-certification-recorded` jobs did not install ripgrep, so their
gates exited on their own `command -v` preamble. They were wired, scheduled, and visible in the
logs. *Wired* is not *running*, which is the next layer down from the sentence FR-127 set out to
retire.

## What the FR got wrong

| FR claim | Reality |
|---|---|
| surface is `45 = 45`, 12 ci-required | **53 = 53, 20 ci-required** when work began. FR-131 and FR-132 landed gates after filing |
| the stale-claim scan misses **83** tracked Markdown files | **41.** FR-131 untracked 36 generated site pages |
| widening the scan will surface false positives needing exemptions | **Zero.** `.agents` and `.cursor` are symlinks, so `git ls-files` never descends into the mirrors. The exemption list ships empty |
| defect Y is **6** stale `monotonic` statements | **8**, and five of the six cited line numbers had drifted. Missed: DD-137's governance summary, its row in the design-doc index, and QA-175's "exact and monotonic" |
| `--write` CI detection is duplicated in **2** places | **3.** `doc-lifecycle.rb` added a fourth copy while this FR was open |
| `scripts/qa/lib/hidden-gate.sh` is a hypothetical | **Live.** `scripts/qa/lib/slack-live-certification-lib.sh` was already tracked and already invisible |

## Design

### Facts, not descriptions of facts

Four libraries under `scripts/lib/`, each replacing one reading of text with an observation.

**`workflow_model.rb`** parses workflow steps. `check_wiring_truth` asks whether a *live step's
`run:`* executes the script, with heredoc bodies and shell comments removed and `if: false` steps
dropped. All four mutations above stop reading as wiring.

**`manifest_model.rb`** walks the bundle document stream per agent. The `fixture-pinned` contract
says "every claude/codex agent in the bundle also declares `binary: fake-*`" — a property of each
object, which a count over a file cannot express.

**`provider_isolation.sh`** resolves `claude` after the shadow is established and fails if it
lands outside. The parity gate now asserts its own isolation on every run, and
`check_provider_isolation` **executes** that assertion against a synthetic PATH in both directions,
requiring it to accept a shadowed provider and reject an unshadowed one.

**`rust_lexer.rb`** carries string, char, raw-string and nested-comment state across lines, so the
ledgers' brace counting cannot be broken by `.body("{")`.

The text conditions were **kept** where they were cheap and the defect real — the `cp` and
`export PATH` lines are still checked, with comments stripped first. FR-134's rule is that a proxy
may be an additional condition, never the only one, and deleting them would have lost a real
signal to make a point.

### Coverage is derived; enumeration is only for exemptions

Four checks read a list of things to look at, and each guarded only what its author knew:

| Site | Was | Now |
|---|---|---|
| stale-claim scan | `docs` + `.claude/skills` | `git ls-files '*.md'` minus declared exemptions |
| gate classification | `ls scripts/qa/*.{sh,rb}` | recursive, with `supportFiles[]` naming non-gates |
| mirror roots | `mirrorRoots` in the policy | tracked symlinks pointing into the source, discovered from the index |
| CI liveness | would have been the gate ledger | jobs parsed out of every workflow file |

The mirror rule found a live casualty immediately:
`.claude/skills/orchestrator-guide/orchestrator-guide`, a tracked symlink pointing at
`../../.claude/skills/orchestrator-guide` from inside that directory, resolving to a
`.claude/.claude` that has never existed. Committed in `1f5af317`, referenced by nothing, seen by
no check. Deleted.

### Environment is part of the gate

`test-governance-ledger-tooling.sh` passed 8/8 on every developer machine and had **never once
succeeded in CI**. Its second case verifies that `--write` refuses under `CI`; its third then
called `--write`, was refused by the mechanism just verified, and died at `set -e`. The gate's
positive path was mutually exclusive with its own safety mechanism, and only where it actually ran.

Nothing structural sees this. The gate is wired, its dependencies are present, its assertions are
sound, and it is dead. So `test-ci-environment-parity.sh` runs each in-scope gate with the CI
variables set and cleared and requires the same exit code — an equivalence assertion, not a success
one, because a gate failing identically in both is a different problem this has no business hiding.

A gate that runs every `ci-required` gate must not be one of them. The first version selected
itself out of the manifest and recursed; the CI job sat at 52 minutes before anyone looked at the
clock, because a hang produces no failure output and so does not look like a defect. Self-exclusion
is derived from `BASH_SOURCE` rather than written as a literal, and a `FR134_PARITY_RUNNING`
sentinel closes the indirect case — if some other in-scope gate ever invokes this one, path
exclusion is blind to it. Both are fixtures.

Three further checks come from the same observation that a gate has an environment:

- `check_job_dependencies` compares each gate's `command -v` preamble against what its job
  installs, through a declared package→command map. The runner baseline is the one input no
  repository scan can derive, so it is declared — and kept deliberately minimal, which makes the
  check strict rather than permissive.
- `check_workspace_scope` requires a gate running the whole workspace to exclude what the sibling
  `test` and `clippy` jobs exclude. DD-139 called `test-filesystem-trigger.sh`'s
  `cargo test --workspace` an accepted duplication. It was a superset, and the extra member is the
  one crate no job installs the Tauri dependencies to build.
- `check_diagnostics_preserved` forbids discarding the output of a cargo command whose failure the
  gate reports. The CI log that started this read `FAIL: cargo test --workspace` and nothing else.
- `check_git_history_available` requires a gate that queries history to run in a job that fetched
  any. This one was found by the diagnostics change on its first run, and it is the most
  consequential of the three: `test-agent-driver-production-parity.sh` proves FR-126's removal with
  `git cat-file`, `git merge-base --is-ancestor`, and a reverse `git apply` of the removal patch —
  the recorded baseline is reachable, the compatibility window is an ordered interval, the patch is
  mechanically revertible. That is the retirement-parity evidence the governance process requires
  before a removal counts as closed, and `actions/checkout` fetches one commit unless told
  otherwise, so all three had failed on every run and passed on every developer machine.

### CI liveness

`config/governance/ci-job-liveness.json` records the last real conclusion of **every job in every
push-triggered workflow**, and `scripts/qa/ci-liveness.rb` verifies it offline.

The scope distinction is the point. `qa-gate-surface.json` classifies `scripts/qa/*`, so
`boundary-coverage`, `test`, `clippy`, `miri` and `cross-compile` were never in it — and
`boundary-coverage` had been red for six consecutive runs. A liveness rule scoped to the gates it
already knew about would not have looked at it once.

Four rules:

1. every workflow file is recorded or excluded with a reason (`release.yml` is excluded: tag-only,
   so there is no last-run-on-`main` to assert);
2. every job of an in-scope workflow has a record, so adding a job fails until someone records it;
3. no non-success record without a `knownFailing` reference and reason — and no annotation left on
   a job that has recovered, so the next real failure is not pre-excused;
4. a record taken before its workflow last changed is **stale**, because it describes a pipeline
   that no longer exists.

Rule 4 is what stops this becoming the thing it was built to catch. It is also why the ledger was
red on the commit that introduced it: this FR rewrote `ci.yml`, so every record predated it, and
the file could not be made honest until a run existed on the new pipeline.

`--refresh` pulls from `gh run`, collapses matrix legs to their worst outcome, and refuses to run
unattended.

### One run, every diagnosis

The governance job's steps now record their outcome and continue; a final step fails on any of
them. A serial job stops at its first failure and reports nothing after it, which is how the
workspace-scope defect stayed invisible behind the ledger tooling's self-lock across two runs.

It paid for itself immediately. Its first real run printed nineteen outcomes and three of them were
red — the liveness ledger reporting its own staleness, the stale-claim scan tripping over this FR's
own QA document, and the agent-driver parity gate. That third one had been failing on every run
since it was wired, behind two earlier failures, and nobody had seen it. Under the old serial
arrangement each of those would have cost a separate push to discover.

The same run also showed why the aggregate has to be a separate step: `continue-on-error` makes
GitHub report a step's `conclusion` as success while `outcome` holds the truth, so a reader looking
at the step list alone sees green. The summary table is what makes the outcomes legible.

### Two defects this FR wrote, and how they were found

Both were invisible locally and appeared on the first real run, which is the
argument this FR makes about everything else, turned on its own output.

**`producer | grep -q` under `set -o pipefail`.** `grep -q` exits at the first match, the producer
takes SIGPIPE, and the pipeline reports the producer's death as its own status. `check_wiring_truth`
read `sed … | grep -qF "$path"` and announced that two gates were not called by their declared
invoker, with `sed: couldn't write 80 items to stdout: Broken pipe` sitting in the log next to the
accusation. Locally `sed` always finished first. Every such pipeline in these files is now a
here-string, which has no pipe and therefore no race.

**A test of environment handling that did not handle the environment.** The `CI=false` case cleared
`CI` and left `GITHUB_ACTIONS` set, which the runner exports — so the write guard kept refusing and
was right to. The test was wrong, not the guard. It now clears every indicator before setting the
one under test.

Neither is exotic. Both are the same shape as the defects this FR was filed about: something that
holds on a developer machine and does not hold where it runs.

### The lexer, and why the obvious fix is worse than the defect

Both ledgers decide what a `#[cfg(test)]` module covers by counting braces, and a brace inside a
string literal is not a brace. `.body("{")` leaves the counter above zero, the module's range runs
to end of file, and every production line after it disappears from the scan — silently, because
the hidden lines simply stop being counted and no number moves.

The obvious fix is to strip literals with a regular expression, line by line. That is worse. This
repository contains `r#"{"items": [` at `item_generate.rs:199`, a raw string spanning three lines.
A per-line matcher reads the closing line's `}` as code, decides that module ended 245 lines early,
and hands 7 lines of test fixture to the ratchet as production usage — moving
`capturesOrJsonPath` from 53 to 60. It trades under-counting for over-counting and the ratchet
moves for a wrong reason.

So the fix is a real lexer, and "the baselines do not change" is a genuine two-sided test rather
than a formality. All four coordination ratchets remain `53 / 30 / 9 / 0`; the core boundary
remains `200 / 37` and `52 / 924 / 143`.

The FR's characterisation was right and worth restating: the defect was **latent, not active**.
Every `cfg(test)` module in the scanned tree is a file-tail module, so the naive counter running
off the end lands on the same excluded range a correct scan would. Nothing was hidden. A new check
asserts no module fails to close, so it stays that way.

## Decisions and their alternatives

**Keep the text conditions on path-shadow.** They caught a real defect (the `export PATH` line
removed) and cost nothing. Removing them to prove a point about proxies would have deleted a
working signal; stripping comments first is what fixes them, and pairing them with an executed
assertion is what stops them being load-bearing.

**Declare the runner baseline rather than probe it.** What a GitHub runner image ships cannot be
derived from this repository. The list is minimal — anything a job could plausibly install is left
out — so the check errs strict, and the jobs going green is the proof.

**Exclude cargo-bearing gates from environment parity.** A second full workspace build per gate is
not worth it, and those gates already run under CI in the real job, which is the same observation.
The limit is written in the script header, not left to be discovered.

**Declare `test-agent-driver-execution-migration.sh`'s scope difference instead of aligning it.**
Its unexcluded workspace sits behind `FR126_FAST`, which CI sets, and the local macOS run is
currently the only place `orchestrator-gui` is compiled at all. Excluding it would have deleted
that crate's last coverage rather than aligned anything. Building it in CI belongs to FR-076.

**Delete the broken mirror symlink rather than declare its directory.** It resolves nowhere and
nothing references it. Declaring it would have made a root out of a mistake.

**Add three gates, against the FR's own non-goal.** FR-134 listed "do not grow the number of gates
in `scripts/qa/` or add new semantic assertions" as a non-goal, and its requirement 8 then asks for
a liveness ledger with a check, a discovery-based job list, and a `CI=1` parity run — none of which
can exist without new gates. The requirement is specific and the non-goal is general, so the
requirement won, exactly as FR-131's acceptance criterion beat its non-goal about navigation. What
the non-goal was protecting is intact: no existing script's `enforcement` classification changed,
and the surface grew only by the three files this FR had to write (53 → 56, 20 → 23 `ci-required`).

## Known limits

- **The stale-claim scan does not catch semantic drift.** Verified rather than assumed: writing
  `monotonic legacy-coordination ratchet` back into `docs/architecture.md` and running the gate
  passes. It matches a script name beside enforcement wording, and a claim about ratchet semantics
  has neither. Extending it would mean a blacklist of domain words, and "monotonic" still appears
  correctly in 22 tracked files — ten of them about fencing tokens, delivery cursors and change
  streams, the rest describing this very change — so such a list would be noise on arrival. The
  eight statements FR-134 owed were fixed by hand.
- **Dependency and history checks are per *declared* job.** `test-ci-environment-parity.sh` runs
  gates that are declared against `governance`, so nothing verifies that *its* job provides what
  those gates need — the parity job was given full history by hand for exactly this reason, not
  because a check demanded it. Modelling "which jobs actually execute which gates, transitively"
  would need the workflow model to follow invocation through shell, which is where this stopped.
- **The runner baseline is an enumeration.** It is the one input that cannot be discovered, and it
  is exactly the shape this FR spent its length removing everywhere else. Mitigated by minimality
  and by CI being the proof, not eliminated.
- **`check_diagnostics_preserved` reads source.** Paired with a behavioural case that runs the gate
  under a `cargo` failing with a recognisable compiler error and requires it to reach the output,
  but the source rule itself is still a rule about text.
- **Liveness freshness is keyed to the workflow file.** A job whose *gate* changed while its
  workflow did not keeps a record that is formally fresh. Keying to any repository change would
  make every commit stale the ledger and the annotation would become permanent noise.
- **`--refresh` needs `gh` and network.** Verification does not; only the refresh path does, and it
  refuses to run unattended.
- **Environment parity compares exit codes, not output.** A gate that passes in both worlds while
  doing different work in each is not detected.
- **Environment parity is slow** — around ten minutes, because it runs a dozen gates twice and
  several of them assemble fixture repositories per case. It has its own job so it costs wall clock
  only against the other long job rather than adding to it, but it is the most expensive check
  added here and the first place to look if CI time becomes a problem.
- **The `stat` wrapper in the Slack certification library was wrong on Linux for its whole life**,
  and the only reason it is fixed here is that repairing the ripgrep gap let that gate reach its
  assertions on ubuntu for the first time. There is no general check for BSD-vs-GNU divergence in
  the gates; parity covers the environment variable axis, not the platform axis. The `runs-on`
  matrix is the only thing that would have caught it, and only once the gate could run.
