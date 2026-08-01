---
lifecycle: active
related_fr: FR-151
---

# DD-162: Release 0.4.0 — The Phantom v0.3.1 Reconciliation And Tag Liveness

**Status**: Implemented (FR-151)

## Problem

Between v0.3.0 (2026-04-04) and this FR, the repository shipped nothing while
believing it had. The `v0.3.1` tag sat on the remote, `CHANGELOG.md` recorded
`[0.3.1] - 2026-04-06`, and all 14 crate manifests said 0.3.1 — but
release.yml has no run for the tag, and GitHub Releases, crates.io and the
Homebrew tap all stopped at 0.3.0. Four months of governance work (FR-126
through FR-150) accumulated in `[Unreleased]`, reaching 116KB, and none of it
reached any published artifact. The release defects FR-150 later found stayed
invisible precisely because the pipeline never executed.

## Root cause

FR-151's original hypothesis — a workflow pushing the tag with
`GITHUB_TOKEN`, swallowed by GitHub's anti-recursion rule — was **disproven**
during Phase 2 verification: none of the four workflows, at 22eab222 or now,
pushes tags; and the push that put 22eab222 on `main` triggered CI, Docs and
Security normally, so that actor's push events were not being suppressed.

The strong-evidence mechanism is a different GitHub rule: **a push carrying
more than three tags produces no trigger events at all.** 38 `checkpoint/*`
tags — local Claude Code editor checkpoints, dated 2026-03-22 through
2026-04-05, all hours before the v0.3.1 tag commit — were present on the
remote, which only a `git push --tags` (or equivalent sweep) puts there. A
`git push --tags` at release time carries v0.3.1 together with every
checkpoint tag not yet on the remote, crosses the three-tag threshold, and
the tag lands silently. Corroboration: v0.2.8, also a lightweight tag, pushed
alone, triggered its release runs normally.

The claim is recorded as **strong-evidence inference, not certainty**: the
GitHub events API retains 90 days and the window had aged out, so the exact
push composition cannot be replayed. The repair was chosen to not depend on
the inference being right.

## Decisions

**The repair is procedural plus a gate, not a token change.** The FR's
suggested fixes (PAT, deploy key) address the disproven mechanism and were
not adopted. Instead: the 38 remote `checkpoint/*` tags are deleted (local
copies retained); the release procedure pushes exactly one tag
(`git push origin vX.Y.Z`, never `--tags`) and verifies a release.yml run
appears, with `workflow_dispatch` as the recorded fallback; and a liveness
gate watches the invariant from then on.

**The liveness gate observes the fact, not a proxy.**
`scripts/qa/test-release-tag-liveness.sh` (ci-required, governance job)
asserts that the highest semver-ordered remote `v*` tag has a GitHub Release
— the pipeline's terminal artifact — or, tolerating the in-flight window
right after a push, at least one release.yml run for the tag ref. A tag with
neither is the phantom signature and fails, naming the tag. API failure and
an empty tag read fail closed with diagnostics (§4.4 shape 5: zero readable
tags and a healthy history are not the same colour). The run's own
success/failure is deliberately not asserted: a failed run is loudly red in
the Actions tab already; the phantom's defining property is that nothing was
triggered at all. DD-161 recorded this exact class as "deliberately not
built now"; this FR was the time to build it.

**One closed historical exemption.** `v0.3.1` itself is exempt while it is
the latest tag — a one-element, dated, named set that can never absorb a
future instance (the §4.4 shape 8 test). The moment v0.4.0 exists, the
exemption goes dormant: v0.4.0 outranks it in the semver sort.

**`gh` joined the declared runner baseline.** The gate reads the GitHub API
via `gh` with the job's `GITHUB_TOKEN`. The enforcement-surface check
requires every command a gate's preamble demands to be provided by its job;
`gh` is preinstalled on GitHub's ubuntu runners and is now declared in
`commandSources.runnerBaseline` — the manifest is where such runner claims
are reviewable.

**First-publish bootstrap is manual by crates.io requirement.** Trusted
Publishing (OIDC) cannot publish a brand-new crate — crates.io requires the
first release of a crate to be published manually before a trusted publisher
can be configured. `orchestrator-persistence` and `orchestrator-slack-gateway`
had never been published (both extracted after v0.3.0), so the 0.4.0
procedure publishes them manually before the tag push, in dependency order
(`config` → `collab` → `persistence`; `slack-gateway` has no internal
dependencies), letting the CI loop skip them via its already-published match
and publish the remaining 8 via OIDC. The loop's match was hardened to accept
both crates.io wordings (`already exists` from the client-side index check,
`already uploaded` from the server-side rejection).

**The CHANGELOG liquidation is part of the release, not a formality.**
Unreleased entries are not historical records; 16 fragments falsified by
later work (stale ledger counts, tense drift, two "additive" migration
claims that m0029 and m0034 contradict, a glued bullet) were corrected
before the cut, by four independent per-section audits against the tree at
the release revision. The `[0.3.1]` section carries a permanent note that
the version never produced artifacts and its changes first shipped in 0.4.0.
crates.io never saw 0.3.1, so 0.4.0 publishes directly with no back-fill.

## Known limits

- **The root cause is inference.** If the true mechanism was something else
  that also produces no run and no event trail, the gate still catches its
  next occurrence — that is why the repair was designed not to depend on the
  diagnosis.
- **The liveness gate has a red window** for a `workflow_dispatch`-only
  release: between the dispatch starting and the Release being published,
  neither evidence class matches (dispatch runs carry `head_branch: main`,
  not the tag). Accepted: the window is minutes long, the fallback path is
  itself the recorded exception, and widening the match to dispatch runs
  would re-open the gap between "something ran" and "this tag was seen".
- **The gate reads the network in CI.** A GitHub API outage reads as a red
  governance gate, not a skip — chosen deliberately; the alternative
  (skip on outage) is §4.4 shape 5.
- The `sleep 30` between publishes (release.yml) remains, as recorded in
  DD-161 — it wastes time, not correctness.
- **The Linux binaries floor at glibc 2.39** (built on ubuntu-24.04 runners).
  Measured during the 0.4.0 verification: Debian bookworm (glibc 2.36)
  refuses `orchestrator` with `GLIBC_2.39 not found`; Ubuntu 24.04 runs it.
  Consistent with DD-161's musl decision (portability proof, not shipped);
  users below the floor have the `cargo install` path.
- **crates.io `orchestratord@0.4.0` is built from `92295c52`, one commit
  after the `v0.4.0` tag** (`f82f1ae0`). The publish loop's verify step
  exposed a defect the tag's tree carries: an `include_str!` escaping the
  crate root, which `cargo package` cannot ship. The fix (the manifest moved
  to `crates/daemon/assets/`, byte-identical content) landed as `92295c52`
  and the crate was published from it; the tag, the GitHub Release binaries
  and the tap formula stay on `f82f1ae0`, whose in-workspace build is
  unaffected by the defect. Functionally identical; recorded rather than
  hidden. The publish-surface gate's new check 4 (packaged-source
  containment, with a synthetic-crate fixture) keeps the class from
  recurring.

## Evidence

QA: [docs/qa/orchestrator/200-release-0-4-0.md](../../qa/orchestrator/200-release-0-4-0.md)
(scenarios 1–4 repeatable; scenario 5 records the one-shot 0.4.0 execution).
Gate green in both modes at authoring; fixture mode `3 passed, 0 failed`
including the positive control; enforcement-surface gate `14 passed, 0
failed` real / `37 passed, 0 failed` fixture after registration.

### 0.4.0 release execution record (2026-08-01, closure evidence)

- **Bootstrap first-publishes** (local, user token, dependency order):
  `orchestrator-config`, `orchestrator-collab`, `orchestrator-persistence`,
  `orchestrator-slack-gateway`, all `Published … v0.4.0 at registry
  crates-io`, each confirmed via the crates.io API before proceeding.
- **Tag trigger verification**: annotated `v0.4.0` on `f82f1ae0` (CI green),
  pushed alone; release.yml run **30682942802** appeared within ~40 seconds —
  the behavior the phantom never produced. No `workflow_dispatch` fallback
  was needed.
- **Run 30682942802**: three build targets, GitHub Release publish, and the
  inline Homebrew tap push all succeeded. The crates.io job published 7
  crates via OIDC, skipped the 4 bootstrapped ones through the hardened
  `already exists|already uploaded` match (observed live in the log), and
  failed on the twelfth — `orchestratord` — on the packaged-source
  containment defect recorded in Known limits; fixed at `92295c52` (CI
  green) and published locally from that commit.
- **crates.io**: all 12 publishable crates individually confirmed at 0.4.0
  via the API, not sampled.
- **GitHub Release `v0.4.0`**: tarballs for aarch64-apple-darwin,
  aarch64-unknown-linux-gnu, x86_64-unknown-linux-gnu, each with `.sha256`,
  plus the combined sums file and the skills bundle.
- **Homebrew**: `brew install c9r-io/tap/orchestrator` on Apple Silicon
  macOS → `orchestrator 0.4.0 (f82f1ae)`.
- **install.sh**: Apple Silicon macOS (Darwin arm64, this workstation) →
  exit 0, `orchestrator 0.4.0 (f82f1ae)`; `docker run --platform linux/amd64
  ubuntu:24.04` → exit 0, `orchestrator 0.4.0 (f82f1ae)`, glibc 2.39. The
  bookworm probe that established the glibc floor is in Known limits.
- **Liveness gate after the release**: `PASS: latest tag v0.4.0 has a
  GitHub Release` — the real evidence path, `v0.3.1` exemption dormant,
  exactly the QA 200 checklist's post-release expectation.
