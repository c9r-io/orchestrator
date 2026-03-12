# Error Semantics Governance

**Related FR**: `FR-014` (closed; feature request doc removed after implementation)  
**Related QA**: `docs/qa/orchestrator/66-error-semantics-governance.md`

## Background and Goals

The repository already denies `panic`/`unwrap`/`expect` in non-test code through clippy, but that only solves the crash surface. It does not guarantee stable error semantics across the critical execution chain.

This change introduces a boundary-focused error governance model so that key paths can distinguish:

- user input errors
- config validation errors
- not-found lookups
- invalid state transitions
- security denials
- external dependency failures
- internal invariant failures

Goals:

- keep internal refactors incremental instead of forcing repo-wide enum migration
- preserve contextual diagnostics (`operation`, optional `subject`, source chain)
- map gRPC errors through one shared policy instead of per-handler string flattening
- add regression coverage for category-to-status behavior

Non-goals:

- removing all `anyhow` usage from the repository
- replacing test-only `expect()` assertions
- adding a new protobuf error-details schema in this change

## Scope

In scope:

- `core` service entrypoints for task, resource, store, and system operations
- daemon gRPC boundary mapping
- secret key RPC flows
- CLI rendering regressions for gRPC status formatting

Out of scope:

- deep repository/kernel migration to typed errors in every module
- non-critical example or script code
- wire-format changes to the gRPC API

## Interface and Data Changes

New core interface:

- `core/src/error.rs`
  - `ErrorCategory`
  - `OrchestratorError`
  - classifier helpers per boundary (`classify_task_error`, `classify_resource_error`, `classify_store_error`, `classify_system_error`, `classify_secret_error`)

Service contract change:

- `core/src/service/{task,resource,store,system}.rs` now return `core::error::Result<T>` at the boundary instead of raw `anyhow::Result<T>`
- internal modules may still use `anyhow`; the service boundary converts or classifies before the error reaches daemon transport code

Daemon contract change:

- `crates/daemon/src/server/mod.rs` owns a single `map_core_error` function
- RPC handlers no longer hand-roll `Status::internal(format!("{e}"))` for normal domain failures

gRPC mapping policy:

- `UserInput` -> `INVALID_ARGUMENT`
- `ConfigValidation` -> `FAILED_PRECONDITION`
- `NotFound` -> `NOT_FOUND`
- `InvalidState` -> `FAILED_PRECONDITION`
- `SecurityDenied` -> `PERMISSION_DENIED`
- `ExternalDependency` -> `UNAVAILABLE`
- `InternalInvariant` -> `INTERNAL`

## Key Design and Tradeoffs

The main tradeoff is deliberate: typed semantics are enforced only at the service and transport boundary, not inside every low-level module. This keeps the change decoupled from scheduler, repository, and config internals while still making user-visible behavior stable.

Classification is message-assisted for now. That is acceptable because:

- boundary ownership is centralized in `core/src/error.rs`
- gRPC mapping logic is no longer duplicated
- future work can replace heuristic classification with richer typed variants without changing the daemon contract

The secret-key RPC path is also routed through the same mapper so key lifecycle state failures are no longer indistinguishable from generic internal errors.

## Risks and Mitigations

Risk: message-based classification can drift when lower-layer wording changes.  
Mitigation: keep classifiers centralized and add focused regression tests for representative boundary errors.

Risk: over-classifying internal failures as user/config errors.  
Mitigation: unrecognized failures default to `InternalInvariant`, not to a softer class.

Risk: CLI output loses operator hints.  
Mitigation: keep `FailedPrecondition` hint handling in CLI common formatting.

## Observability and Operations

- `OrchestratorError` always carries an `operation` string and may carry a `subject`
- gRPC status messages keep operation context for operator diagnosis
- low-level details stay in source chains and existing tracing/event paths
- no new runtime flags are required; the change is compatible with existing daemon and CLI flows

## Testing and Acceptance

Acceptance is satisfied when:

- representative classifier tests pass in `core`
- daemon tests confirm category-to-gRPC-code mapping
- CLI tests confirm formatting still preserves precondition hints and not-found messages
- workspace test suite passes with the new boundary contract

Related executable coverage lives in:

- `docs/qa/orchestrator/66-error-semantics-governance.md`
