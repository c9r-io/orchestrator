---
lifecycle: active
related_fr: FR-150
---

# Orchestrator - The Release Pipeline Publishes What The Workspace Ships And Ships What It Advertises

**Module**: Release / Distribution
**Scope**: the crates.io publish loop and the tap-push step in
`.github/workflows/release.yml`, `SUPPORTED_TARGETS` and the
unsupported-platform refusal in `install.sh`, the intel/license stanzas in
`homebrew/orchestrator.rb`, and the new ci-required gate
`scripts/qa/test-release-publish-surface.sh` with its negative fixtures (three at FR-150; a fourth, packaged-source containment, added by FR-151 after the 0.4.0 publish loop failed on a crate-escaping `include_str!`)
**Scenarios**: 5
**Priority**: High

## Background

The release pipeline last actually ran for **v0.3.0** (2026-04-04). v0.3.1 is
a phantom release, found while writing this document's scenario 4: the tag
exists on the remote and the CHANGELOG records `[0.3.1] - 2026-04-06`, but
release.yml has no run for it — GitHub Releases, crates.io and the Homebrew
tap all stop at 0.3.0, while every crate manifest says 0.3.1. In the four
unexercised months since,
two crates (`orchestrator-persistence`, FR-130; `orchestrator-slack-gateway`,
FR-141) were extracted into the workspace as normal dependencies of crates the
publish loop does publish — and never added to the loop. The next tag would
have failed on "no matching package" **after** the GitHub Release and Homebrew
push succeeded, leaving a half-published release. In the other direction,
`install.sh` composed target triples (`x86_64-apple-darwin`) that release.yml
has never built and died in a bare curl 404.

Both defects are §4.4 shape 2 — a hand-typed list guarding exactly what was
known the day it was written — applied to the release surface. The remedy is
one gate that derives every set it asserts: the publishable crate set from
`cargo metadata`, the dependency order from the real edge list, and the
shipped-target set compared across the three surfaces that each name it.

Decisions recorded (FR-150): Intel macOS is not supported (behavioral refusal,
not a fourth build job); musl stays a cross-compile portability gate and is
deliberately not shipped. Design record at closure:
`docs/design_doc/orchestrator/161-release-pipeline-integrity.md`.

**Safety**: read-only against the working tree. The gate runs `cargo metadata`
(no build), reads three files, and executes `install.sh` under a stubbed
`uname` with a sentinel `curl` — no daemon, no database, no network, no
provider binary. Scenario 4 is the exception and says so: it downloads one
checksum manifest from the public v0.3.0 GitHub Release — the last release
that exists; v0.3.1's does not (see Background).

## Why the assertions are shaped the way they are

**The publish list is compared against `cargo metadata`, never against a
restatement.** The defect being guarded was a list that only grew when someone
remembered; a QA step that hand-listed the expected crates would be the same
defect one layer up.

**The refusal is asserted behaviorally, with a sentinel on the far side.** A
grep for the `SUPPORTED_TARGETS` line would pass with the check commented out
(§4.4 shape 1). The gate runs the real script on the platform that used to 404
and plants a `curl` stub that fails loudly if the download path is reached, so
"refuses before the network" is observed, not inferred.

**Each fixture mutates by commenting out, not deleting** — the mutation the
extraction functions are least likely to catch — **and must fail with a
diagnostic naming the injected object**, because an exit code cannot
distinguish the intended branch from a fixture that died of its own premise
(§4.4 shape 7).

## Scenario 1: the gate passes on the real repository

Steps:

```bash
bash scripts/qa/test-release-publish-surface.sh > /tmp/qa199-s1.log 2>&1
echo "rc=$?"
tail -n 3 /tmp/qa199-s1.log
```

Expected result: `rc=0`; the log ends with the summary line `4 passed, 0
failed` (`3 passed` before FR-151 added the containment check), preceded by PASS lines (publish loop vs cargo metadata,
one shipped-target set, behavioral refusal). A missing summary line voids the
run regardless of exit code.

## Scenario 2: the negative fixtures reject injected defects for named reasons

Steps:

```bash
bash scripts/qa/test-release-publish-surface.sh --fixture-test > /tmp/qa199-s2.log 2>&1
echo "rc=$?"
cat /tmp/qa199-s2.log
```

Expected result: `rc=0` with summary `5 passed, 0 failed` (`4 passed` before FR-151 added fixture 4): the positive
control, a commented-out `crates/orchestrator-persistence` rejected with a
diagnostic naming that crate (isolated to the publish-loop check), an
install.sh triple release.yml does not build rejected naming
`x86_64-apple-darwin`, and a commented-out formula url stanza rejected naming
`aarch64-unknown-linux-gnu`.

## Scenario 3: the gate is wired into CI in all three registration points

Steps:

```bash
jq -r '.scripts[] | select(.path == "scripts/qa/test-release-publish-surface.sh") | .enforcement' \
  config/governance/qa-gate-surface.json
grep -c "test-release-publish-surface.sh" .github/workflows/ci.yml
grep -c "release-surface" .github/workflows/ci.yml
bash scripts/qa/test-qa-gate-surface.sh > /tmp/qa199-s3.log 2>&1; echo "rc=$?"
```

Expected result: `ci-required`; the script is invoked twice in ci.yml (real
and `--fixture-test`); `release-surface` appears at least 4 times (two step
ids, two OUTCOMES reads — the aggregation entry is what makes a
`continue-on-error` step able to fail the job); the surface gate itself exits
0, which asserts mechanically that the swallowed steps are aggregated.

## Scenario 4: the formula template renders with the corrected license and the intel refusal (network)

Steps:

```bash
scripts/update-homebrew-formula.sh v0.3.0 > /tmp/qa199-formula.rb
grep -n 'license "MIT"' /tmp/qa199-formula.rb
grep -n 'Hardware::CPU.intel?' /tmp/qa199-formula.rb
grep -n 'odie' /tmp/qa199-formula.rb
grep -c 'PLACEHOLDER' /tmp/qa199-formula.rb
```

Expected result: render succeeds against the published v0.3.0 checksum
manifest (proves `extract_sha` still matches the real manifest format);
`license "MIT"` present; the intel branch and its `odie` message present; `0`
remaining placeholders. v0.3.0 deliberately, not v0.3.1: the first draft of
this scenario used v0.3.1 and failed with a 404, which is how the phantom
release in Background was found — the tag is on the remote, the release never
happened. Do not "fix" this scenario back to the newest tag without checking
the release exists.

## Scenario 5: the end-to-end boundary is recorded, not claimed

Steps: inspection. Re-read the `crates-io` job in
`.github/workflows/release.yml` and confirm: the publish loop names
`crates/orchestrator-persistence` before `core` and `crates/slack-gateway`
before `crates/daemon`; the tap-push step contains no `uses:` of a third-party
action (inline `gh`/`git` only) and reads `TAP_GITHUB_TOKEN` solely as
`GH_TOKEN`.

Expected result: both orderings hold; no third-party action holds the token.
**Recorded boundary**: an actual `cargo publish` against crates.io cannot be
exercised from QA — the first real execution of this loop is FR-151's 0.4.0
release, and that release is the closing evidence for this scenario's residual
claim. Until then, "the publish succeeds" rests on the derived-set gate plus
this inspection, and this document says so rather than presenting the gate as
end-to-end proof.

## Checklist

- [ ] `test-release-publish-surface.sh` exits 0 with summary `4 passed, 0
      failed`, and `--fixture-test` exits 0 with `5 passed, 0 failed`; both
      summary lines are present in the captured logs
- [ ] the publish loop's crate set equals `cargo metadata`'s publishable set
      in both directions, and every workspace dependency edge points backward
      in the loop
- [ ] release.yml matrix, install.sh `SUPPORTED_TARGETS` and the formula url
      stanzas name one identical target set; `x86_64-apple-darwin` and
      `x86_64-unknown-linux-musl` are in none of them
- [ ] install.sh under a stubbed x86_64-Darwin `uname` exits non-zero, names
      the triple, offers `cargo install`, and the sentinel `curl` was never
      reached
- [ ] the gate is `ci-required` in `qa-gate-surface.json`, invoked twice by
      ci.yml's governance job, and both step ids are read by the `OUTCOMES`
      aggregation; both steps carry `pendingMeasurement` entries until the
      next cost refresh
