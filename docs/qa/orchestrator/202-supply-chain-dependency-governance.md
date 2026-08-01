---
lifecycle: active
related_fr: FR-153
---

# Orchestrator - The Supply Chain Ledgers Are Derived, Covered, And Binding

**Module**: Dependency governance / CI supply chain
**Scope**: `.github/dependabot.yml` coverage against the repository's npm
trees; action version uniformity across `.github/workflows/*.yml`; the
`deny.toml` prose counts against their derivations; the unmaintained
advisory ledger in `.cargo/audit.toml` and the `--deny unsound --deny
unmaintained` invocation in `security.yml`; the de-localized
`.cargo/config.toml`. Gates: `scripts/qa/dependency-policy.rb` (rules
`prose-counts-derived`, `dependabot-npm-coverage`, extended
`audit-unsound-denied`) and `scripts/qa/test-dependency-policy.sh`
(cases 19–21).
**Scenarios**: 5
**Priority**: High

## Background

FR-153 (2026-08-01 audit, re-verified at `a538d508`): three npm trees sat
outside Dependabot after an unrecorded wholesale removal (`3446b652`);
action versions drifted because closed Dependabot PRs suppress re-offering;
`deny.toml`'s stated duplicate-copy count was one day stale; 17 unmaintained
advisories rode along as unbooked debt; and a tracked `.cargo/config.toml`
imposed one machine's USB-drive throttle on CI. Design record:
`docs/design_doc/orchestrator/164-supply-chain-dependency-governance.md`.

**Safety**: every scenario is read-only against the working tree. The
fixture suite mutates scratch copies under `TMPDIR`; no daemon starts, no
database is touched, nothing reaches the network except scenario 6's
optional `gh` reads.

## Scenario 1: the extended dependency-policy gate passes against the working tree

Steps: `ruby scripts/qa/dependency-policy.rb; echo $?`

Expected result: exit 0 and the summary line
`Dependency policy: PASS (71 accepted duplicate(s), 0 finding(s))` — the
accepted-duplicate count matches the number `deny.toml`'s header states,
because `prose-counts-derived` fails the gate when they diverge.

## Scenario 2: every new rule can fire, its control cannot, and the tool half still holds

Steps: `bash scripts/qa/test-dependency-policy.sh > /tmp/dep-fixtures.log 2>&1; echo $?`
then inspect the log for cases 19–21. Where a `cargo-deny` binary exists
(security.yml's cargo-deny job, or locally), additionally:
`bash scripts/qa/test-dependency-policy.sh --tool-fixtures; echo $?`

Expected result: ruby half exits 0 with final line
`=== 39 passed, 0 failed ===`, and the log contains all of: `19.` (stale
crate count fires), `19b.` (reworded-away phrase fails closed rather than
skipping), `19c.` (stale external-package count fires), `19d.` (prose
beyond the anchor is free — silent), `20.` (commented-out npm entry fires —
the mutation a deletion-minded author would not test), `20b.` (uncovered
new tree fires), `20c.` (stale npm entry fires), `20d.` (missing
dependabot.yml fails closed), `20e.` (cadence change is silent), `21.`
(dropped `--deny unmaintained` fires), `21b.` (flag order is silent). The
summary line's presence is part of the assertion: a run that aborted early
never prints it. The tool half exits 0 with `=== 6 passed, 0 failed ===`;
case 15b still shows the `unmatched-skip` / `skip-is-live` pair covering
their separate halves after the deny.toml prose edits.

## Scenario 3: one version per action, asserted by parse and approximated by grep

Steps (parse — the assertion):

```
ruby -ryaml -e '
uses = Dir[".github/workflows/*.yml"].flat_map { |f|
  YAML.safe_load(File.read(f), aliases: true)["jobs"].values.flat_map { |j|
    (j["steps"] || []).map { |s| s["uses"] }.compact } }
dup = uses.grep(%r{^actions/}).map { |u| u.split("@") }.group_by(&:first)
          .transform_values { |v| v.map(&:last).uniq }.select { |_, v| v.length > 1 }
abort dup.inspect unless dup.empty?
puts "one version per action"'
```

Steps (grep — the paired proxy, the FR's original criterion):
`grep -ho "actions/[a-z-]*@v[0-9]*" .github/workflows/*.yml | sort -u`

Expected result: the parse prints `one version per action`; the grep lists
each `actions/*` name exactly once (checkout@v7, setup-node@v7,
upload-artifact@v7, download-artifact@v8 as of `a538d508`+5). The grep
alone is §4.4 shape 1 — a version string in a comment satisfies it — which
is why the parsed form is the assertion of record.

## Scenario 4: the unmaintained ledger binds in both directions

Steps: `cargo audit --deny unsound --deny unmaintained; echo $?` (network:
advisory DB fetch), then `ruby scripts/qa/dependency-policy.rb` after
temporarily deleting one `# ...` comment line above any RUSTSEC entry in a
scratch copy (`--repo-root`).

Expected result: cargo audit exits 0 — all 17 unmaintained advisories plus
RUSTSEC-2024-0429 are booked, none unbooked. The scratch mutation makes the
gate fire `audit-unsound-denied`: an entry without a reason above it is not
an acceptance. The reverse direction — an *unbooked* eighteenth advisory —
is enforced by cargo audit itself in security.yml, which is the ratchet the
flag exists for (fixture: case 21).

## Scenario 5: Dependabot coverage is real, not declared

Steps: `ruby scripts/qa/dependency-policy.rb` (rule `dependabot-npm-coverage`
derives the npm-tree set by walking for `package.json` and requires set
equality with the config, both directions). For live evidence:
`gh pr list --author 'app/dependabot' --state all --limit 10` after any
push that changes `.github/dependabot.yml`, which triggers an immediate
update run.

Expected result: the gate passes — three npm entries (`/gui`, `/site`,
`/.claude/skills/project-bootstrap/assets/template/portal`) match exactly
three discovered trees, and `cargo` + `github-actions` entries are present.
The live check shows Dependabot activity for the npm directories (the
FR-153 closure evidence records PRs produced by the re-enablement push).

## Checklist

- [ ] the accepted-duplicate count in the gate's summary line equals the
      count `deny.toml`'s header states — divergence is a
      `prose-counts-derived` finding, not a doc edit
- [ ] every new rule's must-fire fixture is paired with a must-not-fire
      control on the same probe, and the npm-entry mutation comments out
      rather than deletes
- [ ] action-version uniqueness is asserted on parsed `uses:` values;
      the grep form is recorded as a proxy only
- [ ] all 18 advisory acceptances carry a reason and a `cargo tree -i`
      retirement condition, and both `--deny` flags are asserted by the
      gate rather than assumed from the YAML
- [ ] the npm coverage set is walked from the repository, both directions,
      failing closed on a missing or unreadable config
