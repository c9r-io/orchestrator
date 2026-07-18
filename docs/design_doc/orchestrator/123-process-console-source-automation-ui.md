# Orchestrator - Process Console Source Automation UI

**Module**: Orchestrator
**Status**: Approved
**Related Plan**: FR-112 Process Console source automation management
**Related QA**: `docs/qa/orchestrator/160-process-console-source-automation-ui.md`
**Created**: 2026-07-18
**Last Updated**: 2026-07-18

## Background

FR-107 through FR-111 established authenticated Slack reaction evidence, deterministic task templates, exact badge bindings, canonical task creation, durable retry, Attention, and safe route operations. Operators could use those capabilities through YAML and CLI, but daily configuration and diagnosis still required remembering resource fields and manually joining source, route, binding, template, Attention, and task identifiers.

FR-112 adds the product frontend inside the existing Process Console `Sources` domain. The GUI remains a typed client: the daemon validates manifests, renders goals, matches bindings, enforces authorization, owns optimistic concurrency, records audit evidence, and performs every mutation.

## Goals And Non-goals

Goals:

- manage task templates and exact Slack badge bindings without editing YAML;
- preview an unsaved template and simulate an unsaved binding with the production daemon renderer/matcher;
- inspect recent routes, attempt history, stable failures, health, and linked provenance;
- make save, suspend/resume, replay, and ignore reviewed, audited, and stale-safe;
- preserve read-only inspection, secret/message privacy, accessibility, and narrow-window operation.

Slack OAuth installation, SecretStore value editing, Slack message-body display, frontend matching/rendering, destructive force cleanup, and a new top-level navigation item remain out of scope. Reversible binding suspension is preferred; dependency-aware force deletion stays in the Admin CLI.

## Information Architecture And Routes

`Sources` retains three primary views:

```text
Sources
├── Events
├── Process bindings
└── Automations
    ├── Templates
    ├── Badge bindings
    └── Recent routes
```

Stable hash routes are:

- `#/sources/events[/<event-id>]`;
- `#/sources/bindings[/<task-id>]`;
- `#/sources/automations/templates[/<name>]`;
- `#/sources/automations/bindings[/<name>]`;
- `#/sources/automations/routes[/<route-id>]`.

The legacy `#/sources/<task-id>` process deep link remains parseable. New process links use the canonical `#/processes/<task-id>` route. Route detail links to its bounded source event, current binding/template editor, matching Attention item, and Process Workspace. Attention items containing `source_route_id` link back to route detail.

## UI Interactions

Template editor fields cover resource name, Skill name/invocation/arguments, workflow, workspace, goal template, allowlisted variables, and start policy. The preview panel requires an explicit configured Slack installation and sample permalink; “Render preview” sends both with the unsaved manifest to the daemon and labels the result as side-effect-free. “Copy” creates a new local draft without changing the original revision.

Badge Binding fields cover installation/Trigger, exact reaction, template, channel policy, actor roles, and suspended state. The installation catalog exposes configured actor identifiers and safe role names but no token/signing value. “Simulate badge” sends authenticated-evidence-shaped sample inputs and an optional unsaved manifest to the production matcher; the result explicitly states that no mutation or network call occurred.

Recent Routes provides state, binding, and task filters, aggregate health counters, dense route rows, safe failure details, pinned revisions, and an attempt timeline. “Replay” defaults to the pinned generation; adopting current configuration is an explicit reviewed checkbox. “Ignore” deliberately terminates the route. Both require an audit reason and compare the displayed route version.

Read-only users receive safe editors and simulation/preview but no save, lifecycle, replay, ignore, protected Slack link, or hidden focusable mutation control.

## Daemon And Tauri Interfaces

The additive `SourceAutomationCatalogGet` RPC returns project-scoped templates, bindings, Slack installation policy metadata, workflows, and workspaces. It excludes SecretStore references/values, rendered goals, provider payloads, message coordinates, and protected permalinks. Tauri maps every proto response into local serializable types instead of exposing generated proto or raw JSON.

Draft operations add optional manifest overlays:

- `SourceTaskTemplatePreviewRequest.draft_content`;
- `SourceTaskBindingSimulateRequest.draft_content`;
- `SourceAutomationSimulateRequest.draft_binding_content`.

The daemon validates exactly one expected built-in resource, clones one immutable active-config snapshot, overlays the draft, and calls the existing renderer/matcher. It never publishes the clone, persists a resource/source/route/task, or performs provider network access.

Mutations add optimistic contracts:

- `ApplyRequest.expected_revision` compares a normalized template/binding content hash;
- `ApplyRequest.require_absent` makes create fail if the resource already exists;
- `SourceTaskBindingMutationRequest.expected_revision` fences suspend/resume;
- replay/ignore retain the FR-111 positive route version and idempotency requirements.

Configuration mutations share a daemon mutex around revision check and persistence. A missing, newly created, deleted, or changed resource returns gRPC `Aborted`; the GUI maps it to an explicit reload-and-review message. Canonical audit payloads contain resource identifiers, revision/version, action, reason, and request hashes, not manifests, URLs, goals, or secrets.

Manifest validation now projects responsible resource paths through `DiagnosticEntry.scope`, including `metadata.name`, `spec.goalTemplate`, `spec.skill.*`, references, roles, reaction, channel, event kind, and target kind. The GUI renders blocking messages next to the matching field while keeping the daemon error as authority.

## Data And Compatibility

No database migration is required. Templates/bindings continue through versioned resource persistence; routes/attempts/changes and Attention continue through migration 34. The new proto fields are additive and optional. Existing CLI apply/preview/simulate/mutation callers send `None`/`false`, preserving prior behavior.

The frontend does not store automation drafts, Slack URLs, rendered goals, tokens, or payloads in local/session storage. Drafts exist only in React state for the view lifetime. `SourceEvent` Tauri projection parses bounded reaction provenance inside Rust and omits `normalized_json` entirely.

## Key Design And Tradeoffs

1. A catalog RPC was chosen over parsing exported YAML in the frontend so role-safe projections and revisions remain typed and daemon-owned.
2. Unsaved-manifest overlays were chosen over a frontend validator/renderer so preview and live behavior cannot drift.
3. Content hashes were retained as revisions instead of GUI counters because they survive restart/export and describe normalized meaning.
4. Reviewed dialogs require a reason and trap/restore focus; mutation buttons are absent for insufficient roles rather than cosmetically disabled.
5. The UI displays the current resource when following a historical pinned revision. The immutable hash remains visible; historical snapshot rendering stays an operations/API concern.

Risks and mitigations:

- Stale drafts overwrite current policy: daemon-side CAS and authoritative reload reset the editor.
- Preview leaks private data: only the operator-provided sample URL and daemon-redacted result live in memory; no storage or audit body.
- Route streams create load/noise: one cancellable project stream emits bounded deltas and reloads authoritative pages.
- Dense controls become inaccessible: semantic headings/labels/live regions, visible status text, focus trapping, axe checks, and a one-column layout below 760 px.

## Observability And Operations

All mutations use the canonical action-audit envelope and retain daemon request IDs in failures. Route health reuses FR-111 authoritative status counts and attempt/change history. No new metric labels or high-cardinality data are introduced.

The release/rollback control is configuration based: suspend a binding/Trigger or set `reactionRouting: disabled`; no frontend rollback changes persisted route/task/audit history. Older GUI clients ignore additive RPC fields. An older daemon does not expose the new catalog/draft/CAS interface, so the FR-112 GUI requires a matching daemon version.

## Test Plan And Acceptance

- Rust unit/workspace tests cover scoped diagnostics, safe Tauri projection, additive proto callers, and existing matcher/renderer/reliability invariants.
- Vitest covers template preview/apply, field diagnostics, binding simulation/suspension, route replay/version, read-only visibility, routing, and reviewed-dialog focus.
- Playwright covers creation, preview, simulation, suspension, replay, provenance deep links, read-only DOM/storage privacy, axe, reduced motion, and 640 px layout.
- A real Tauri/gRPC test runs against an isolated daemon with two templates and two badge bindings in one Slack installation, then exercises catalog, preview, shared simulation, CAS, suspend, and resume.
- The existing signed fake-Slack vertical flow proves durable route/task convergence and protected permalink boundaries.

Run:

```bash
./scripts/qa/test-source-automation-ui.sh
```

Detailed acceptance steps are in `docs/qa/orchestrator/160-process-console-source-automation-ui.md`.
