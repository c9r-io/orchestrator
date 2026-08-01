---
lifecycle: active
related_fr: FR-154
self_referential_safe: true
---

# Orchestrator - CLI Output Format Equivalence And Three-Surface Documentation Parity

**Module**: CLI output layer (`crates/cli/src/output/`) / CLI documentation surfaces
**Scope**: the FR-154 render chokepoint (`output/render.rs`) making `-o json`
and `-o yaml` structurally equivalent for every command; the unified `-o`
convention with its two named exceptions; `config/governance/cli-surface.json`
as the committed clap-tree projection; the three documentation surfaces (EN/ZH
`07-cli-reference.md`, the built-in guide) held to it by
`scripts/qa/test-cli-doc-parity.sh` and two cargo test families.
**Scenarios**: 5
**Priority**: High

## Background

FR-154 found six commands whose json and yaml outputs carried different data
(two independently hand-written projections each), six commands advertising
`-o yaml` and rejecting it at runtime, 16 sites printing an empty string on
serialization failure, and 29 leaf commands absent from all three
documentation surfaces (63 absent from the markdown references). The closure
is structural: every printer builds one `serde_json::Value` and serializes it
through the single chokepoint `crates/cli/src/output/render.rs`, and the
documentation surfaces derive from `config/governance/cli-surface.json`, a
committed `CommandFactory` dump. Design record:
`docs/design_doc/orchestrator/167-cli-output-render-chokepoint.md`.

**Safety**: every scenario is a read-only cargo test run or script run over
the working tree; no daemon is started, no database touched.

## Scenario 1: json/yaml deep equivalence over fully-populated fixtures

Steps:

```bash
cargo test -p orchestrator-cli format_parity 2>&1 | tail -3
```

Expected result: all `format_parity::*` tests pass — 22 payload projections
(task list/items/detail, event list, timeline response/delta, attention
item/list/delta, agent session/status, handoff snapshot, resume boundary,
audit record, secret key composite, seven source projections) rendered through
the real encoders, parsed back, and compared for deep value equality; every
fixture has all `Option`s populated and non-empty `Vec`s, with `: `-bearing
and CJK strings as YAML-escaping hazards.

## Scenario 2: the comparator and the chokepoint can both fail

Steps:

```bash
cargo test -p orchestrator-cli comparator_detects_divergence 2>&1 | tail -3
cargo test -p orchestrator-cli chokepoint_no_stray_serializers 2>&1 | tail -3
```

Expected result: both pass. `comparator_detects_divergence` feeds the
equivalence helper a hand-made divergent json/yaml pair and asserts it reports
divergence — with the shared projection, a per-command yaml field deletion is
no longer expressible, so the honest negative is the comparator itself plus
the chokepoint scan, which fails on any `serde_yaml::to_string` or
`serde_json::to_string*` outside its named-file allowlists, any per-encoding
`OutputFormat` match arm outside `render.rs`/`common.rs`, and any
serialization result swallowed by `unwrap_or_default` (each allowlist entry
names a file and a reason; no subtrees).

## Scenario 3: the committed CLI surface is fresh and shaped correctly

Steps:

```bash
cargo test -p orchestrator-cli surface 2>&1 | tail -3
```

Expected result: `cli_surface_json_is_fresh` (regenerates the
`CommandFactory` walk in memory and byte-compares against
`config/governance/cli-surface.json`) and `surface_covers_known_tree_shape`
(exactly 2 hidden nodes with ancestor propagation, `debug` bare-invocable,
`task list -o` metadata, `version --json` recorded hidden) both pass. Editing
`crates/cli/src/cli.rs` without rerunning
`ORCHESTRATOR_WRITE_CLI_SURFACE=1 cargo test -p orchestrator-cli cli_surface_json_is_fresh`
fails this scenario.

## Scenario 4: the built-in guide equals the clap tree

Steps:

```bash
cargo test -p orchestrator-cli guide_matches_clap_leaves 2>&1 | tail -3
cargo test -p orchestrator-cli guide_topics_do_not_collide_with_commands 2>&1 | tail -3
```

Expected result: both pass — `command_entries()`'s 126 command strings equal
the visible invocable clap paths bidirectionally (failure prints both
one-sided diffs), and topic pseudo-entries (`error-codes`) never shadow a real
path. Adding a CLI command without a guide entry fails this scenario.

## Scenario 5: the documentation parity gate and its negative fixtures

Steps:

```bash
bash scripts/qa/test-cli-doc-parity.sh; echo "exit=$?"
bash scripts/qa/test-cli-doc-parity.sh --fixture-test; echo "exit=$?"
```

Expected result: verification mode prints 6 passed, 0 failed (surface
readable, EN covers all 126 paths, ZH covers all 126 paths, every documented
invocation resolves to a real visible command, guide equals surface); fixture
mode prints 13 passed, 0 failed — the six fixtures (truncated surface,
commented-out EN coverage, ZH-only loss, documented-but-removed command,
documented hidden command, commented-out guide entry) are each rejected with a
diagnostic naming the derived victim, and the TARGETED meta-assertion proves
every registered check has a fixture. Both exits are 0. The gate is
ci-required in `.github/workflows/ci.yml`'s governance job (steps
`cli-doc-parity` / `cli-doc-parity-fixtures`).

## Checklist

- [ ] a new printer builds one `serde_json::Value` before the format match
      and serializes only through `render::emit` — never a second projection
      per encoding
- [ ] chokepoint allowlists stay named files with reasons, never subtrees
      (§4.4 shape 8); removing the `--json` aliases next cycle also removes
      `version --json` from the surface's hidden-args record
- [ ] after any `crates/cli/src/cli.rs` change, regenerate the surface with
      `ORCHESTRATOR_WRITE_CLI_SURFACE=1 cargo test -p orchestrator-cli cli_surface_json_is_fresh`
      and update guide.rs + both references (repair recipe: `guide-alignment`
      skill)
- [ ] parity fixtures stay fully populated — an empty `Vec` or `None` field
      passes vacuously and hides an escaping divergence
- [ ] flag-table completeness in the markdown references is NOT gated
      (DD-167 known limits); do not cite this gate as evidence for it
