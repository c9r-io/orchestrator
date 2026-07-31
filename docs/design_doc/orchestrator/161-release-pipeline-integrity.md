---
lifecycle: active
related_fr: FR-150
---

# DD-161: Release Pipeline Integrity — The Publish Surface Is Derived, Not Remembered

**Status**: Implemented (FR-150)

## Problem

The release pipeline last actually executed for **v0.3.0** (2026-04-04).
v0.3.1 is a phantom: the tag sits on the remote and the CHANGELOG records
`[0.3.1] - 2026-04-06`, but release.yml has no run for that tag — GitHub
Releases, crates.io and the Homebrew tap all stop at 0.3.0 while all 14 crate
manifests say 0.3.1. (Found by this FR's own QA scenario 4, whose first draft
rendered the formula against v0.3.1 and got a 404. The likely mechanism is a
tag pushed in a context that does not trigger workflows; establishing it and
reconciling the phantom is FR-151's business, recorded there.) In the four
months since the last real release the workspace grew two crates by extraction — `orchestrator-persistence`
(FR-130) and `orchestrator-slack-gateway` (FR-141) — and release.yml's
hand-typed crates.io publish loop never learned about either. Both are normal
dependencies with `version = "0.3.1"` pins: `core` requires the first,
`orchestratord` the second. The next `cargo publish core` would have failed on
"no matching package named `orchestrator-persistence`" — a message the loop's
`already exists` idempotency fallback does not match — in a job that runs
`needs: publish`, i.e. **after** the GitHub Release and the Homebrew tap push
had already succeeded. A half-published release, from the one failure mode a
release pipeline exists to prevent.

The shipped-target surface carried the mirrored defect. release.yml builds
three targets; `install.sh` composed whatever `uname` reported, so an Intel
Mac produced `x86_64-apple-darwin`, an artifact URL that has never existed,
and died inside `curl -fsSL` under `set -eu` with no diagnosis. The Homebrew
formula's `on_macos` block had an `arm?` branch and nothing else — an Intel
Mac received a formula with no `url` at all. The formula also declared
`license "Apache-2.0"` against a MIT repository. And the tap push routed
`TAP_GITHUB_TOKEN` through `dmnemec/copy_file_to_another_repo_action@main` —
the only action in the tree pinned to a moving branch, holding a cross-repo
token.

Every one of these is §4.4 shape 2 (a hand-listed set guards what was known
when it was written) landing on surfaces that only a real release executes —
which is why four months of green CI said nothing about any of them.

## Decisions

**Intel macOS is not supported** (option B, decided 2026-08-01). The remaining
Intel fleet does not justify a fourth build job on every release. The refusal
is explicit at both entry points: `install.sh` validates the composed triple
against `SUPPORTED_TARGETS` before touching the network and names the
`cargo install orchestrator-cli orchestratord` alternative; the formula's
intel branch says the same via `odie`. A silent 404 became a one-line answer.

**musl stays a gate and stays unshipped.** `x86_64-unknown-linux-musl` in the
cross-compile matrix is a portability proof — it keeps glibc-only code from
landing — not a distribution promise. The difference between the gated set and
the shipped set is now written down (ci.yml comment, CHANGELOG Known
Non-goals) instead of readable as an oversight.

**The tap push is inline.** `gh repo clone` + `git push` in a `run:` step,
every command auditable in the workflow, no third-party code holding the
token. Replacing the action rather than SHA-pinning it removes the trust
question instead of freezing it.

## Mechanism

`scripts/qa/test-release-publish-surface.sh` (ci-required, governance job,
with `--fixture-test`) derives every set it asserts:

1. **Publish loop** — the publishable crate set from
   `cargo metadata --no-deps` (`publish == null`), compared both directions
   against the loop extracted from release.yml, plus dependency-topological
   order checked against the real workspace edge list (normal deps only; dev
   and build deps do not constrain crates.io order). Extraction failing is a
   failed assertion with a diagnostic naming what moved, never a skip (§4.4
   shape 7).
2. **Shipped-target set** — release.yml matrix, install.sh
   `SUPPORTED_TARGETS`, and the formula's uncommented `url` stanzas must be
   identical. The formula extraction is anchored to line starts precisely so
   a commented-out stanza does not count as shipped — the bare substring grep
   would have been satisfied by the gate's own fixture 3 (§4.4 shape 1).
3. **Behavioral refusal** — the real `install.sh` runs under a stubbed
   `uname` reporting the platform that used to 404, with a sentinel `curl`
   on the stub PATH that fails loudly if the download path is ever reached.
   "Refuses before the network" is observed, not inferred from source text.

Three negative fixtures, each mutating a private copy of one file by
commenting out rather than deleting (the mutation extraction code is least
likely to catch), each required to fail with a diagnostic naming the injected
object, each isolated manually against base copies — a generic all-checks
sweep would couple check 3 to fixture 2's mutation, since the refusal's
behavior depends on install.sh's own support list.

Registration is threefold and all three are load-bearing: the
`qa-gate-surface.json` entry (scope for the derived scanners), the two
governance-job steps, and the `OUTCOMES` aggregation lines — a
`continue-on-error` step missing from `OUTCOMES` is a gate that cannot fail
the job. `test-qa-gate-surface.sh` now verifies that aggregation mechanically,
which was confirmed during this FR by watching it count the new steps.
Cost-wise both steps sit in `ci-step-cost.json` `pendingMeasurement` with
written reasons until the next refresh measures them.

## Known limits

- **The publish end-to-end has still never run.** The gate proves the list
  matches the workspace; it cannot prove crates.io accepts twelve crates in
  this order. The first real execution is FR-151's 0.4.0 release, and QA 199
  scenario 5 records that boundary explicitly rather than presenting the gate
  as end-to-end evidence.
- **The phantom v0.3.1 is diagnosed, not repaired, here.** Why the pushed tag
  produced no workflow run — and what 0.4.0 must do about a version number
  that crates.io has never seen while every manifest carries it — is FR-151
  scope. This gate would not have caught it either: it asserts the surfaces
  agree with the workspace, not that a tag push actually reaches the
  workflow. A liveness signal for the release workflow itself (last run vs
  last remote tag) would close that class; deliberately not built now.
- The `sleep 30` between publishes (release.yml) remains — five minutes of
  unconditioned waiting instead of index-propagation polling. Deliberately
  out of scope here; it wastes time, not correctness.
- The matrix extraction reads literal `target:` values anywhere in
  release.yml. Today only the build matrix has them; a second matrix with
  literal triples would join the asserted set and fail closed (surplus,
  loudly), which is the acceptable direction.
- `extract_publish_list` understands one loop shape. A refactor of the
  publish job must teach it the new shape — the premise failure names the
  file and the function, so the cost is a red gate with instructions, not a
  vacuous pass.

## Evidence

QA: [docs/qa/orchestrator/199-release-pipeline-integrity.md](../../qa/orchestrator/199-release-pipeline-integrity.md)
(scenarios 1–5, including the recorded end-to-end boundary). Gate green in
both modes at authoring; fixture mode `4 passed, 0 failed` including the
positive control. CHANGELOG entries under Fixed, Security, and Known
Non-goals in the same change.
