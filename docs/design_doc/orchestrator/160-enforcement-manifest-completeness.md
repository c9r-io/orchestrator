---
lifecycle: active
related_fr: FR-147
---

# DD-160: the enforcement manifest is only as good as its completeness

**Status**: Released
**FR**: FR-147
**QA**: `docs/qa/orchestrator/198-enforcement-manifest-completeness.md`

## The problem

FR-143 established a rule this repository now applies widely: a scanner derives
its **scope** from `config/governance/qa-gate-surface.json` rather than listing
the files it guards, because "a hand-listed set guards exactly what was known the
day it was written, and the next instance lands outside it silently".

That inversion moves the enumeration rather than removing it. The manifest is now
the list, and nothing was comparing it against what CI actually runs.

Measured at `9fa37e37`: **three shell gates were executed by `ci.yml` and had no
entry in the manifest.**

| gate | executed by |
|---|---|
| `scripts/qa-doc-lint.sh` | `ci.yml:governance` |
| `scripts/coverage-governance.sh` | `ci.yml:boundary-coverage`, `ci.yml:coverage-policy-fixtures` |
| `scripts/check-async-lock-governance.sh` | `ci.yml:async-lock-governance` |

Consequently `jq-status-observed.rb` and `fixture-target-drift.rb`, both of which
derive scope from the manifest's ci-required shell gates, had **never once read
any of the three**. The most pointed instance:
`test-agent-driver-documentation-alignment.sh` declares `qa-doc-lint.sh` as its
`invokedBy` — the callee was governed while its caller was not.

The structural reason none of the existing checks could see this: check 1
(`check_surface_complete`) derived its disk set from `find scripts/qa`, and all
three live in `scripts/`, not `scripts/qa/`. Check 3 (`check_wiring_truth`) asks
the opposite question — whether each *declared* entry is really executed — and
says nothing about a script nobody declared.

> **Updated by FR-158.** Check 1's root is now `scripts`, not `scripts/qa`, and
> its extension set includes `.mjs`. FR-158 measured what the narrow root was
> still hiding after this FR closed: 28 of 122 tracked scripts, among them every
> shared library the ci-required gates source — `rust_source.rb`,
> `workflow_model.rb`, `gate_jq.sh` and nine more. This FR's own reasoning
> applies to that set unchanged; the gates were governed and the engine they run
> on was not. The account above stands as the state at this FR's close.

## What the numbers actually are

The FR reported the difference set as three. Under its own stated method — derive
from `run_commands` for "every workflow job" — it is **six**. The three it named
are exactly the `ci.yml` subset; the other three are release tooling in
`release.yml`. Derived two ways, which agree:

| route | executed | undeclared |
|---|---|---|
| `workflow_model.rb` over `WorkflowModel.workflows` | 36 `.sh` (42 incl. `.rb`) | 6 |
| raw `grep -ohE` over `.github/workflows/*.yml` | 36 `.sh` | 6 |

That the two routes agree also establishes something narrower and useful: no
script in this repository is *only* mentioned in a comment or a heredoc. The
model and the grep would disagree if one were.

The reverse difference set is **7**, every one carrying `invokedBy`, run by its
own wrapper — the FR's claim here survived verification. `check_wiring_truth`
already governs that direction including the `invokedBy` chain, so the new check
covers the forward direction only.

DD-157's known limit #2 recorded "three scripts executed by `ci.yml`" and is
correct as written, because it is scoped to `ci.yml`. It is now closed.

## The scope decision

The new check reads **every** workflow, not `ci.yml`.

Narrowing it to `ci.yml` — where all the known gaps were — would be §4.4 shape 2
aimed at the check itself: it would guard the one workflow its author had in mind
while the next instance landed silently in another. Two of the four workflows
here already run governance gates (`security.yml` runs the dependency policy
gate, `docs.yml` the publishing integrity gate), so the wider scope is not
hypothetical.

## The exemption, and why it is not a blanket

Three of the six undeclared scripts are release tooling: they build or publish an
artifact and assert nothing. They are declared in `supportFiles` under a third
role, `release-tooling`, because that array already exists to say "this file is
not a gate, here is its role and its reason" and is already enforced by
`check_support_files_declared`.

Two deliberate constraints, both answering §4.4 shape 8 — an exemption written as
a subtree guards nothing, absorbs instances that do not exist yet, and never
produces a line in any log:

1. **Per path, never a directory or a glob.** Three entries with three reasons.
2. **Conditional on the trigger, and the condition is derived.** A
   `release-tooling` entry fails the moment any workflow that executes it runs on
   a branch push or a pull request. Computed from each workflow's parsed trigger
   map (`development_triggered?`), not from a list of workflow names:
   `release.yml` is `push: {tags: [v*]}` plus dispatch, while `ci.yml`,
   `docs.yml` and `security.yml` all carry branch pushes or pull requests.

A third constraint closes the cheap bypass. Of the three support roles, `fixture`
and `library` both state the file is never invoked as a gate itself; only
`release-tooling` describes a file a workflow runs at top level. So a **directly
executed** script declared under any other role fails. Without this, silencing
the check costs one word: relabel a governance gate `library` and it is declared,
exempt from the trigger rule, and never examined again. That is the mutation
fixture 26 applies, and it is cheaper than the deletion fixture 25 applies.

## `coverage-governance.sh`: a declaration the checker cannot see

This entry is the one place FR-147 could have added a false assurance, and it is
worth recording in full because the shape generalises.

`check_provider_isolation` verifies a `no-provider` claim by grepping **the
gate's own shell text** for the fixture bundles it names. That is sound for a
script whose provider exposure is a bundle it applies. `coverage-governance.sh`
runs `cargo llvm-cov --workspace --all-targets --all-features`, `npm run
test:coverage` and `npx playwright test`: anything it could reach is named in
Rust or TypeScript, so the grep finds nothing, the loop reads zero rows, and the
check passes having verified nothing.

The second-order effect is worse than the first. Declaring `no-provider` marks
the gate "not provider-capable", which **suppresses**
`check_provider_stub_coverage` for `boundary-coverage` — a job that installed no
provider stubs. The declaration would have removed the only backstop that could
have contradicted it.

So FR-147 added `./.github/actions/provider-stubs` to both `boundary-coverage`
and `coverage-policy-fixtures`. The stubs exit 97 loudly on any real
`claude`/`codex`, which is §4.4's rule applied literally: a proxy may be an
additional condition, never the only one, and the thing that observes the fact
itself here is running the job behind a stub that fails if the real binary is
reached.

## Known limits

1. **The `no-provider` branch still reads a shell script to answer a question
   about a whole test suite.** The stubs make the claim observable at run time on
   the two coverage jobs; they do not make `check_provider_isolation` able to
   verify it statically. Any future `no-provider` entry whose gate compiles and
   runs the workspace inherits the same gap, and the stub installation is what
   has to be checked, not the declaration. `check_provider_stub_coverage` does
   not require stubs for a `no-provider` gate — by design, or every grep-only
   gate would demand them — so this pairing is currently a decision recorded
   here rather than a rule a gate enforces.
2. **`fixture-target-drift.rb` cannot see through a local alias.** It recognises
   the landing proof by a statement's leading word (`fixture_mutate`,
   `fixture_premise`, `fixture_produce`). `test-qa-gate-surface.sh` calls it
   through `inject()`, and the roughly thirty older call sites escape the rule
   only because they rewrite with `perl -pi -e`, which its in-place pattern does
   not match. FR-147's three fixtures use `ruby -e`, which does match, and were
   reported as unproven mutations until they called `fixture_mutate` directly.
   Two consequences worth naming: an `if inject ... sed -i` site would be
   reported today and an `elif fixture_mutate` site is reported regardless of
   correctness, since `WRAPPERS` matches `if <fn>` and not `elif <fn>`. The gate
   is right that these need proving; it is the alias and the `elif` it reads
   wrongly.
3. **The fixture `BASE` tar list was itself a stale enumeration**, and this FR
   tripped it: it copied `scripts/qa-doc-lint.sh` but neither of the other two
   gates outside `scripts/qa`, so the new check would have reported them
   undeclared in every case — reading as "the check works" while every fixture
   below it failed for the fixture's own reason. It is now derived from the
   manifest, and fails loudly if that derivation yields nothing.

## What was measured rather than assumed

FR-147 §2 asserted that adding these entries would light up two unrelated gates,
and gave that as the reason FR-145 deferred the work. Measured in a scratch
worktree with the entries added, before any of them was committed:

| consumer | before | after |
|---|---|---|
| `jq-status-observed.rb` | PASS, 36 scanned | PASS, 39 scanned, 0 findings |
| `fixture-target-drift.rb` | PASS, 31 gates | PASS, 34 gates |
| `test-jq-status-observed.sh` | — | PASS 18/18 |
| `test-fixture-target-drift.sh` | — | PASS 18/18 |
| `test-qa-gate-surface.sh` | — | PASS 13/13 |
| `ci-cost.rb` | PASS | PASS |

No staged migration was needed. Both scanned sets grew by exactly three, which is
also FR-147's fourth acceptance criterion satisfied at the moment of the edit
rather than argued for afterwards.

## Cost

`check_workflow_execution_declared` spawns one Ruby process to derive the
executed set and at most one per distinct workflow to classify triggers — five
per invocation in this repository. The trigger classification was originally
asked per (exemption, record) pair, inside a nested loop, which the fixture mode
runs some thirty times; it is hoisted for that reason. The `governance` job
carries a recorded 2700s budget in `config/governance/ci-cost.json`.

Note for whoever refreshes that ledger: at the time of writing the budget reports
`NOT ENFORCED`, because FR-149's `QA doc lint workflow-ID scope negative
fixtures` step has never run and has no measurement. Three new ci-required
entries do not change that, but the ceiling binds again on the refresh that
measures them, and this FR added three fixtures to the most expensive gate in
that job.
