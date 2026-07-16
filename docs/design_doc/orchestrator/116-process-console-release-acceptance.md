# Orchestrator - Process Console v1 Release Acceptance

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-106 Console v1 release acceptance, migration-proof QA, release notes, and rollback runbook  
**Related QA**: `docs/qa/orchestrator/153-process-console-release-acceptance.md`  
**Created**: 2026-07-15  
**Last Updated**: 2026-07-15

## Background

FR-095 through FR-105 delivered the Process Console as independently governed vertical slices. Their isolated tests retained useful ownership and diagnostics, but there was no clean-tree gate proving that all slices, the real Tauri-to-gRPC recovery path, release performance fixtures, and forward migrations work together. The action-audit script also equated migration-31 presence with the latest schema being exactly 31, which became false when migration 32 was added.

This design establishes the final Console v1 release boundary without adding a runtime API or schema. It treats migration presence as an identity/capability assertion, coordinates existing slice-owned scripts, proves a populated historical upgrade, and centralizes operator rollout and rollback behavior.

## Goals

- Require current-HEAD daemon, CLI, and GUI builds from a clean worktree.
- Aggregate all nine Console slice gates without duplicating their domain assertions.
- Accept schema 31, schema 32, and future additive schemas when migration 31 and its table capability exist, while rejecting a catalog missing migration 31.
- Prove a populated schema-26 database preserves every Console aggregate and audit association through migration 32 and can rebuild metrics.
- Publish one release note source and one operator upgrade/rollback runbook.

## Non-goals

- New Process Console behavior, UI redesign, protobuf methods, or database migrations.
- GitHub Release, tag, package, or FR-076 desktop distribution work.
- Down migrations or destructive removal of additive Console tables.
- Replacing independently owned slice scripts with one monolithic test implementation.

## Scope

- `scripts/qa/test-process-console-release.sh` coordinates builds, repository quality gates, and slice scripts.
- `scripts/qa/test-control-plane-action-audit.sh` verifies migration identity and schema capability.
- `core/src/migration.rs` contains the populated historical upgrade regression.
- `CHANGELOG.md` and `docs/guide/agent-process-console-v1-operations.md` define release and operator contracts.
- DD-116 and QA-153 record the design and reproducible evidence.

## Interfaces And Data Changes

There are no runtime interface or schema changes. The governed release interfaces are:

- `./scripts/qa/test-process-console-release.sh`: non-interactive, fail-fast, clean-tree release command.
- `KEEP_RELEASE_QA=1`: opt-in retention of temporary diagnostic logs; default cleanup remains privacy-safe.
- Migration-31 capability: the applied catalog row named `m0031_control_action_audit`, schema version at least 31, the `control_action_audit` table, and its required columns.
- Operator contract: the Console v1 operations guide and Unreleased changelog section.

## Key Design

1. The release coordinator first validates tools and a clean worktree. A clean tree makes checked-out source equal to current HEAD and prevents an uncommitted local patch from becoming release evidence.
2. It explicitly builds daemon, CLI, Rust GUI, and web GUI before daemon-backed tests. Cargo may reuse valid dependency artifacts, but it always evaluates current HEAD and cannot skip the build merely because an executable file exists.
3. Repository-wide tests, strict Clippy, and documentation lint run before nine slice gates: timeline, Attention, handoff/resume, Session, source/Slack, action audit, Console UI, vertical flow, and process metrics.
4. Each gate prints its owning FR, command, status, and elapsed time. The coordinator stops on the first failure and preserves business assertions in the owning script.
5. The migration-31 test matrix creates SQLite backups at schema 31, current schema 32, a simulated future additive schema, and current schema with catalog row 31 removed. Only the last case must fail.
6. The populated fixture applies migrations incrementally from schema 26, inserts representative domain state as each schema becomes available, then upgrades to latest. It verifies stable IDs, Session state normalization, project backfill, request-ID joins, and rebuildable metric rollups.

## Data Preservation Contract

The historical fixture preserves:

- the task, event, and pre-existing exited Session;
- Attention item, action, and change records;
- immutable handoff snapshot and Session control action;
- source event and binding identities;
- canonical action-audit identity joined to domain and event projections;
- migration-32 metric observation and all supported rebuilt rollups.

The migration kernel remains forward-only. A normal binary rollback keeps migrations 27-32 and their tables. A database restore is a separate disaster action used only for failed migration or corruption.

## Alternatives And Tradeoffs

- Repeating domain assertions in the coordinator would make one command self-contained but create drift and obscure slice ownership. Calling the scripts retains focused failure diagnostics.
- Checking only `MAX(schema_migrations.version) >= 31` would tolerate newer schemas but could falsely pass when migration 31 is missing. Identity plus capability is stricter and forward-compatible.
- Copying an old fixture directly to schema 32 would be shorter but would not exercise intermediate data shapes and backfills. Incremental population proves the actual supported path.
- A full gate is slower than individual feedback loops. Slice scripts remain available for iteration; the 399-second aggregate gate is a release boundary.

## Risks And Mitigations

- **Local port/daemon collision**: slice scripts use isolated data roots, non-standard ports where applicable, and cleanup traps.
- **Stale binaries**: current-HEAD builds run before daemon-backed gates; Session QA receives `SKIP_BUILD=1` only after that build succeeds.
- **Sensitive diagnostic retention**: temporary logs are deleted by default; retention requires explicit opt-in and logs exclude content-bearing product fields.
- **Future migration drift**: the simulated future schema proves that feature presence is independent from latest-version equality.
- **Unsafe rollback advice**: the runbook separates fail-closed binary rollback from last-resort database restore and forbids normal down migration.

## Observability And Operations

- Every gate emits name, owner, command, duration, and terminal status.
- Failure is fail-fast and prints the completed summary; optional logs live outside the repository.
- The operator runbook provides backup integrity checks, migration verification, domain stop-loss, rollout order, compatibility checks, and disaster boundaries.
- The release gate itself mutates only isolated fixtures and build outputs; it does not touch a production database or external provider.

## Test Plan

- Unit/migration: populated schema-26 upgrade through latest with entity, relationship, backfill, and rollup assertions.
- Script regression: migration 31/32/future accepted and missing-31 rejected.
- Aggregate: clean current-HEAD builds, workspace tests, strict Clippy, documentation lint, and nine owning scripts.
- Integrated UI: 21 Vitest tests, 15 Playwright tests, source-wide frontend coverage reporting, production build, accessibility checks, and real Tauri handler to gRPC flow.
- Performance: 50,000-event timeline and Process Console metric fixtures execute in release mode under DD-114 budgets.

## QA Docs

- `docs/qa/orchestrator/153-process-console-release-acceptance.md`
- `scripts/qa/test-process-console-release.sh`

## Acceptance Criteria

- The migration identity matrix accepts schema 31, 32, and future additive versions and rejects missing migration 31.
- The populated schema-26 fixture upgrades to schema 32 without losing Console entities or request-ID joins and rebuilds all supported metric rollups.
- The clean-tree release coordinator builds current HEAD and passes all 14 gates.
- The real vertical flow proves failure → Attention → evidence → handoff → stale rejection → reviewed resume → Attention resolution.
- Changelog and operations guide cover compatibility, migrations, rollout, stop-loss, binary rollback, and disaster restore.
- Normal rollback never deletes migrations 27-32 or their additive tables.
