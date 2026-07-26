---
lifecycle: active
related_fr: FR-135
---

# DD-146: bash 3.2 Compatibility And The Coverage Main Path

**Module**: CI / Governance
**Status**: Implemented (FR-135)
**Related Plan**: FR-135
**Related QA**: `docs/qa/orchestrator/184-bash32-compatibility.md`
**Related**: DD-139 (QA gate enforcement surface), DD-145 (gate surface execution truth), DD-144 (doc lifecycle governance)
**Created**: 2026-07-26
**Last Updated**: 2026-07-26

## Background

`scripts/coverage-governance.sh` built an optional flag as an array:

```bash
branch_args=()
...
cargo llvm-cov --workspace --all-targets --all-features \
  "${branch_args[@]}" --json --output-path "$OUTPUT_DIR/rust.json"
```

CI pins `dtolnay/rust-toolchain@stable`, so `branch_args` stays empty. bash 4.4
and later expand an empty array under `set -u` to zero words. bash 3.2 rejects
it:

```
[coverage] collecting instrumented Rust tests
./scripts/coverage-governance.sh: line 38: branch_args[@]: unbound variable
```

The `boundary-coverage` job is `runs-on: macos-latest`. It died on that line on
every run it ever had, from the commit that introduced the job onward — 77
commits by the time FR-135 was worked. No coverage was ever compared. The gate
was wired and never guarded.

Two things kept it invisible for that long.

**The `##[error]` pointed somewhere else.** The upload step ran under
`if: always()` with `if-no-files-found: error`, so when generation failed the
summary's only error read `No files were found with the provided path:
target/coverage-governance/`. The real failure was the step above, and a reader
skimming the summary saw a missing artifact.

**The sibling job covers a disjoint path.** `coverage-policy-fixtures` runs
`./scripts/coverage-governance.sh --fixture-test`, and line 16 of that script is
`exec node scripts/coverage/test-coverage-governance.mjs`. The process is
replaced; nothing below line 16 is reached. Two jobs, one script, and the green
one said nothing at all about the red one.

### A corrected premise

The FR's diagnosis was accurate — the root cause, the line numbers, the disjoint
paths and the masking upload step all reproduced exactly, and the run log
(`30169894711`, line 512) matches its quoted error verbatim. Four corrections
were needed before implementing.

- **The interpreter does not come from a `shell:` declaration.** The FR
  attributed the 3.2 resolution to the workflow's `shell: /bin/bash -e`.
  `ci.yml` has no `defaults` block and the step declares no `shell:`. The
  interpreter comes from the script's own `#!/usr/bin/env bash` shebang, which
  the error prefix (`./scripts/coverage-governance.sh: line 38:`) confirms. This
  is not pedantry: adding `shell: bash` to the step would have fixed nothing,
  and the exposure is not one step but every shell file any macOS job invokes.
- **`BASH_COMPAT=3.2` cannot simulate this.** Measured against bash 5.3, both as
  an environment variable and as an inline assignment: empty-array expansion,
  `declare -A`, `mapfile`, `${x^^}`, `local -n`, `wait -n` and `shopt -s
  globstar` all still succeed. There is no way to run the semantic half of this
  check on a Linux runner, which decides where the gate is hosted.
- **`mapfile` is not only historical.** The FR named it as a shape encountered
  during FR-126. Four calls were still live in
  `.claude/skills/security-test-doc-gen/scripts/extract_surface.sh`, and a fifth
  bash-4-only construct the FR did not mention — `declare -A` — sat in
  `scripts/qa/test-coordination-strangler.sh`.
- **One shape the obvious rule would flag is not a hazard.** `${!a[@]}` on an
  empty array is fine in bash 3.2, and so is `${#a[@]}`; only the value
  expansions `${a[@]}` and `${a[*]}` are not. `scripts/regression/lib/probe-runner-lib.sh`
  uses the index form on arrays initialised empty and must not be "fixed".

## Design

### The rewrite

Every expansion of a possibly-empty array becomes the guarded form:

```bash
cargo llvm-cov ... ${branch_args[@]+"${branch_args[@]}"} --json ...
```

Thirty-five sites across sixteen files. The inner expansion stays quoted, so
elements containing spaces survive — verified, because the mechanical rewrite
touched argument arrays and file lists. `declare -A PRODUCTION` became a `case`
lookup function, and the four `mapfile` calls became `while IFS= read -r` loops.

### One spelling, enforced

`scripts/qa/bash32-compat.rb` blanks the exact canonical string before looking
for bare expansions. A second accepted spelling would mean parsing arbitrary
`${x[@]+...}` bodies with their nested braces, which is the brace counting that
has broken gates in this repository before. Enforcing one spelling is cheaper
and stricter.

### Invocation, not mention

The builtin rules first matched the bare word anywhere. That reported the
gate's own wrapper for containing the string `mapfile` inside the path
`$WORK/hazard/mapfile.sh`. The rules now require command position — start of
line, or after `;`, `&`, `|`, a bracket, or a shell keyword. The subject is
whether a builtin runs, and a path is not an invocation.

The same reasoning covers the wrapper's fixtures. This file's gate scans the
wrapper that tests it, so every fixture is written from a here-document: a
here-document body is data to the enclosing script, and case 7 asserts the
scanner reads it that way.

### Coverage walked, not listed

`git ls-files '*.sh'` — 95 files, including the `.claude/skills/**` templates,
which is where three of the five bash-4-only constructs lived. There is no
exemption list. A roster guards exactly what existed when it was written.

### Where the executed half lives

Because `BASH_COMPAT` cannot restore the semantics, the fixture corpus only
proves anything where `/bin/bash` really is 3.2. That is the macOS leg of
`coverage-policy-fixtures`. On Linux the wrapper reports those cases as
`SKIPPED` and prints a warning rather than counting them as passes, and case 9
reads `ci.yml` with a YAML parser to assert that some job whose `runs-on`
resolves to macOS actually runs this script. Without that case, a green ubuntu
run would mean the executed half had been skipped everywhere and nobody would
know.

### The two coverage jobs, stated

| Job | Path | What it proves |
|---|---|---|
| `coverage-policy-fixtures` | `--fixture-test`, which `exec`s node on line 16 | the JavaScript policy engine: baseline comparison, unsupported-branch handling, path fixtures |
| `boundary-coverage` | the shell main path, lines 19-87 | that coverage is actually collected on a real toolchain and compared against the approved baseline |
| `test-coverage-governance-mainpath.sh` (new) | the shell main path with the toolchain stubbed | that the main path is reachable and assembles the right argv, in seconds rather than in the heavy job |

The first two cannot substitute for each other: they share a file and no lines.
The third is not a substitute for `boundary-coverage` either — it stubs cargo,
so it collects nothing and compares nothing. What it does is move the failure
mode that actually occurred, a shell error before the first real command, into a
job that finishes in seconds and runs on both platforms.

### Diagnostic fidelity

`if-no-files-found` becomes `warn`. `always()` stays, so artifacts still upload
when the *comparison* fails, which is when they are most worth reading. The
generation step fails the job by itself; the upload step has no failure to add,
and the one it was adding pointed at the wrong step. A case in the main-path
wrapper reads the parsed workflow and rejects any `upload-artifact` step that
combines the two settings again — the pair is what causes this, and grepping for
either key alone says nothing about the pair.

## Verification by mutation

Eighteen defects, applied one at a time to the committed gate, each reverted
before the next:

| Mutation | Defect introduced | Caught by |
|---|---|---|
| M1 | the empty-array rule is not applied | compat 3, compat 8 |
| M2 | the scanned set is filtered by a roster | compat 2 |
| M3 | the `declare -A` rule never matches | compat 4/associative, compat 7 |
| M4 | the `mapfile` rule never matches | compat 2, compat 4/mapfile |
| M5 | the `${x^^}` rule never matches | compat 4/case-conversion |
| M6 | the `local -n` rule never matches | compat 4/nameref |
| M7 | the `wait -n` rule never matches | compat 4/wait-n |
| M8 | the `globstar` rule never matches | compat 4/globstar |
| M9 | `${#a[@]}` and `${!a[@]}` are treated as hazards | compat 1, compat 6 |
| M10 | comments are scanned as code | compat 7 |
| M11 | here-document bodies are scanned as code | compat 1, compat 7 |
| M12 | files that fail `bash -n` are skipped | compat 8 |
| M13 | builtins match on mention, not invocation | compat 1 |
| M14 | the macOS CI step is removed | compat 9 |
| M15 | **the original defect, restored** | mainpath 1, mainpath 2 |
| M16 | `branch_args` is dropped from the argv entirely | mainpath 3 |
| M17 | `--fixture-test` no longer short-circuits | mainpath 5 |
| M18 | `if-no-files-found: error` returns | mainpath 6 |

M15 is the one worth naming: putting `"${branch_args[@]}"` back makes the
main-path wrapper fail under `/bin/bash` on this host, which is the acceptance
criterion "reproduces before, does not reproduce after" as an executable test
rather than a note.

Two cases are isolated by no mutation, and neither can be. Compat case 5
observes bash itself rather than the gate, so no change to the gate can make it
fail; it is what makes the other cases mean anything. Main-path case 4 asserts
the `COVERAGE_BRANCH_MODE=required` refusal, which no mutation in this set
touches.

## A second blocker, visible only once the first was gone

With the expansion fixed, the job ran for two and a half minutes into
`cargo llvm-cov --workspace --all-targets --all-features` and then failed on
something else entirely:

```
error: proc macro panicked
   --> crates/gui/src/lib.rs:158:14
    |
158 |         .run(tauri::generate_context!())
    = help: message: The `frontendDist` configuration is set to
            `"../../gui/dist"` but this path doesn't exist
```

`tauri::generate_context!` reads `frontendDist` at compile time. The job ran
`npm ci` and installed a Playwright browser but never built the bundle, so
`orchestrator-gui` could not compile there. This had always been true; it was
unobservable because the shell error ended the step in about a second, before
any crate was compiled. A `npm run build` step now precedes coverage
collection.

This is worth stating plainly: a job red for one reason can hide a second
reason indefinitely, and "the first error is fixed" is not the same claim as
"the job works". Only the run decides that, which is why FR-135's acceptance
criterion is an observed run rather than a local pass.

## A defect the fixtures had

Case 8 claimed to test "a script that does not parse". Its fixture was

```bash
if [ -z "$1" ; then
```

which `bash -n` accepts: `[` is a command, `;` ends it, `then` follows. The file
parsed. The case was passing on a premise that was not true, and the mutation
that should have isolated it — skipping files that fail `bash -n` — survived.
The fixture now uses an unmatched `done` and the case asserts up front that
`bash -n` rejects it, so the fixture cannot quietly stop being what it says.

Case 7 had the same shape: its comment fixture used a construct the rules only
match in command position, so a comment could never have matched it and
disabling comment stripping changed nothing. It now mentions an expansion, which
is position-independent.

## Consequences

### What this establishes

- The `boundary-coverage` job reaches the coverage comparison on the macOS
  runner. Observed: run `30182768742`, `coverage governance passed`, a 3.9 MB
  artifact uploaded, the job's first success.
- Every tracked shell file is checked for constructs bash 3.2 rejects, on a set
  derived from git rather than declared.
- The rules are executed, not just matched: each class is run under a real bash
  3.2 and must fail, and its prescribed replacement must succeed.
- The shell main path of `coverage-governance.sh` is exercised on both platforms
  in a job that takes seconds.
- A generation failure is the first error a reader sees.

### Accepted costs

- The guarded form is noisier than `"${arr[@]}"`. It is applied uniformly rather
  than only where an array is provably emptyable, because "provably" is a
  flow-sensitive claim and the gate is not a shell interpreter.
- `test-coverage-governance-mainpath.sh` stubs six commands. Stubs drift from
  the tools they stand in for; the argv assertions are exact, so drift shows up
  as a failure rather than as a silent pass.

### Known limits

- The scan reads shell files. `run:` blocks inside `.github/workflows/**` and
  the `./.github/actions/provider-stubs` composite action are also executed by
  bash on macOS runners and are **not** covered. They are short and currently
  free of these constructs, checked by hand during FR-135, but nothing enforces
  that.
- Emptiness is decided per file, flow-insensitively: an array assigned `=()`
  anywhere makes every value expansion of that name a finding, even where an
  earlier guard has already proved it non-empty.
  `test-agent-driver-documentation-alignment.sh` is such a case and was rewritten
  anyway, since the guarded form costs nothing.
- The comment scanner tracks quoting per line. A quote opened on one line and
  closed on the next is read as unbalanced, and a `#` after it may be treated as
  a comment when it is not.
- bash 3.2 has more incompatibilities than the seven classes here. These are the
  ones this repository has actually hit; a new one arrives unguarded until it is
  added.
- Recovering this job exposed a property of `config/governance/ci-job-liveness.json`
  worth writing down: **the `governance` job cannot record its own recovery in
  one pass.** Its liveness step runs inside itself, so any commit that touches
  `ci.yml` makes the job's own record stale, the job goes red, and the refreshed
  record then says `failure` — which the next run reads and fails on again. The
  `knownFailing` annotation is the designed escape and converges in two steps,
  which is what FR-135 used; it is not a defect so much as a shape a reader
  should know before assuming the annotation means something is broken.
