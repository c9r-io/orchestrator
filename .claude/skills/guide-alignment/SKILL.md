---
name: guide-alignment
description: "Repair CLI documentation drift flagged by the FR-154 parity infrastructure: regenerate config/governance/cli-surface.json, read the failing gate's diagnostics, and fix docs/guide/ (EN+ZH) and the built-in guide until the gate and cargo tests pass. Use when cli-doc-parity or guide_matches_clap_leaves fails, or after adding/changing CLI commands."
---

# Guide Alignment

Since FR-154, CLI-vs-documentation drift is *detected* mechanically — this
skill is the repair procedure, not the detector. The single source of truth is
the clap tree, projected into `config/governance/cli-surface.json`; three
surfaces are held to it in CI:

| Surface | Enforced by | Where it runs |
|---|---|---|
| `config/governance/cli-surface.json` freshness | `cli_surface_json_is_fresh` (cargo test) | test job |
| built-in guide (`crates/cli/src/commands/guide.rs`) | `guide_matches_clap_leaves` (cargo test) + gate check 5 | test + governance jobs |
| `docs/guide/07-cli-reference.md` EN + ZH | `scripts/qa/test-cli-doc-parity.sh` (ci-required) | governance job |

## Procedure

1. **Regenerate the surface** after any `crates/cli/src/cli.rs` change:

   ```bash
   ORCHESTRATOR_WRITE_CLI_SURFACE=1 cargo test -p orchestrator-cli cli_surface_json_is_fresh
   ```

   Read the diff of `config/governance/cli-surface.json` — it names exactly
   what changed (new paths, flags, defaults, hidden markers).

2. **Run the detector and read its diagnostics** — never re-derive the drift
   by hand:

   ```bash
   bash scripts/qa/test-cli-doc-parity.sh
   cargo test -p orchestrator-cli guide_matches_clap_leaves
   ```

   The gate prints the missing paths per document, invalid/hidden invocations,
   and the two-sided guide-vs-surface diff.

3. **Fix the built-in guide first**: add/remove `GuideEntry` items in
   `crates/cli/src/commands/guide.rs` (`command_entries()`; pseudo-topics live
   separately in `topic_entries()`). Summaries come from the `about` strings in
   `cli-surface.json`; examples must use real flags with real defaults.

4. **Fix the markdown references**: `docs/guide/07-cli-reference.md`, then its
   structural mirror `docs/guide/zh/07-cli-reference.md`. Coverage counts an
   inline backtick span or a fenced-code-block line that starts with the
   command path; HTML-commented mentions do not count. Follow the existing
   section layout; flags in tables, examples in ```bash blocks.

5. **Sweep chapters 01–06 and the ZH mirrors** for examples using changed
   flags or defaults; the gate's check 4 catches removed/hidden commands but
   prose describing old *behavior* needs eyes.

6. **Re-run step 2 until green**, then `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all -- --check`.

## Rules

- EN is the source of truth; ZH mirrors EN structure and content exactly.
- Hidden commands and hidden flags (deprecated aliases like `--json`) must NOT
  be documented.
- Output conventions to document: unified `-o {table,json,yaml}` (collections
  default table, single objects/mutations default yaml); streaming
  follow/watch commands take `-o {json,yaml}` (NDJSON json default); the two
  named exceptions are `agent session read --chunks-json` and
  `guide --format`.
- Preserve narrative prose; only fix factual CLI references.
- Never hand-maintain a command list this infrastructure derives — that is the
  drift mechanism FR-154 removed (design record: DD-167).
