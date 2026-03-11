# Orchestrator - Database Migration Kernel and Repository Governance

**Module**: orchestrator
**Status**: In Progress
**Related FR**: `docs/feature_request/FR-009-database-migration-framework.md`
**Related QA**: `docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md`
**Created**: 2026-03-11
**Last Updated**: 2026-03-11

## Background And Goals

## Background

FR-009 Phase 1 introduced a `persistence` infrastructure layer, moved schema bootstrap into `PersistenceBootstrap`, removed the public `ensure_column` helper, and routed session/workflow-store runtime access through repository traits.

That phase intentionally did not finish the broader database-governance problem:

- migration step implementations still live in a single `core/src/migration.rs` file
- runtime task/scheduler/config persistence is still spread across direct SQL call sites
- `core/src/db.rs` remains a compatibility facade with mixed business helpers
- operator-facing schema status and migration listing must be added without breaking the CLI/core split

The next step must preserve the current SQLite + `rusqlite` execution model while creating durable seams for future migration-framework evolution.

## Goals

- Split migration concerns into explicit persistence subdomains: catalog, runner, status, and step implementations.
- Make schema evolution migration-owned only; runtime code must not perform dynamic schema patching.
- Expand repository governance to the highest-value runtime paths without requiring a flag-day rewrite.
- Add read-only DB operations visibility for schema version and pending migrations.
- Keep external CLI/gRPC behavior and existing SQLite database files backward compatible.

## Current Implementation Alignment

- Implemented in this phase:
  - `core/src/persistence/migration.rs` now owns `Migration`, registered catalog, `SchemaStatus`, runner, and applied-status helpers
  - `core/src/migration.rs` now acts as a compatibility facade plus legacy migration step host
  - gRPC and CLI now expose `db status` and `db migrations list`
- Still pending in this phase:
  - move migration step bodies out of `core/src/migration.rs` into dedicated `steps/*`
  - extract scheduler/config/task-write repository seams
  - add historical SQLite fixture coverage

## Non-goals

- Replacing SQLite with another database engine.
- Introducing `sqlx`, `SeaORM`, or another ORM in this phase.
- Adding general-purpose down migrations as a required capability.
- Converting every historical helper in `core/src/db.rs` in a single change.

## Scope And User Experience

## Scope

- In scope:
  - new migration submodule(s) under `core/src/persistence/`
  - migration catalog / runner / status split
  - compatibility forwarding from `core/src/migration.rs`
  - repository expansion policy for task, scheduler, and config persistence
  - read-only CLI visibility for DB status and migration listing
  - upgrade-validation strategy for historical SQLite samples

- Out of scope:
  - new remote APIs
  - schema-breaking data model redesign
  - interactive rollback workflows

## UI Interactions (If Applicable)

- CLI only:
  - `orchestrator db status`
  - `orchestrator db migrations list`
- No portal or browser UI changes.

## Interfaces And Data (If Applicable)

## API (If Applicable)

- No gRPC or proto contract changes required for the core task/resource/store APIs.
- New CLI commands are read-only operational commands exposed through the existing daemon/client architecture.

## Database Changes (If Applicable)

- No immediate schema changes are required to split the migration kernel.
- `schema_migrations` remains the source of truth for current schema version unless a later phase explicitly replaces it.
- `SchemaStatus` remains the read-only status view and may be extended with additional metadata such as applied count or pending names.
- New migration implementations must not be added to `core/src/migration.rs`; the remaining legacy step bodies are transitional and must later move into dedicated step files under `core/src/persistence/`.

## Key Design And Tradeoffs

## Key Design

1. Create a migration kernel under `core/src/persistence/`:
   - `catalog`: ordered list of descriptors
   - `runner`: transactional pending-migration execution
   - `status`: current/target/pending inspection
   - `steps/*`: one implementation file per migration or small migration group
2. Keep `core/src/migration.rs` as a thin compatibility entry point so existing imports do not require a flag-day rewrite.
3. Treat `core/src/db.rs` as a shrinking compatibility facade:
   - no new schema helpers
   - no new business SQL helpers
   - existing helpers migrate out incrementally as repositories land
4. Expand repositories by business aggregate rather than by raw table:
   - `TaskRepository`
   - `SchedulerRepository`
   - `ConfigRepository`
5. Default operational rollback strategy is backup restore plus forward-fix, not arbitrary down migration.

## Alternatives And Tradeoffs

- Option A: adopt `sqlx` or an ORM immediately.
  - Pros: stronger ecosystem tooling for migrations and query generation.
  - Cons: large blast radius; mixes boundary cleanup with framework replacement.

- Option B: leave migration code and runtime SQL spread as-is, only add more tests.
  - Pros: smallest diff.
  - Cons: does not solve ownership ambiguity or improve future change safety.

- Chosen: split the migration kernel and continue repository expansion first.
  - Pros: controlled risk, clearer ownership, better observability, easier later framework swap if still needed.
  - Cons: temporary coexistence of old and new persistence entry points during the transition.

## Risks And Mitigations

- Risk: migration split introduces subtle ordering or version-registration regressions.
  - Mitigation: enforce ascending, contiguous, unique migration metadata through tests.

- Risk: repository expansion stalls halfway and leaves `db.rs` permanently mixed.
  - Mitigation: explicitly forbid adding new business helpers to `db.rs`; treat it as compatibility-only.

- Risk: operators expect rollback support once DB commands exist.
  - Mitigation: document read-only visibility separately from rollback promises; default to backup restore and forward-fix.

- Risk: historical database samples drift from real field usage.
  - Mitigation: maintain a small curated set of upgrade fixtures: empty, old, partial-upgrade, current.

## Observability And Operations (Required)

## Observability

- Logs:
  - startup logs must include current schema version, target version, and applied migration count
  - migration failure logs must include migration version and name
- Metrics:
  - no new mandatory metrics in this phase
- Tracing:
  - existing tracing path is sufficient; additional spans are optional

## Operations / Release

- New commands:
- `orchestrator db status`
- `orchestrator db migrations list`
- Rollout:
  - deploy updated binaries and let bootstrap run pending migrations
- Rollback:
  - restore SQLite backup or ship a forward-fix; generic down migrations are out of scope
- Compatibility:
  - existing SQLite databases must continue upgrading in place

## Testing And Acceptance

## Test Plan

- Unit tests:
  - migration catalog ordering / uniqueness / contiguity
  - pending runner idempotency
  - failed migration does not advance schema version
  - `SchemaStatus` for empty, outdated, and current databases
- Integration tests:
  - upgrade from baseline to latest
  - upgrade from intermediate versions to latest
  - recover from partially-upgraded databases
  - daemon/bootstrap startup remains green
- CLI tests:
  - `db status` output for empty/current/outdated DBs
  - `db migrations list` shows current and pending migrations clearly

## QA Docs

- `docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md`

## Acceptance Criteria

- A dedicated persistence migration kernel owns catalog, runner, and status responsibilities.
- `orchestrator db status` and `orchestrator db migrations list` are available through the existing daemon/client stack.
- Migration implementations are no longer added directly to `core/src/migration.rs`; remaining legacy step bodies are transitional only.
- `core/src/db.rs` receives no new business SQL helpers.
- Repository expansion plan is documented and bounded to task, scheduler, and config persistence.
- Historical SQLite upgrade validation is documented and executable.
