---
lifecycle: active
related_fr: FR-151
---

# Orchestrator - Release 0.4.0, The Phantom v0.3.1 Reconciliation, And Tag Liveness

**Module**: Release / Distribution
**Scope**: the 0.4.0 version cut across all 14 workspace manifests, the
`[Unreleased]` liquidation in `CHANGELOG.md`, the new ci-required gate
`scripts/qa/test-release-tag-liveness.sh` with its three fixtures, the
hardened idempotency match in `.github/workflows/release.yml`, and the
one-shot full-chain verification of the 0.4.0 release itself
**Scenarios**: 5
**Priority**: High

## Background

v0.3.1 was a phantom release: the tag reached the remote, every manifest said
0.3.1, and the release pipeline never ran — GitHub Releases, crates.io and
the Homebrew tap stayed at 0.3.0 for four months. FR-151's verification
disproved the FR's original hypothesis (no workflow in this repository pushes
tags) and established the strong-evidence mechanism: GitHub creates no
trigger events for a push carrying more than three tags, and 38 `checkpoint/*`
tags had reached the remote — the signature of a `git push --tags` sweeping
the release tag up with a batch of local editor checkpoints. The event log
ages out after 90 days, so the mechanism is recorded as inference, and the
repair does not depend on it (DD-162).

Scenarios 1–4 are repeatable regression checks. Scenario 5 is the recorded
one-shot execution of the 0.4.0 release — the first real execution of the
publish pipeline since v0.3.0, and the boundary DD-161 recorded ("the publish
end-to-end has still never run") being crossed. Its evidence lives in DD-162
and does not re-run.

## Scenario 1: Release tag liveness gate (real mode)

**Steps**
1. `bash scripts/qa/test-release-tag-liveness.sh; echo "exit=$?"`

**Expected result**
- Exit 0, summary `1 passed, 0 failed`.
- The single PASS line names the highest semver-ordered remote `v*` tag and
  the evidence class: a GitHub Release, a release.yml run for an in-flight
  tag, or the one closed historical exemption (`v0.3.1`, DD-162). After the
  0.4.0 release exists the exemption line must no longer appear — `v0.4.0`
  outranks `v0.3.1` in the semver sort, so the exemption is dormant, not load-bearing.

## Scenario 2: Release tag liveness gate (fixture mode)

**Steps**
1. `bash scripts/qa/test-release-tag-liveness.sh --fixture-test; echo "exit=$?"`

**Expected result**
- Exit 0, summary `3 passed, 0 failed`, and all three lines present:
  1. the phantom signature (a tag with no Release and no run, simulated by a
     stubbed `gh`) fails with a diagnostic naming `v9.9.9`;
  2. an API outage fails closed with a diagnostic (never a skip — an empty
     read and a healthy history are not the same colour);
  3. the healthy positive control passes, proving the two red fixtures fail
     through the checks rather than through a broken harness.

## Scenario 3: Publish-surface gate still green after the version cut

**Steps**
1. `bash scripts/qa/test-release-publish-surface.sh; echo "exit=$?"`
2. `bash scripts/qa/test-release-publish-surface.sh --fixture-test; echo "exit=$?"`

**Expected result**
- Both exit 0 (`4 passed, 0 failed` and `5 passed, 0 failed`, including the FR-151 packaged-source containment check): the 0.4.0
  version bump and the release.yml idempotency-grep hardening changed neither
  the publish list's shape nor the shipped-target set, and the QA-199
  fixtures still fail on injected defects.

## Scenario 4: Workspace version consistency

**Steps**
1. `cargo metadata --format-version 1 --no-deps | jq -r '[.packages[].version] | unique | .[]'`
2. `rg -c 'version = "0\.4\.0"' Cargo.lock`

**Expected result**
- Step 1 prints exactly one version (the current workspace version; `0.4.0`
  at this document's writing) — 14 packages, no stragglers.
- Step 2 finds the workspace members' lock entries at the same version
  (14 at this document's writing; the exact count moves with the workspace).

## Scenario 5: 0.4.0 full-chain release execution (one-shot, recorded)

This scenario does not re-run. It records what was executed for the 0.4.0
release and where the evidence lives; the repeatable descendants are
scenarios 1–4 and the liveness gate in CI.

**Steps executed**
1. Manual first-publish bootstrap of the two crates crates.io had never seen
   (`orchestrator-config` → `orchestrator-collab` → `orchestrator-persistence`
   in dependency order, then `orchestrator-slack-gateway`) — crates.io
   Trusted Publishing cannot publish a brand-new crate, so the first release
   of each is manual by requirement.
2. Annotated tag `v0.4.0` on the release commit, pushed **alone**
   (`git push origin v0.4.0`, never `--tags`), followed by verification that
   a release.yml run appeared for the tag within minutes.
3. The release.yml run monitored to completion: 3 build targets, GitHub
   Release, Homebrew tap push, crates.io publish loop (bootstrapped crates
   skipped via the already-published match, the remaining 8 published via
   OIDC).
4. Post-release: all 12 publishable crates visible on crates.io at 0.4.0
   (checked individually, not sampled); `brew install` → `orchestrator
   --version` = 0.4.0 on Apple Silicon macOS; `install.sh` executed on
   Apple Silicon macOS and x86_64 Linux (container); GitHub Release carries
   all 3 target artifacts.

**Expected result**
- Every step above recorded as executed with its evidence (run URL, log
  paths, per-crate crates.io checks) in DD-162's Evidence section.

## Checklist

- [ ] `test-release-tag-liveness.sh` exits 0 with summary `1 passed, 0
      failed`, and `--fixture-test` exits 0 with `3 passed, 0 failed`; both
      summary lines are present in the captured logs
- [ ] the fixture diagnostics name their objects: `v9.9.9` for the phantom
      signature, "fail closed" for the API outage, and the positive control
      names `v9.9.8`
- [ ] `cargo metadata` reports exactly one version across all 14 workspace
      packages
- [ ] the gate is `ci-required` in `qa-gate-surface.json`, invoked twice by
      ci.yml's governance job with `GH_TOKEN`, both step ids read by the
      `OUTCOMES` aggregation, and both steps carry `pendingMeasurement`
      entries until the next cost refresh
- [ ] after the 0.4.0 release exists: the gate's PASS line cites the
      `v0.4.0` GitHub Release, not the `v0.3.1` exemption
