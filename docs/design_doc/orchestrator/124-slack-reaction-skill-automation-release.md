# Orchestrator - Slack Reaction Skill Automation Release

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-113 aggregate release acceptance for FR-107 through FR-112  
**Related QA**: `docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md`  
**Created**: 2026-07-18  
**Last Updated**: 2026-07-18

## Background

FR-107 through FR-112 delivered authenticated Slack reaction ingestion, versioned source task templates, exact badge bindings, canonical task creation, reliable route operations, and Process Console management as independently governed slices. Each slice had focused acceptance evidence, but release readiness still required one clean-tree gate proving that fresh binaries, a real signed webhook, the outbound Slack permalink boundary, two distinct Skill/workflow routes, recovery, populated migration, compatible binary rollback, Tauri, and browser UI all operate as one product.

FR-113 establishes that release boundary. It does not add a runtime interface or product state transition. Product authority remains in the daemon, existing slice scripts remain independently executable, and the aggregate coordinator preserves their ownership and diagnostics.

## Goals

- Build the current daemon, CLI, Rust GUI, and Web GUI from a clean worktree before daemon-backed acceptance.
- Prove two Slack badges in one installation create two distinct zero-cost deterministic Skill/workflow tasks.
- Exercise durable acknowledgement, concurrent identity convergence, rate-limit recovery across restart, and one reviewed Attention replay.
- Preserve Console and source automation data through migrations 26 through 34 and a compatible previous-binary rollback.
- Aggregate real Tauri, Vitest, Playwright, accessibility, RBAC, responsive, and privacy acceptance.
- Publish a task-oriented setup, diagnosis, upgrade, stop-loss, and rollback guide.

## Non-goals

- Connecting to a production Slack workspace or invoking a paid coding agent.
- Creating a Slack app, completing OAuth installation, or distributing a desktop installer.
- Outbound Slack progress messages, Slack message-body ingestion, or `reaction_removed` cancellation.
- A destructive down migration, database version fabrication, or deletion of tasks already created from Slack.

## Scope

In scope:

- `scripts/qa/test-slack-skill-automation-release.sh` as the clean-tree aggregate coordinator;
- `scripts/qa/test-slack-skill-automation-vertical.sh` as the isolated signed-webhook, recovery, Tauri, and rollback fixture;
- `fixtures/manifests/bundles/slack-skill-automation-release-fixture.yaml` as a deterministic two-badge, two-workflow bundle;
- migration and real Tauri provenance regressions;
- release documentation, changelog, and cross-document acceptance routing.

Out of scope:

- changes to Slack normalization, matching, rendering, task creation, route operations, or GUI product semantics owned by FR-107 through FR-112;
- production credentials, external network calls, or live provider costs.

## UI Interactions

The release gate validates the existing visible path rather than adding navigation:

1. Open "Sources" from primary navigation.
2. Open "Automations" and inspect "Templates", "Badge bindings", and "Recent routes".
3. Follow route links to the source event, Attention item, and Process Workspace.
4. Verify ReadOnly users receive safe metadata without mutation controls or protected Slack links.

## Interfaces And Data

No new RPC, Tauri command, HTTP route, CLI command, table, or column is introduced.

The governed release interfaces are:

- `./scripts/qa/test-slack-skill-automation-release.sh` — fail-fast clean-tree release gate;
- `KEEP_RELEASE_QA=1` — retain aggregate logs after success; failure logs are retained automatically;
- `FR113_PREVIOUS_REF` — override the pinned compatible previous daemon commit for rollback qualification;
- `KEEP_QA=1` — retain the isolated vertical fixture for diagnosis;
- the existing `/source/slack/{project}/{trigger}` signed Slack endpoint and existing source automation CLI/Tauri contracts.

Migrations 33 and 34 remain additive and forward-only. Migration 33 adds the durable route identity and frozen template/binding evidence. Migration 34 adds generation, optimistic version, retry/lease state, attempt/change history, and Attention correlation. The release regression seeds a populated migration-33 route, applies migration 34, and verifies status normalization plus generation/change backfill.

## Key Design

1. The aggregate script coordinates owning slice scripts instead of copying their assertions. `SKIP_DEPENDENCY_GATES=1` avoids nested reruns only when the aggregate has already invoked the dependency directly; standalone scripts retain their original behavior.
2. The vertical fixture applies both badges to the same message and uses two deterministic echo workflows. `agent-implement` selects `$ticket-fix` and `slack-release-implement`; `agent-docs` selects `$qa-doc-gen` and `slack-release-docs`. Distinct automation binding identities produce two tasks, while same message/reaction/binding retries remain idempotent. Both start and complete without an AI provider.
3. The fake Slack API implements only the production boundary used by the daemon: bearer-token validation, deterministic `chat.getPermalink`, HTTP 429 with `Retry-After`, and `invalid_auth`. It records only a coordinate hash, outcome, and attempt number.
4. Concurrent deliveries use distinct Slack event IDs for the same message/reaction/binding identity. The durable automation key must converge all deliveries on one route and deterministic task.
5. The restart checkpoint is a persisted `retrying` route after a provider 429. The daemon stops before retry is due, restarts against the same database, and completes the existing route without a new delivery.
6. The actionable recovery path rotates the test SecretStore credential, runs side-effect-free preview and simulation, then replays with a positive route version, operator reason, idempotency key, and explicit current-config adoption.
7. The real Tauri regression joins route, source event, task binding, task detail, and timeline through production handlers and gRPC. Protected permalink retrieval is separate from safe list/get projections.
8. Compatible rollback disables `reactionRouting` before starting the pinned FR-111-era daemon on the schema-34 database. The previous daemon may append an ignored source event, but it must preserve existing tasks, routes, migration rows, and audit evidence and create no new automation work.

## Alternatives And Tradeoffs

- A single monolithic release script would be easier to invoke but would hide slice ownership and duplicate mature assertions. A small coordinator plus a dedicated vertical fixture keeps failure attribution explicit.
- Mock-only GUI coverage would be faster but would not prove Tauri serialization or gRPC role projections. The release includes both fast Playwright mocks and one real Tauri boundary.
- Simulating rollback with only a migration prefix would avoid an extra build but would not prove an actual compatible old executable can open retained additive data. The release builds a pinned repository commit in an isolated worktree.
- Restoring a database backup for normal rollback would reduce version mismatch concerns but would discard post-backup tasks and source evidence. Normal rollback is therefore forward-only; restore is reserved for migration failure or corruption.

## Risks And Mitigations

- **Fixture logs leak private source data**: aggregate and vertical logs are scanned for all fixture secrets, credentials, the private workspace host, URLs, goals, and raw payload markers. Fake Slack logs retain hashes only.
- **Old-binary qualification drifts**: the default previous ref is a full commit hash, can be overridden explicitly, and must expose schema-34 source automation reads before the rollback assertion passes.
- **Port/process collisions**: all daemons, sockets, databases, homes, and HTTP services are isolated; 19313-19315 are non-standard test ports with bounded readiness and cleanup traps.
- **Concurrent or timed tests become flaky**: the scripts poll durable states rather than sleeping for expected completion. The only provider delay is represented by persisted `Retry-After` state.
- **Release gate becomes too slow for iteration**: slice scripts remain runnable independently; the aggregate is the release boundary rather than the default development loop.

## Observability

- Every aggregate gate reports name, owner, command, duration, terminal status, and a retained log path on failure.
- Vertical assertions use durable source, route, attempt, task, binding, Attention, and canonical audit records.
- Failure diagnostics never print fixture credentials, raw Slack bytes, message URL, or rendered goal.
- Existing source automation health, failure categories, route attempts, and Process Console projections remain authoritative; FR-113 introduces no metric label.

## Operations / Release

- Build and test from a clean worktree with `./scripts/qa/test-slack-skill-automation-release.sh`.
- Use only an explicitly reviewed compatible previous ref for rollback qualification.
- Before production upgrade, drain work, run SQLite `quick_check`, and create an SQLite `.backup`.
- For stop-loss, set `reactionRouting: disabled` or suspend the Trigger/binding before changing binaries.
- Normal rollback keeps migrations 33-34 and all source/task evidence. If the old binary cannot safely open the additive schema, stop it and forward-fix with the current binary.
- Restore a backup only after a failed migration or proven corruption.

## Test Plan

- Unit/migration: populated schema-26 through schema-34 preservation and populated schema-33 route generation/change backfill.
- Integration: real Slack signature bytes, fake `chat.getPermalink`, double badge selection, concurrent convergence, retry/restart, Attention replay, and compatible binary rollback.
- Tauri: production command serialization for cataloged route, protected route, source event, task binding, task detail, and timeline.
- Frontend: Vitest coverage, Playwright workflows, axe, RBAC, redaction, dialog focus, reduced effects, and 640 px layout.
- Repository: current artifact builds, workspace tests, strict Clippy, documentation lint, owning slice scripts, guide contract, and diagnostic privacy scan.

## QA Docs

- `docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md`
- `scripts/qa/test-slack-skill-automation-release.sh`
- `scripts/qa/test-slack-skill-automation-vertical.sh`

## Acceptance Criteria

- Fresh daemon, CLI, GUI, and Web GUI artifacts pass all FR-107 through FR-112 owning gates.
- Two signed Slack badge events select distinct Skill/template/workflow tasks and retain complete provenance.
- Duplicate delivery, concurrent routing, rate-limit retry, and daemon restart converge without duplicate tasks.
- One permanent provider error becomes actionable Attention and resolves after reviewed replay.
- Populated Console/source data survives forward migration and compatible binary rollback.
- Real Tauri and fast browser UI acceptance prove provenance, role, privacy, accessibility, and narrow layout.
- The user guide and changelog cover setup, permissions, privacy, compatibility, stop-loss, upgrade, and rollback.
- The clean-tree aggregate gate and all repository quality gates pass.
