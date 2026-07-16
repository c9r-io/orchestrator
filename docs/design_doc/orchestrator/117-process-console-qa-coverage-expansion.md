# Orchestrator - Process Console QA Coverage Expansion

**Module**: Orchestrator GUI
**Status**: Approved
**Related Plan**: Post-release functional analysis and risk-based expansion of unit, browser UI, accessibility, and coverage reporting
**Related QA**: `docs/qa/orchestrator/154-process-console-functional-ui-regression.md`
**Created**: 2026-07-16
**Last Updated**: 2026-07-16

## Background

The Process Console release suite proved the critical failure-to-recovery path, but its fast frontend coverage was concentrated around Attention, reviewed resume, Sessions, and Operations. Processes list ordering, complete Attention mutation behavior, Sources routing/replay, timeline reset and pagination, handoff risk confirmation, global navigation, and theme persistence had either only indirect coverage or no focused automated regression. Vitest also had no repository-owned coverage command, so contributors could not distinguish exercised modules from unmeasured UI code.

## Goals

- Add behavior-focused tests for every primary Console destination and the highest-risk state transitions.
- Exercise role boundaries for Attention, Sources, handoff/resume, and Session control in the rendered UI.
- Cover timeline snapshot, follow, deduplication, pagination, reset, and failure behavior at the hook boundary.
- Add a deterministic, locally reproducible frontend coverage command with honest source-wide collection.
- Keep browser tests isolated from real daemons, agent providers, and mutable developer data.

## Non-goals

- Claiming that browser mocks replace the live Tauri/gRPC vertical flow in QA-150.
- Raising a percentage by excluding untested runtime modules or adding implementation-detail assertions.
- Changing Process Console product behavior, RPC contracts, database state, or rollout policy.
- Testing legacy Wish/System CRUD exhaustively in this slice.

## Scope

- In scope: `gui/src/hooks/useTimeline.ts`, `gui/src/components/HandoffPanel.tsx`, `gui/src/pages/Sources.tsx`, `gui/tests/e2e/process-console.spec.ts`, Vitest coverage configuration, and Process Console QA documentation.
- Out of scope: daemon mutations, live Slack/GitHub deliveries, paid agents, release packaging, and production notification permission prompts.

## UI Interactions

- Pages/routes: `#/attention`, `#/processes`, `#/processes/{task_id}`, `#/sessions`, `#/sources`, and `#/system`.
- Key controls: "Claim", "Resolve", "Generate handoff", "Preview resume", "Execute reviewed plan", "重新路由", global `Cmd/Ctrl+1..5`, theme toggle, and keyboard-openable process rows.

## Interfaces And Data Changes

There are no runtime interfaces or data changes. The testing interface adds:

- `npm run test:coverage`: Vitest with V8 coverage over all runtime TypeScript/TSX sources except type-only, entry-point, test, and setup files.
- `gui/coverage/`: ignored generated output; `coverage-summary.json` remains locally inspectable.
- Expanded deterministic Tauri fixtures for task ordering, source routing, Attention mutations, handoff snapshots, and System navigation.

## Key Design

1. Unit/component tests own state-machine edges close to the code: timeline cursor/reset behavior, risky resume confirmation, and source authorization/filtering.
2. Playwright owns visible cross-component journeys: navigation, keyboard reachability, ordering, mutation consequences, source correlation, theme, and handoff discoverability.
3. Existing live QA remains authoritative for Tauri/gRPC serialization, daemon authorization, persistence, and audited mutations.
4. Coverage includes untested source files so the report exposes remaining risk instead of reporting only imported modules.

## Alternatives And Tradeoffs

- A screenshot-heavy suite would cover more pixels but provide weak state-transition evidence. Role/label assertions and captured typed commands are more stable and diagnostic.
- Running every browser test against a daemon would improve integration depth but slow feedback and create fixture contention. The existing isolated live vertical flow remains the smaller integration proof.
- A high global percentage threshold would fail immediately on legacy Expert/Wish surfaces and encourage low-value tests. This change establishes measurement first; future work can add per-domain thresholds as coverage grows.

## Risks And Mitigations

- **Mock drift**: browser fixtures could diverge from Tauri contracts.
  - Mitigation: QA-150 retains production Tauri-handler/gRPC coverage and typed frontend structures remain shared.
- **Exact-count drift in release docs**: added tests can make release evidence stale.
  - Mitigation: linked QA docs and the release acceptance references are updated together.
- **Coverage percentage misinterpretation**: a source-wide number is lower than imported-file-only coverage.
  - Mitigation: documentation states the collection scope and treats uncovered modules as a prioritized backlog.

## Observability

- Test output reports file/scenario counts and failing scenario names.
- Playwright retains traces only on failure.
- V8 emits a text report and `coverage-summary.json`; generated reports are excluded from git.
- Fixtures capture command names and bounded IDs/idempotency keys, never prompts, transcripts, secrets, or raw source payloads.

## Operations / Release

- Config: no runtime configuration changes.
- Migration / rollback: no migration; rollback removes the added tests, coverage dependency/script, and documentation updates.
- Compatibility: tests run against the existing React 18, Vitest 4, Playwright, and Tauri mock boundary.

## Test Plan

- Unit/component: 21 Vitest scenarios across routes, roles, Attention reconciliation, evidence, Operations, timeline following, handoff safety, and Sources authorization.
- E2E: 15 Playwright journeys across primary navigation, recovery, Sessions, Sources, responsiveness, accessibility, preferences, and role gates.
- Build: TypeScript plus production Vite build.
- Live integration: retain `./scripts/qa/test-process-console-vertical-flow.sh` as the daemon-backed authority.

## QA Docs

- `docs/qa/orchestrator/154-process-console-functional-ui-regression.md`
- `docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md`

## Acceptance Criteria

- `npm run test:coverage` passes and collects all eligible GUI runtime sources.
- 21 Vitest scenarios and 15 Playwright journeys pass deterministically.
- Processes, Attention mutations, Sources, handoff safety, timeline resilience, global navigation, roles, and accessibility have explicit automated assertions.
- No test calls an external agent/provider or mutates the developer's running demo daemon.
