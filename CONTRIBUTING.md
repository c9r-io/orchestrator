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

# Run tests (excludes GUI crate which needs Tauri deps)
cargo test --workspace --exclude orchestrator-gui

# Lint
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
orchestrator init
orchestrator apply -f fixtures/capability-test.yaml
orchestrator task create --goal "test run"
orchestrator task list
```

## Code Style

- **Formatting**: `cargo fmt` (enforced in CI)
- **Linting**: `cargo clippy` with `-D warnings` (zero warnings policy)
- **Async safety**: `std::sync::RwLock` restricted to approved files (see `scripts/check-async-lock-governance.sh`)
- **Commits**: conventional format — `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`

## Changing A Production Agent

Production Agents under `docs/workflow/` are pinned by fingerprint in
`config/governance/coordination-collapse-ledger.json`, so a spec change turns the coordination
governance gate red until the ledger is updated. The update is a human reading a diff:

```bash
ruby scripts/qa/coordination-governance.rb                  # names the Agent and the changed spec keys
ruby scripts/qa/coordination-governance.rb --emit-inventory # print the candidate and review it
ruby scripts/qa/coordination-governance.rb --emit-inventory --write   # apply it locally
```

`--emit-baseline` does the same for the four source-touch ratchets, which are compared exactly.
`--write` refuses to run under `CI`.

**Commit the ledger update in the same commit as the change that caused it.** This is a constraint,
not a convention: the mismatch report derives the previous spec from `git show HEAD:<file>`, so
splitting the commit both breaks the diagnosis and leaves the intermediate revision failing the gate.
See [DD-140](docs/design_doc/orchestrator/140-governance-ledger-regeneration.md).

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

**Commit the regenerated ledger or snapshot in the same commit as the change that caused it.**
The snapshot diff is the only place a schema change is legible to a reviewer, and an
intermediate revision that fails the gate is one nobody can bisect through. `--write` refuses
to run under `CI`. See [DD-142](docs/design_doc/orchestrator/142-core-boundary-freeze.md).

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

**Commit the regenerated index in the same commit as the document change.** `--write` refuses to
run under `CI`. See [DD-144](docs/design_doc/orchestrator/144-doc-lifecycle-governance.md).

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
