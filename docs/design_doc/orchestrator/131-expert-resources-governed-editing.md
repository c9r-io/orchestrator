---
lifecycle: active
related_fr: FR-119
---

# Orchestrator GUI - Expert Resources Governed Editing

**Module**: Orchestrator Core / Daemon / GUI
**Status**: Released
**Related Plan**: FR-119 typed resource catalog and reviewed optimistic editing
**Related QA**: `docs/qa/orchestrator/169-expert-resources-governed-editing.md`
**Created**: 2026-07-25
**Last Updated**: 2026-07-25

## Background

The Expert Resources panel previously requested collection YAML and displayed it as an opaque block. Although Tauri commands for describe and apply existed, users had no resource row from which to reach those paths. The frontend would also have needed to infer identity from YAML, making the daemon's resource model, authorization, and audit boundaries easy to bypass accidentally.

FR-119 replaces the unreachable flow with a typed daemon-owned catalog and a reviewed edit path. The daemon remains authoritative for resource identity, validation, concurrency, RBAC, persistence, and audit evidence.

## Goals

- Make Workspace, Workflow, Agent, StepTemplate, and ExecutionProfile resources individually reachable by mouse and keyboard.
- Give read-only users inspect/copy access and Operator/Admin users an explicit reviewed edit flow.
- Reject stale drafts without overwriting newer state and preserve the user's draft after validation or conflict failures.
- Correlate mutations through Action Audit and request IDs without persisting manifest content.

## Non-goals

- Reimplementing manifest validation or authorization in React.
- Directly editing daemon files or SQLite from the GUI.
- Supporting arbitrary CRDs or SecretStore editing in this panel.
- Redesigning the complete Expert Console information architecture.

## Scope

- In scope: typed catalog summaries, bounded pagination, canonical describe manifests, stable revisions, optimistic Apply, reviewed confirmation, accessible focus management, and isolated vertical QA.
- Out of scope: resource deletion, bulk editing, diff rendering, virtualized lists, and external provider setup.

## UI Interactions

- Surface: Task detail → "Expert" → "Resources".
- Five type controls load daemon-owned resource summaries.
- A resource row opens a canonical manifest detail. "Back to resources" returns focus to the selected row.
- "Copy" is available to all roles. "Edit" exists only for Operator/Admin.
- "Review changes" opens a modal showing resource identity, project impact, and a required audit reason. A successful Apply re-describes authoritative state and displays its request ID.

## gRPC And Tauri Interfaces

`ResourceCatalogList` is an additive read-only gRPC method:

```text
ResourceCatalogListRequest {
  resource_type: string
  project: optional string
  cursor: optional string
  limit: uint32
}

ResourceSummary {
  kind: string
  name: string
  project_id: string
  revision: string
  source: optional string
}
```

`DescribeResponse.resource` carries the summary whose revision corresponds to the canonical editable content. Tauri exposes matching `resource_list`, `resource_describe`, and `resource_apply` DTOs. Apply sends `expected_revision`, `require_absent`, operator reason, and idempotency key through the existing `ApplyRequest` and Action Audit envelope.

## Data And Revision Model

- No database migration is introduced.
- Catalog order is deterministic by resource name. Pages are bounded to 1–500 entries and use the last name as an opaque continuation cursor.
- For the five supported builtin kinds, revision is SHA-256 over the canonical YAML value after map-key normalization.
- Describe reconstructs an apply-compatible builtin manifest instead of returning ResourceStore generation and timestamp fields.
- Existing semantic revisions for SourceTaskTemplate and SourceTaskBinding remain unchanged for compatibility.
- Action Audit stores the canonical request hash and resource identity (`Kind/name`), not raw manifest content.

## Key Design

1. The frontend never parses YAML to discover identity. It selects a `ResourceSummary` and passes its canonical resource path and revision back to the daemon.
2. Apply requires exactly one builtin manifest when an optimistic fence is present. The daemon resolves the manifest identity, locks configuration mutation, compares the authoritative revision, then validates and persists.
3. Both successful and rejected reviewed mutations use per-kind apply audit evidence — `resource.workspace.apply` for a Workspace, and so on per `ResourceKind` (FR-164; before that every kind but three shared the generic `resource.apply`). Validation and stale-revision failures therefore remain traceable. A manifest that fails to parse has no resolvable identity and still records the generic `resource.apply` against `resource_manifest`.
4. A successful mutation triggers a fresh describe. A conflict also refreshes authoritative content and revision, while the editor keeps the user's draft for manual reconciliation.
5. Native buttons, visible focus rings, dialog focus trapping/return, reduced-transparency fallback, and automated Axe checks preserve accessibility.

## Alternatives And Tradeoffs

- Parse collection YAML in React: avoids a new RPC but duplicates schema knowledge and cannot provide trustworthy identity or pagination.
- Apply without revision fencing: simpler, but silently overwrites concurrent operator changes.
- Lock an edit session server-side: stronger coordination, but adds durable lease lifecycle and recovery complexity disproportionate to occasional expert edits.
- Selected approach: additive typed summaries plus optimistic CAS, retaining canonical YAML as the editable payload.

## Risks And Mitigations

- Risk: catalog and describe revisions drift.
  - Mitigation: both use the same canonical builtin projection and a regression assertion compares them.
- Risk: UI role hiding is treated as authorization.
  - Mitigation: every RPC is authorized again by daemon control-plane policy.
- Risk: manifest or secret material leaks through errors or audit.
  - Mitigation: audit hashes content, public evidence uses identity/request ID, and the isolated QA gate searches logs, UI output, audit JSON, and SQLite.
- Risk: concurrent edits lose work.
  - Mitigation: stale revisions fail closed; authoritative content reloads separately from the retained draft.

## Observability

- Action: `resource.<kind>.apply` — `resource.workspace.apply` for the Workspace edits this document covers (FR-164). Unparseable manifests fall back to `resource.apply`.
- Target: the per-kind `target_type` (`workspace` here) and canonical `Kind/name`.
- Correlation: `x-request-id` is returned through Tauri and shown after Apply; failures include the same request-ID diagnostic.
- Durable evidence: Action Audit status, expected revision, reason code, operator reason, request hash, and result/error code.
- Metrics: no new metric family; existing control-plane request and Action Audit health provide the operational signal.
- Logs: daemon logs may identify the request and resource but must not include submitted YAML.

## Operations / Release

- No new runtime configuration or schema migration is required.
- The RPC and response field additions are backward compatible; old clients continue to use Get/Describe/Apply.
- Rollback removes the new GUI catalog surface and RPC handler. Existing resources and Action Audit rows require no data rollback.
- Run `scripts/qa/test-expert-resources-governed-editing.sh` against debug binaries for isolated vertical certification.

## Test Plan

- Rust unit tests cover all five projections, deterministic pagination, stable revisions, and apply-compatible describe output.
- gRPC integration tests cover the typed catalog and describe summary round trip.
- Vitest covers role boundaries, navigation, authoritative reload, validation recovery, conflict recovery, and stale-list clearing.
- Playwright covers keyboard entry, five catalogs, reviewed operator editing, focus return, and Axe.
- The isolated daemon script proves real Tauri/gRPC Apply, CAS rejection, audit status, and privacy.

## QA Docs

- `docs/qa/orchestrator/169-expert-resources-governed-editing.md`

## Acceptance Criteria

- All five supported resource types have individually actionable rows.
- Read-only users can inspect/copy but cannot Apply; Operator/Admin require reviewed confirmation.
- Identity comes only from daemon DTOs.
- Successful Apply reloads authoritative content; validation and conflict failures retain the draft.
- Mutation evidence is request-ID correlated and contains no manifest sentinel.
- Rust, Vitest, Playwright, and isolated daemon gates pass.
