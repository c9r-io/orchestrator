# Orchestrator - Database Persistence Bootstrap and Repository Boundaries

**Module**: orchestrator
**Status**: Approved
**Related Plan**: Phase 1 of FR-009: introduce a persistence infrastructure layer, make schema bootstrap a single entry point, remove public `ensure_column`, and route session/workflow-store access through repository traits without changing external CLI behavior.
**Related QA**: `docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md`
**Created**: 2026-03-11
**Last Updated**: 2026-03-11

## Background And Goals

## Background

The existing SQLite layer had three structural issues:

- `core/src/db.rs` mixed connection configuration, schema migration bootstrap, and business-facing helpers.
- Public `ensure_column` encouraged runtime schema patching outside the migration module.
- Session persistence and local workflow store persistence still performed direct SQL inside feature modules instead of going through explicit repository boundaries.

This made future migration-framework work harder, because schema bootstrap and runtime persistence concerns were still tightly coupled.

## Goals

- Introduce a `persistence` module as the dedicated home for schema/bootstrap and SQLite access infrastructure.
- Make schema initialization flow through one explicit bootstrap entry point.
- Remove public access to `ensure_column` so schema evolution remains owned by migrations.
- Move session and workflow-store async wrappers onto repository traits and SQLite implementations.
- Preserve current SQLite behavior and keep CLI/runtime behavior backward compatible.

## Non-goals

- Replacing the current migration engine with `sqlx` in this phase.
- Converting all scheduler/config/task writes to repository traits.
- Adding new user-facing CLI commands such as `db status` or rollback management.

## Scope And User Experience

## Scope

- In scope:
  - New `core/src/persistence/` module.
  - `PersistenceBootstrap` and `SchemaStatus`.
  - `SessionRepository` and `WorkflowStoreRepository` traits with SQLite implementations.
  - Rewiring `db.rs`, `service/bootstrap.rs`, `session_store.rs`, and `store/local.rs`.

- Out of scope:
  - New CLI surface.
  - UI changes.
  - Full migration-framework replacement.

## UI Interactions (If Applicable)

- Not applicable.

## Interfaces And Data (If Applicable)

## API (If Applicable)

- No HTTP/gRPC API changes.

## Database Changes (If Applicable)

- No new tables or columns were introduced.
- Schema bootstrap now routes through `PersistenceBootstrap::ensure_current`.
- Compatibility column backfills remain inside `core/src/migration.rs` as a private helper, rather than a public DB utility.
- Session and workflow store runtime access now flow through repository traits:
  - `SessionRepository`
  - `WorkflowStoreRepository`

## Key Design And Tradeoffs

## Key Design

1. Add `core/src/persistence/` with three subdomains:
   - `sqlite`: low-level connection setup
   - `schema`: migration bootstrap/status
   - `repository`: persistence interfaces and SQLite implementations
2. Keep `core/src/db.rs` as a compatibility facade so existing callers do not need a flag-day migration.
3. Remove public `ensure_column` to stop new runtime schema patching from spreading.
4. Reuse existing sync session/store SQL logic where appropriate, but route async feature modules through repository boundaries.
5. Keep the SQLite WAL single-writer/read-split runtime model unchanged.

## Alternatives And Tradeoffs

- Option A: full `sqlx` migration in the same change.
  - Pros: stronger long-term migration governance.
  - Cons: much larger blast radius; harder to distinguish architectural refactor from framework swap.

- Option B: leave `db.rs` and feature modules as-is, only add tests.
  - Pros: smallest diff.
  - Cons: does not create a durable seam for future migration-framework work.

- Chosen: introduce persistence seams first, defer framework swap.
  - Pros: controlled risk, cleaner next step, no external contract churn.
  - Cons: mixed SQL access patterns still remain in untouched modules.

## Risks And Mitigations

- Risk: partial extraction leaves the repository story inconsistent.
  - Mitigation: document this as FR-009 Phase 1 and keep follow-up scope explicit.

- Risk: schema bootstrap changes could regress initialization flow.
  - Mitigation: add dedicated persistence bootstrap tests and keep `init_schema` as a compatibility wrapper.

- Risk: async wrappers could drift from existing behavior when moved behind traits.
  - Mitigation: keep existing session/store tests and run the full `agent-orchestrator` test suite.

## Observability And Operations (Required)

## Observability

- Logs:
  - Schema bootstrap logs the number of applied migrations through the existing tracing path.
- Metrics:
  - No new metrics introduced in this phase.
- Tracing:
  - No new tracing spans introduced.

## Operations / Release

- Config: no new environment variables.
- Migration / rollback:
  - Roll forward by deploying the updated binaries and letting bootstrap run.
  - Rollback is code-only for this phase; schema ownership is unchanged.
- Compatibility:
  - Existing SQLite databases remain compatible because migration execution still uses the current migration registry.

## Testing And Acceptance

## Test Plan

- Unit tests:
  - `persistence::schema` bootstrap/status tests
  - existing `session_store` async wrapper tests
  - existing `store::local` and `service::store` tests
- Integration tests:
  - full `cargo test -p agent-orchestrator`
- E2E:
  - not required for this internal refactor phase

## QA Docs

- `docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md`

## Acceptance Criteria

- `PersistenceBootstrap::ensure_current` becomes the effective schema bootstrap entry point.
- Public `crate::db::ensure_column` is removed.
- Session async wrappers use `SessionRepository`.
- Local workflow store async wrappers use `WorkflowStoreRepository`.
- All existing tests pass without changing external CLI behavior.
