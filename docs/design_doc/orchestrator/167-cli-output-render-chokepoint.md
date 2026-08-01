---
lifecycle: active
related_fr: FR-154
---

# DD-167: CLI Output Render Chokepoint And Clap-Derived Documentation Surfaces

**Status**: Released (FR-154, 2026-08-02)

## Background

FR-154's verified inventory (rebuilt at `98f28a8a` by three independent
audits): six commands returned different data from `-o json` vs `-o yaml`
because each format had its own hand-written projection — `task list` yaml
dropped the three item counters; `task items` yaml dropped five fields;
`event list` yaml dropped `task_item_id` and renamed+re-typed
`payload`→`payload_json`; `agent session` yaml was `println!`-interpolated
pseudo-YAML dropping 12 of 17 fields, unescaped; `agent list` yaml printed
`{:?}` capabilities and omitted keys conditionally; `attention get`/`follow`
`-o table` printed compact JSON. Six more commands advertised `-o yaml` in
clap and rejected it at runtime. Sixteen sites printed an empty string when
serialization failed (`unwrap_or_default`). Four output mechanisms coexisted
(`-o`, `--json`, `--chunks-json`, `--format`), 21 mutation commands hardcoded
their format with no flag at all, and the three documentation surfaces —
EN/ZH `07-cli-reference.md` and the 1802-line hand-maintained built-in guide
(90 entries, zero clap linkage) — disagreed with the clap tree and with each
other: 29 leaves documented nowhere, 63 absent from the markdown references.

## Goals

1. Make json/yaml divergence structurally impossible, not merely tested-for.
2. One output convention across the CLI, with exceptions named and reasoned.
3. Clap as the single source of truth for all three documentation surfaces,
   enforced in CI; adding a command without documenting it fails the build.

## Non-goals

- Server-rendered outputs (`get`/`describe`/`check`/`manifest export` pass a
  format string to the daemon and print `resp.content`; their rendering is
  the daemon's concern).
- Replacing `serde_yaml` (archived upstream) — but the chokepoint reduces a
  future replacement to one file.
- Generating the markdown references' prose from the surface (skeleton
  coverage is gated; prose stays human).

## Design

### The chokepoint (`crates/cli/src/output/render.rs`)

`Encoding { JsonPretty, JsonCompact, Yaml }` deliberately has no `Table`
variant. `encode(&Value, Encoding) -> Result<String>` is the only place in
the crate that serializes output; it decides pretty/compact and trailing
newlines once, and propagates failures (anyhow → stderr + nonzero exit).
`OutputFormat::encoding() -> Option<Encoding>` (None = caller renders a
table) and `StreamFormat::encoding()` are the only format→encoding maps.

The sanctioned printer shape: build exactly one `serde_json::Value` per
payload *before* the format match, then `Some(enc) => render::emit`,
`None => table`. Tables read the same value (`kv_table`) or the source
structs, so a table may omit columns but never show data the machine
encodings lack.

Enforcement is paired per §4.4 (a proxy is never the only condition):

- **Behavioral**: `format_parity::*` — 22 payload projections over
  fully-populated fixtures (every Option Some, every Vec non-empty, `: ` and
  CJK hazard strings), rendered through the real encoders, parsed back, deep
  value equality; `json_yaml_round_trip_identity` over an adversarial corpus
  (YAML-1.1 trap strings, `u64::MAX`, multi-line, nesting);
  `comparator_detects_divergence` proves the comparator itself can fail.
- **Structural**: `chokepoint_no_stray_serializers` scans the crate source —
  `serde_yaml::to_string` only in `render.rs` + `tool.rs` (manifest
  re-serialization for gRPC apply, not stdout); `serde_json::to_string*` only
  in `render.rs` + `guide.rs` (`--format json`); per-encoding `OutputFormat`
  match arms only in `render.rs` + `common.rs` (format-name stringification
  for server-side rendering); no serialization result swallowed by
  `unwrap_or_default` anywhere. Allowlists are named files with reasons,
  never subtrees (§4.4 shape 8).

With the shared projection, FR-154's original negative criterion ("delete a
field from a yaml projection") became structurally inexpressible; the honest
negatives are the comparator test and the chokepoint scan, and the FR was
amended to say so before implementation.

### The output convention

`-o {table,json,yaml}` everywhere: collections default `table`, single
objects and mutation acks default `yaml`, streams default `json`.
`Commands::Get` (collection, table) vs `Describe` (detail, yaml) already
followed the rule and stay distinct — kubectl precedent, not an
inconsistency. Streaming commands (`attention follow`, `source automation
watch`, `source connection watch`) take `StreamFormat { Json /*NDJSON*/,
Yaml }`; the `table` choice that silently printed JSON is no longer
advertised (parse-time rejection). `version` and `task trace` keep `--json`
one release cycle as a hidden `conflicts_with` alias.

**Named exceptions, kept deliberately**: `agent session read --chunks-json`
is a content/framing mode (NDJSON chunk records vs raw bytes), not an
encoding of the same data; `guide --format {markdown,json}` is document
rendering. Neither advertises table/yaml semantics it cannot honor.

### One source, three projections

`config/governance/cli-surface.json` is a committed `CommandFactory` walk
(`crates/cli/src/surface.rs`): 153 nodes — path, hidden (ancestor-
propagated), leaf/bare_invocable, about, aliases, args with
possible_values/defaults/hidden. clap's synthesized `help` subcommands and
global args are excluded. Freshness: `cli_surface_json_is_fresh` regenerates
in memory and byte-compares (`ORCHESTRATOR_WRITE_CLI_SURFACE=1` rewrites);
`surface_covers_known_tree_shape` pins the known shape so a broken walk
cannot silently shrink the surface.

Consumers:

- **Built-in guide**: `command_entries()` (126 entries) must equal the
  visible invocable set bidirectionally (`guide_matches_clap_leaves`);
  pseudo-topics (`error-codes`) moved to a structurally separate
  `topic_entries()` — a type distinction, not an exemption list — with a
  no-collision test.
- **Markdown references**: `scripts/qa/test-cli-doc-parity.sh` (ci-required,
  governance job) requires EN and ZH `07-cli-reference.md` to cover every
  visible invocable path, rejects invocations of unknown or hidden commands
  (longest-known-prefix token walk with alias support), and re-checks the
  guide-vs-surface equality cargo-free (so the ci-required claim does not
  depend on a cargo build in the governance job). Coverage extraction strips
  HTML comments first, which is what makes the comment-out fixtures
  meaningful. Six negative fixtures, each naming a derived victim.

## Tradeoffs

- **Ratchet**: adding a CLI leaf now fails CI until the surface is
  regenerated, a guide entry exists, and both markdown references cover it.
  Intended — that is the drift mechanism being removed. The repair recipe is
  the rewritten `guide-alignment` skill.
- **Doc-coverage semantics are prefix-based**, not per-flag: the gate proves
  every command is present and no dead command is documented; it does not
  prove flag tables are complete. Flag-level parity stays with the skill's
  manual sweep (known limit).
- `kv_table` renders nested objects as compact JSON — a legible default that
  avoids per-payload table code for 21 mutation acks.
- The chokepoint scan is textual (line-oriented, statement-scoped for the
  `unwrap_or_default` rule); it is paired with the behavioral suite precisely
  because a text scan alone would be a §4.4 violation.

## Compatibility

The full breaking surface is enumerated in CHANGELOG `[Unreleased]`
Compatibility And Migrations: the `--json` deprecation schedule,
`task trace` pretty-printing, `task items` `label`→`qa_file_path`,
additive json/yaml field gains, `agent session get/resolve` default flip,
streaming `-o table` parse-time rejection, `attention` pretty JSON. In-repo
consumers were audited: all pass explicit `-o json`;
`probe-low-output.sh` migrated in the same commit as the flag change.

## Observability

A serialization failure is now a visible stderr diagnostic with context
(`failed to serialize output as JSON/YAML`) and a nonzero exit — previously
an empty stdout line and exit 0. Gate diagnostics name the exact missing
path, unknown invocation, or guide diff entry.

## Testing / Acceptance

`docs/qa/orchestrator/205-cli-output-doc-parity.md` — five scenarios: the
parity suite, the comparator+chokepoint negatives, surface freshness, guide
equality, and the gate with its fixtures. Certification sweep recorded in the
FR-154 closure commit.

## Known limits

- Flag-table completeness in the markdown references is not gated (see
  Tradeoffs); the surface JSON carries the data a future flag-level check
  would need.
- `--json` aliases must be removed next release cycle (CHANGELOG carries the
  schedule); the surface records them as hidden until then.
- The daemon-rendered `get`/`describe` formats are outside the chokepoint; if
  the daemon ever grows a json/yaml divergence it needs its own FR.
- serde_yaml is archived upstream; replacement is a one-file change when a
  successor is chosen.
