---
lifecycle: active
related_fr: FR-153
---

# Supply Chain And Dependency Governance

**Status**: Released
**FR**: FR-153（供应链与依赖面治理）
**QA**: docs/qa/orchestrator/202-supply-chain-dependency-governance.md
**Predecessor**: docs/design_doc/orchestrator/156-dependency-policy-gate.md
(FR-133) — this document extends its ledgers; it does not supersede it.

## The problem

The 2026-08-01 audit found the dependency surface self-describing in ways
the tree no longer backed: npm trees with no updater, one workflow using
two versions of the same action, a policy file whose header stated counts
nobody re-derived, 17 unmaintained advisories accepted in one sentence of
prose, and a tracked `.cargo/config.toml` imposing a USB-drive throttle on
CI runners.

## What the FR got wrong, and what that taught

Phase 2 step 0 rebuilt every claim at `a538d508` before planning. The
corrections are themselves the most useful findings.

### The bans claim inverts: two tools, two universes

The FR read `Cargo.lock`, found schemars at three versions and bit-vec at
two with no matching skips, and concluded `cargo deny check bans` was
either red or running on a stale skip table. It was green. The lock
records every *optional* dependency of every package whether or not any
feature enables it; `serde_with` declares optional `schemars 0.9` / `1.2`
and `yasna` declares optional `bit-vec 0.9.1`, none enabled anywhere in
the graph, so cargo-deny — which checks the feature-resolved graph, not
the lock — never sees them. A raw lock grep counts 50 multi-version
crates; `cargo tree --all-features --target all --duplicates` counts 48,
and 48 is what `deny.toml` said. Neither number is wrong; they measure
different universes, and the policy governs the one that compiles.

The half the FR got right was the drift itself, at a different number:
the header said "70 extra copies" while the skip list held 71, because
`base64@0.22.1` was accepted (2026-08-01, the #79/#82 governance) after
the sentence was written. A stated count invites exactly this, so both
counts are now gated (below) rather than trusted. The "653 external
packages" in the licenses note was re-derived as 654 lock entries carrying
a `source` and is gated on that definition — the lock-based count is the
one derivable without a resolver, and the prose now says which universe it
means.

### The npm hole was a removal, not an oversight

Dependabot npm coverage for three trees — `gui/`, `site/`, and the
project-bootstrap portal template — existed for a few hours on 2026-07-23:
added at `b16b9156`, removed at `3446b652` ("chore: finish Dependabot
cleanup") with no recorded reason, after PRs #67–75 (all breaking majors)
were closed unmerged. Nothing noticed for nine days because nothing was
positioned to notice: the config was the only statement of intended
coverage, and it is exactly the artefact the removal edited. The
`dependabot-npm-coverage` rule breaks that circularity by deriving the
required set from the repository itself.

### Action drift has a mechanism, and it is not a config defect

`ci.yml` held `setup-node@v7` and `@v6` in the same file. The
github-actions ecosystem was enabled the whole time. The mechanism:
Dependabot offered exactly these bumps — #65 (setup-node 6→7) and #15
(upload-artifact 4→7) — and both were closed, and a closed PR suppresses
re-offering that update until a newer version exists. Manual edits
(`1c0b170d`) then added `@v6` steps into a `@v7` file. The convergence
commit therefore lands *at or above the closed PRs' targets* (setup-node
v7, upload-artifact v7, download-artifact v8), which moots the
suppression instead of fighting it. `release.yml`'s upload/download pair
moves together; all post-v4 majors share the v4 artifact backend.

### The 17 unmaintained advisories have three roots, not one

The FR attributed all 17 to "Tauri 2 pinning gtk-rs 0.18". Measured by
`cargo tree -i` per crate:

- **11** are the gtk-rs 0.18 archival (atk, atk-sys, gdk, gdk-sys,
  gdkwayland-sys, gdkx11, gdkx11-sys, gtk, gtk-sys, gtk3-macros, plus
  proc-macro-error via glib-macros 0.18). Retire together when Tauri moves
  off gtk-rs 0.18 — FR-076 territory. Condition: `cargo tree -i gtk` empty.
- **5** are the unic UCD family, via tauri-utils → urlpattern 0.3.
  Tauri-rooted but gtk-independent: a gtk fix does not retire them.
  Condition: `cargo tree -i unic-common` empty.
- **1** is paste, via cel-interpreter 0.10 — a *daemon/CLI* dependency
  that ships in the binaries and that no Tauri migration will ever touch.
  Condition: `cargo tree -i paste` empty.

The split matters because a ledger whose retirement condition is wrong is
a ledger nobody can retire.

## The design

### Prose counts are compared against their derivations

`prose-counts-derived` (scripts/qa/dependency-policy.rb) anchors on two
phrases in `deny.toml` — "N crates resolve to more than one version; M
extra copies" and "N external packages" — and compares them against the
skip list itself (distinct crate names / entry count; every duplicate must
hold a skip or CI is red, so the skip list is a faithful mirror of the
graph's duplicate set) and the lock (entries carrying `source`). A phrase
that has been reworded out of existence is a finding, not a skip: the gate
says what it can no longer see (§4.4 shape 7). No resolver is needed, so
the rule runs in the governance job with no cargo.

### npm coverage is a set equality, both directions

`dependabot-npm-coverage` walks the tree for `package.json` (pruning
node_modules, target, .git, dist — the portal template under `.claude/`
deliberately stays in scope), and requires: an npm entry per tree, a tree
per npm entry, and the presence of `cargo` and `github-actions` entries.
Missing config, unparseable config, and an empty walk all fail closed.
Grouping in `dependabot.yml` keeps noise down — minor+patch collapse to
one PR per tree, majors arrive individually because the July batch proved
each needs its own verdict.

### The unmaintained ledger binds

`security.yml` now runs `cargo audit --deny unsound --deny unmaintained`.
Every unmaintained advisory is booked in `.cargo/audit.toml` with a reason
and a retirement condition, or the build is red — the eighteenth advisory
cannot arrive silently. FR-133 declined this, reasoning an 18-entry ignore
file *is* the policy; FR-153 chose it for exactly that reason plus the
ratchet. The per-entry comment requirement was already enforced
(`audit-unsound-denied`); that rule now also asserts both `--deny` flags.

### The tracked build throttle is gone

`.cargo/config.toml` kept only a comment documenting the user-level recipe
(`~/.cargo/config.toml`: `jobs = 4`, `incremental = false`). CI runners
get default parallelism back.

## Measurement

- Before: `Rust test` 259s — run 30684584564 at `7d3abb8f`, jobs capped
  at 4 on a 4-vCPU ubuntu runner.
- After: `Rust test` 252s — run 30695310417 at `9122b3c1`, default
  parallelism, same runner class.
- Delta: −7s (−2.7%), inside run-to-run noise, matching the prediction
  written before the measurement (~0, because the runner has 4 vCPUs and
  the cap matched the hardware). The reason to remove the throttle was
  never CI seconds but that a local hardware compromise was shipping as
  project policy.

## Closure evidence

- Security run 30695310417-sibling (workflow `Security`, same push,
  conclusion success): first CI execution of `cargo audit --deny unsound
  --deny unmaintained` and of cargo-deny over the edited deny.toml.
- Dependabot: the config push triggered five update runs, all success,
  and produced PRs #84–#92 across `gui/` and the portal template within
  minutes — grouped minor+patch (#84, `gui-minor-patch`) and individual
  majors (react 19, vite 8, jsdom 30), plus #83 on the github-actions
  ecosystem. `site/` produced no PR because its tree was already current,
  which is the correct null result, not absence of coverage — the
  `dependabot-npm-coverage` rule asserts the coverage itself.
- Governance budget after refresh: 1639s against 2700s (39% headroom),
  ledger refreshed from run 30695310417 with zero pendingMeasurement
  entries remaining (the four FR-150/FR-151 steps got their first
  numbers in the same refresh).

## Known limits

- **skip-is-live reads the lock, and the lock lies by superset.** A skip
  whose crate is still duplicated *in the lock* but converged *in the
  graph* — possible when one lock version is a never-enabled optional —
  survives both `--deny unmatched-skip` (the version is still in the
  graph) and `skip-is-live` (the lock still shows two versions). Closing
  it offline would require feature resolution in Ruby; the case is narrow
  (it needs a phantom to shadow a real convergence) and is accepted here
  rather than half-fixed. Found by attacking the tooling with §4.4 shape 8
  during FR-153 planning.
- **cargo-audit has no unmatched-ignore ratchet.** An ignore entry whose
  advisory stopped applying (crate left the tree) lingers silently; the
  retirement conditions in `.cargo/audit.toml` are `cargo tree -i`
  one-liners a human runs, not a gate. Booked here so the next auditor
  attacks the ledger's liveness, not only its completeness.
- **Dependabot's update runs are not queryable evidence.** The REST API
  exposes alerts and PRs, not update-run logs; closure evidence for
  coverage is therefore the PRs a config push provokes plus the
  `dependabot-npm-coverage` gate, not a dry-run transcript.
- **prose-counts-derived anchors on phrases.** A rewrite that keeps a
  count but restates it in new words trips the phrase-missing branch and
  must update the rule's anchors — deliberate coupling, since an anchor
  loose enough to survive arbitrary rewording is loose enough to match
  the wrong sentence.
