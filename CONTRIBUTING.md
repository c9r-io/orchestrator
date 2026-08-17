# Contributing to Agent Orchestrator

Thank you for your interest in the Agent Orchestrator project!

## About This Project

This is an **AI-native development** project — the codebase is primarily developed and maintained using AI-assisted workflows (Claude Code with orchestrator skills). This means our development model differs from traditional open-source projects, and we're actively exploring how external contributions best fit into this paradigm.

## How to Contribute

### Feature Requests (Preferred)

The most impactful way to contribute is by sharing your use cases and ideas:

1. Open a [Feature Request](https://github.com/c9r-io/orchestrator/issues/new?template=feature_request.md) issue
2. Describe your scenario and the problem you're trying to solve
3. We'll evaluate and track it as an internal FR document

### Bug Reports

Found a bug? Please [report it](https://github.com/c9r-io/orchestrator/issues/new?template=bug_report.md) with:

- Your OS and architecture
- `orchestrator --version` / `orchestratord --version` output
- Steps to reproduce
- Expected vs. actual behavior
- Relevant logs (if applicable)

### Pull Requests

PRs are welcome with the following guidance:

- **Small fixes** (typos, doc improvements): submit directly
- **Non-trivial changes**: please open an issue first to discuss the approach — this avoids duplicated effort since the AI-native workflow may already have the change in progress
- All PRs must pass CI: `cargo fmt`, `cargo clippy -D warnings`, `cargo test`

## Development Setup

### Prerequisites

- **Rust** 1.77+ (`rustup` recommended)
- **protoc** (Protocol Buffers compiler)
  - macOS: `brew install protobuf`
  - Linux: `sudo apt-get install -y protobuf-compiler`
  - Or let the build system use the vendored protoc automatically

### Build & Test

```bash
# Build all crates
cargo build --workspace

# Run tests. The GUI crate compiles against gui/dist, so build the frontend
# bundle once first (Linux additionally needs the webkit2gtk/gtk dev packages:
# sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev).
npm --prefix gui ci && npm --prefix gui run build
cargo test --workspace

# Lint
cargo clippy --workspace --all-targets -- -D warnings

# Without the GUI prerequisites, exclude the GUI crate instead
cargo test --workspace --exclude orchestrator-gui
cargo clippy --workspace --exclude orchestrator-gui --all-targets -- -D warnings

# Format check
cargo fmt --all -- --check

# Async lock governance (CI enforced)
./scripts/check-async-lock-governance.sh
```

### Running Locally

```bash
# Start daemon in foreground
orchestratord --foreground --workers 2

# In another terminal
orchestrator daemon status --wait-ready
orchestrator apply -f fixtures/manifests/bundles/capability-test.yaml
orchestrator task create --goal "test run"
orchestrator task list
```

## Code Style

- **Formatting**: `cargo fmt` (enforced in CI)
- **Linting**: `cargo clippy` with `-D warnings` (zero warnings policy)
- **Async safety**: `std::sync::RwLock` restricted to approved files (see `scripts/check-async-lock-governance.sh`)
- **Commits**: conventional format — `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`

## Regenerated Artifacts

Five tracked files are generated from the tree rather than written by hand, and each has a gate
that compares the file against what the tree currently produces. The four Ruby regenerators refuse
to run under `CI`; the schema snapshot has no such guard, because its regeneration is gated by the
`UPDATE_SCHEMA_SNAPSHOT` variable itself rather than by a flag a script can refuse.

| Artifact | Regenerate with | Record |
|---|---|---|
| `config/governance/coordination-collapse-ledger.json` | `ruby scripts/qa/coordination-governance.rb --emit-inventory --write` | [DD-140](docs/design_doc/orchestrator/140-governance-ledger-regeneration.md) |
| `config/governance/core-boundary-ledger.json` | `ruby scripts/qa/core-boundary.rb --emit-baseline --write` | [DD-142](docs/design_doc/orchestrator/142-core-boundary-freeze.md) |
| `config/governance/schema-snapshot.sql` | `UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p agent-orchestrator schema_snapshot` | [DD-142](docs/design_doc/orchestrator/142-core-boundary-freeze.md) |
| `config/governance/doc-lifecycle-index.json` | `ruby scripts/qa/doc-lifecycle.rb --emit-index --write` | [DD-144](docs/design_doc/orchestrator/144-doc-lifecycle-governance.md) |
| `docs/feature_request/README.md` (generated block only) | `ruby scripts/lib/fr_registry.rb write` | FR-155 |

**Commit the regenerated artifact in the same commit as the change that caused it.** This is a
constraint, not a convention, and the reason is shared: splitting it leaves an intermediate
revision that fails the gate, which is a revision nobody can bisect through. The first four
artifacts each have a second, specific reason recorded in their own section below.

The FR registry has no section of its own, and one property worth stating here: it is derived from
the `HEAD` ancestry rather than from the working tree, so a new FR file does not enter the table
until it is committed. The gate therefore goes red *because of* the commit that adds an FR, and
regeneration necessarily follows it. Amend that commit rather than adding a second one — this is
the one artifact where "same commit" cannot be satisfied by ordering alone.

Regenerating is not reviewing. `--write` produces a candidate; the diff is the thing a human is
accountable for, and for the ledgers that carry judgement fields it is the only place the
judgement is visible.

## Changing A Production Agent

Production Agents under `docs/workflow/` are pinned by fingerprint in
`config/governance/coordination-collapse-ledger.json`, so a spec change turns the coordination
governance gate red until the ledger is updated. The update is a human reading a diff:

```bash
ruby scripts/qa/coordination-governance.rb                  # names the Agent and the changed spec keys
ruby scripts/qa/coordination-governance.rb --emit-inventory # print the candidate and review it
ruby scripts/qa/coordination-governance.rb --emit-inventory --write   # apply it locally
```

`--emit-baseline` does the same for the four source-touch ratchets, which are compared exactly, and
`--emit-consumers` for the production consumer counts in `consumerInventory`. All three accept
`--write`, which refuses to run under `CI`.

`--emit-consumers` regenerates only the counts. The rest of each entry — `state`, `scope`,
`retainedCarrier`, the code-level blockers — is a reviewed judgement about what the count *means*,
and a tool that rewrote it would be deciding rather than measuring.

The specific reason this one must not be split: the mismatch report derives the previous spec from
`git show HEAD:<file>`, so a separate commit breaks the diagnosis as well as the gate.

## Changing The Core Crate Or A Migration

`core` is frozen at its current boundary by `config/governance/core-boundary-ledger.json`: its
top-level `pub mod` count, its public item count, and every one of its 200 `rusqlite`
references, per file. Adding a module or a `rusqlite` reference turns the boundary gate red,
and so does removing one — the comparison is exact in both directions, because a decrease is
the goal and blessing it is the review that matters.

```bash
ruby scripts/qa/core-boundary.rb                       # names the file and the count that moved
ruby scripts/qa/core-boundary.rb --emit-baseline       # print the candidate and read it
ruby scripts/qa/core-boundary.rb --emit-baseline --write   # apply it locally
```

Adding a migration changes the schema of 46 tables. The reviewed result lives in
`config/governance/schema-snapshot.sql`, and the migration chain is tested against it:

```bash
cargo test -p agent-orchestrator schema_snapshot                        # verify
UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p agent-orchestrator schema_snapshot   # regenerate
```

The specific reason for the snapshot: its diff is the only place a schema change is legible to a
reviewer at all.

## Adding Or Retiring A Design Doc Or QA Doc

Every file under `docs/design_doc/` and `docs/qa/` declares its lifecycle in YAML frontmatter, and
`config/governance/doc-lifecycle-index.json` is generated from those declarations. A new document
without frontmatter fails the build, and so does an index that has drifted from the documents —
the comparison is exact in both directions.

```yaml
---
lifecycle: active          # active | superseded
related_fr: FR-132         # optional; omit rather than guess at the attribution
---
```

```bash
ruby scripts/qa/doc-lifecycle.rb                     # names the document and what is wrong with it
ruby scripts/qa/doc-lifecycle.rb --emit-index        # print the candidate index and read it
ruby scripts/qa/doc-lifecycle.rb --emit-index --write   # apply it locally
```

When a change replaces the mechanism an existing document describes, set that document to
`lifecycle: superseded` and add `superseded_by:` naming the successor. Do not delete it — the
history is the audit trail, and the prose banner is what a human reads. A document that merely
received a post-release update is still active.

`lifecycle` is not the `**Status**:` header some design docs carry: that one records implementation
maturity, and a `Released` document can be superseded.

This index carries no judgement fields — it is derived wholly from the frontmatter, so the diff to
read is the frontmatter change that caused it.

## Adding A Resource Kind Or A Top-Level Command

A new `ResourceKind` or a new top-level command group has to justify itself against the concepts
that already exist. State in the PR description **why this is not a field, a parameter, or a
subcommand of something we already have.**

The cost is not the code. There are twelve built-in kinds, 127 leaf commands, roughly nineteen
pipeline variables and a runtime vocabulary on top of that, and every addition is paid by every
person who later has to hold the whole surface in their head. Two costs in particular do not show
up in a diff:

- **An audit action name is permanent.** A new kind mints `resource.<snake_kind>.apply` and
  `.delete` into `control_action_audit`, and recorded action names are never renamed — FR-164 kept
  `source.template.apply` for exactly this reason. Merging or renaming the concept later does not
  merge or rename its history.
- **A concept that overlaps an existing one is worse than a parameter.** EnvStore and SecretStore
  have identical specs and differ in three behaviours; a reader cannot tell which to use from the
  manifest, and the guide had to grow a comparison table to say so.

Reviewer checklist for such a PR:

- [ ] The PR says why this is not a field, parameter or subcommand of an existing concept.
- [ ] If it overlaps an existing kind or command, the difference is stated **behaviourally** — what
      the system does differently — not as intent ("this one is for sensitive values").
- [ ] The user-facing name matches what the CLI, the API, the audit trail and the docs will call
      it. One object, one noun.
- [ ] The guide chapter that lists the surface is updated in the same PR.

This rule is deliberately not enforced by a gate. Whether a concept is a parameter in disguise is a
judgement, and a gate could only check that *some* justification text exists — a text-presence
proxy certifying a review it cannot observe, which is worse than no gate. The same rule is in
`.claude/skills/orchestrator-guide/SKILL.md`, because agents write manifests and CLI surfaces here
too. See [DD-172](docs/design_doc/orchestrator/172-governance-expansion-boundary.md) for the
governance-side counterpart, which *is* gated, and why that one can be.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
