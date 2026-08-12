---
lifecycle: active
related_fr: FR-165
self_referential_safe: true
---

# 219. Two-sided ratchets: coverage drift and advisory acceptances

Verifies FR-165 requirements 3 and 4: the coverage baseline fails in both
directions, and every advisory acceptance is still accepting something.

Every scenario is read-only or operates on copies under `$TMPDIR`. Nothing starts a
daemon or writes the runtime database. Scenario 1 runs `cargo llvm-cov` over the
workspace, which writes only to `target/`.

Design record: `docs/design_doc/orchestrator/181-two-sided-ratchets.md`.

## Scenario 1 — the coverage measurement, pinned

### Steps

```bash
git rev-parse HEAD
git ls-files '*.rs' | sort | xargs shasum | shasum          # record before
cargo llvm-cov --workspace --all-targets --all-features \
  --json --output-path target/coverage-governance/rust.json
git rev-parse HEAD
git ls-files '*.rs' | sort | xargs shasum | shasum          # must be unchanged
```

### Expected

Exit 0, and both the revision and the tracked-Rust digest identical before and
after — a coverage figure whose source moved mid-run is not a measurement of
anything. Recorded run: `15f54289`, digest `d3731c3e7e7e06fd` both sides,
`cargo-llvm-cov 0.8.5`, macos-aarch64.

Drift against the baseline as approved before this work, all 22 metric pairs:

| Entry / metric | approved | actual | drift |
|---|---|---|---|
| CLI / functions | 30.82 | 54.94 | +24.12 |
| CLI / lines | 35.49 | 53.02 | +17.53 |
| cli/commands / lines | 33.78 | 44.78 | +11.00 |
| cli/commands / functions | 27.80 | 38.68 | +10.88 |
| daemon/session / lines | 15.52 | 20.69 | +5.17 |
| daemon/session / functions | 32.81 | 37.68 | +4.87 |
| daemon adapter / functions | 35.88 | 37.13 | +1.25 |
| daemon adapter / lines | 41.84 | 42.93 | +1.09 |
| core/domain / lines | 84.29 | 85.27 | +0.98 |
| core/domain / functions | 81.29 | 82.24 | +0.95 |
| 12 further pairs | — | — | exactly 0.00 |

The interval (1.25, 4.87) is empty, which is what makes 3.0 a stable band rather
than a fitted one. **The FR's 52.86% for CLI does not reproduce**: 53.02% here, on
a denominator that grew 7373 → 7456. The earlier figure was measured before commits
that added CLI code; it was single-source prose in the baseline's own note.

## Scenario 2 — the baseline fails in both directions

### Steps

```bash
bash scripts/coverage-governance.sh --fixture-test
```

### Expected

`coverage governance fixtures: PASS`, exit 0. The FR-165 cases:

| Case | Must |
|---|---|
| 17.53 above approved, slack 3 | fail on **both** metrics, naming CLI, the approved value, the drift in points, and the word "re-approve" |
| 0.98 above approved, slack 3 | pass — ordinary drift from unrelated work costs nothing |
| exactly 3.00 above, slack 3 | pass — a declared 3 allows three points, not 2.99 |
| 3.01 above, slack 3 | fail |
| 50 above, **no** slack declared | pass — absent is not zero, or this change breaks every pre-existing baseline on arrival |
| 0.01 above, `slack: 0` | fail — zero is not absent |
| 20 **below** approved, slack 3 | fail with `40% < 60%` — declaring a band must not switch off regression detection |
| unsupported branches on both sides, slack 3 | pass — a `null` percent is not an infinite improvement |
| the committed baseline | must declare `policy.improvementSlack` as a number, carry a rationale over 200 characters, and be **≤ 5.0** |

The last case is the one that keeps the rest honest, and its third clause was added
by the closure self-check. Every case above declares its own slack, so they prove
the mechanism and say nothing about the committed number — a baseline shipping
`improvementSlack: 100` satisfied every other assertion here while restoring the
unbounded interval the requirement exists to close. 5.0 is the ceiling because the
drifts being caught began at +4.87. Verified by mutation: setting the committed
value to 100 fails with `improvementSlack is 100; at 5 or above the band admits the
gaps it was introduced to catch`.

The `branches` key must be present and explicitly unsupported in each fixture.
Omitting it makes every case report an extra "missing percentage", which is how the
first draft got three failures where it asserted two — and it would have read as
the band misfiring rather than as the fixture being wrong.

## Scenario 3 — the re-approved baseline is clean, and only three entries moved

### Steps

```bash
node -e '
import("./scripts/coverage/coverage-governance.mjs").then(async (m) => {
  const fs = await import("node:fs");
  const raw = JSON.parse(fs.readFileSync("target/coverage-governance/rust.json", "utf8"));
  const base = JSON.parse(fs.readFileSync("coverage/boundary-baseline.json", "utf8"));
  const rust = m.summarizeRust(raw, process.cwd(), "unsupported");
  const summary = { rust, frontend: base.frontend,
                    playwright: { total: base.playwright.minimumScenarios, failed: 0 } };
  const f = m.compareSummary(summary, base);
  console.log(f.length === 0 ? "PASS" : f.join("\n"));
});'
git diff --stat coverage/boundary-baseline.json
```

### Expected

`PASS`. Frontend and Playwright are held at their approved values because neither
was measured on this host — Node 26 breaks the GUI unit suite and there are no
Playwright browsers — so this scenario exercises the Rust side only, which is what
requirement 3 changed. The diff must move exactly three entries: CLI,
`cli/commands`, `daemon/session`. Every other entry stays, its drift being inside
the band.

## Scenario 4 — every advisory acceptance is still accepting something

### Steps

```bash
ruby scripts/qa/dependency-policy.rb
grep -c 'retire-when' .cargo/audit.toml
grep -c '^\s*"RUSTSEC' .cargo/audit.toml
```

### Expected

Exit 0, and both counts **18** — one declaration per acceptance, checked rather
than assumed to correspond. The gate reports the split before its verdict:

```
Advisory acceptances: 18 total — 17 absent, 1 patched>=
Dependency policy: PASS (71 accepted duplicate(s), 0 finding(s))
```

That line exists because the closure self-check found `.cargo/audit.toml`'s header
claiming it. `absent` is strictly weaker than `patched>=` — it retires only when the
crate leaves the tree, so an advisory that does have a fixed release and is booked
`absent` goes on being accepted after the fix lands, and the check cannot tell
because it does not read the advisory database. A falling `patched>=` count is the
signal, and it has to be printed or the weakening produces no line anywhere. The
count also makes the header's own "17 unmaintained acceptances" checkable against
one line of output instead of by hand.

Re-derive the subjects independently against the lock; all 18 crates must be
present, and glib must be below 0.20.0 or RUSTSEC-2024-0429 is stale:

```bash
python3 - <<'PY'
import re
lock = open('Cargo.lock').read()
v = {}
for m in re.finditer(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', lock):
    v.setdefault(m.group(1), []).append(m.group(2))
for line in open('.cargo/audit.toml'):
    m = re.match(r'\s*#\s*retire-when:\s*crate=(\S+)\s+(\S+)', line)
    if m: print(f"{m.group(1):22} {m.group(2):18} lock={v.get(m.group(1), 'ABSENT')}")
PY
```

Recorded: 18 rows, every crate present at a single version, glib at 0.18.5 against
a `patched>=0.20.0` bound.

## Scenario 5 — the acceptance ratchet fails on a broken ledger

### Steps

```bash
bash scripts/qa/test-dependency-policy.sh
```

### Expected

`45 passed, 0 failed`, exit 0, and the summary line present. The FR-165 cases:

| Case | Mutation | Must |
|---|---|---|
| 22 | one `retire-when` line removed | fail — an acceptance with no end condition |
| 22b | the crate renamed out of the lock | fail naming "not in Cargo.lock at all" |
| 22c | glib's bound lowered to 0.18.0 | fail naming "the advisory is fixed" — the **reverse instance** a presence check cannot see |
| 22d | paste given `patched>=0.9.0` against a locked 1.0.15 | fail — a lexical version compare would say unpatched and stay silent |
| 22e | gtk given `patched>=0.18.3` against a locked 0.18.2 | **pass** — or the check could satisfy every case above by reporting "fixed" unconditionally |
| 22f | `Cargo.lock` emptied | fail, asserted on the detail `audit-ignore-is-live examined nothing` rather than the `empty-scan` tag, because `skip-is-live` emits the same tag for the same lock |

22b renames rather than deletes: an upstream rename is how this actually happens,
and it leaves the file looking complete.

## Regression Checklist

- [x] `ruby scripts/qa/dependency-policy.rb` — exit 0
- [x] `bash scripts/qa/test-dependency-policy.sh` — 45 passed, 0 failed
- [x] `bash scripts/coverage-governance.sh --fixture-test` — PASS
- [x] `bash scripts/qa/test-coverage-governance-mainpath.sh` — 10 passed, 0 failed
- [x] `bash scripts/qa/test-bash32-compat.sh`, `test-bash32-lexer.sh` — exit 0
- [x] `ruby scripts/qa/pipefail-short-circuit.rb`, `jq-status-observed.rb`,
      `fixture-target-drift.rb` — exit 0
- [x] `bash scripts/qa/test-qa-gate-surface.sh` — exit 0 (no new gate added:
      `dependency-policy.rb` and `coverage-governance.sh` were already ci-required,
      so requirements 3 and 4 cost the budget nothing)
- [x] `ruby scripts/qa/ci-cost.rb` — exit 0, 2024s of 2700s, unchanged
- [x] `ruby scripts/qa/rollback-contract-single-source.rb`,
      `doc-lifecycle.rb`, `qa-doc-lint.sh`, `test-governance-ledger-tooling.sh` — exit 0

### Carried forward from QA 218

`ruby scripts/qa/ci-liveness.rb` is still red and still needs a real CI run to
refresh run IDs against the current sha; it predates FR-165 requirement 1's `ci.yml`
edit. `bash scripts/qa/test-markdown-link-integrity.sh` still aborts under bash 3.2
in the primary working directory and passes at the same commit in a clean worktree
(`docs/ticket/20260812-markdown-link-gate-aborts-under-bash32.md`). Neither is
affected by requirements 3 or 4, which touched no workflow and no markdown link.
