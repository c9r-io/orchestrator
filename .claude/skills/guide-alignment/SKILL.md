---
name: guide-alignment
description: "Compile-driven user guide alignment. Builds the project, walks the full CLI --help tree, compares against docs/guide/ (EN+ZH), auto-fixes drift, and outputs an alignment report. Use when docs may have drifted from CLI implementation."
---

# Guide Alignment

Compile-driven alignment of `docs/guide/` with the actual CLI binary.

## Phase 1: Compile & Collect CLI Truth

1. Run `cargo build --release`. If compilation fails, stop and report the error.
2. Walk the CLI command tree by running `--help` at every level:
   - `./target/release/orchestrator --help`
   - For each top-level subcommand: `./target/release/orchestrator <cmd> --help`
   - For nested subcommands (`task`, `store`, `debug`, `secret`, `db`, `manifest`, `agent`): `./target/release/orchestrator <cmd> <subcmd> --help`
   - For deeply nested commands (`debug sandbox-probe`, `secret key`, `db migrations`): recurse one more level
3. From each `--help` output, extract: subcommand names, aliases, argument names, types, defaults, required/optional status, short descriptions.
4. Also run `./target/release/orchestratord --help` and `./target/release/orchestratord control-plane --help` for daemon commands.

## Phase 2: Parse Documentation

1. Read all `docs/guide/*.md` and `docs/guide/zh/*.md` files.
2. Extract every CLI invocation from code blocks (` ```bash `) and inline code references.
3. Extract every argument/flag from documentation tables.
4. Build a map: command path -> documented flags, aliases, descriptions.

## Phase 3: Compare & Classify

For each command path from Phase 1, compare against Phase 2:

- **Missing-in-doc**: command, subcommand, alias, or flag exists in `--help` but not documented.
- **Missing-in-code**: documented command/flag no longer exists in `--help`.
- **Mismatch**: flag name, default value, or description differs between doc and `--help`.

Produce a structured diff list.

## Phase 4: Auto-Fix Documentation

1. For **Missing-in-doc** items: add the command/flag to the appropriate section in `docs/guide/07-cli-reference.md`, following existing formatting conventions (markdown tables for flags, bash code blocks for examples). Also update the alias table if applicable.
2. For **Missing-in-code** items: remove or update the stale reference.
3. For **Mismatch** items: update the documentation to match `--help`.
4. Apply the same fixes to `docs/guide/zh/*.md`, translating new English content into Chinese following the existing translation style.
5. Scan `docs/guide/01-06` for stale CLI examples caught in the comparison and fix them.
6. Update the C/S CLI command surface section at the bottom of `07-cli-reference.md` to include any new commands.

## Phase 5: Alignment Report

Output a Markdown report containing:

- Timestamp
- Compilation status (success/failure)
- Number of commands checked
- Per-command diff summary (table: command | category | detail | action taken)
- List of auto-fixed items
- List of items requiring human review (if any)

## Rules

- EN (`docs/guide/`) is the source of truth; ZH (`docs/guide/zh/`) mirrors EN structure and content.
- Preserve narrative text around CLI examples; only fix factual CLI references.
- Hidden commands (marked `hide = true` in clap) must NOT be documented.
- The alias table in both EN and ZH must be kept complete and sorted alphabetically.
- Process is idempotent: running it twice with no code changes should produce no diff.
