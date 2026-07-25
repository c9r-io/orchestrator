---
lifecycle: active
related_fr: FR-104
---

# Orchestrator - Process Console Operational Metrics And Local Dashboard

**Module**: Orchestrator  
**Status**: Approved  
**Related Plan**: FR-104 local, privacy-safe product metrics, projector health, bounded queries, performance fixtures, and System Operations UI  
**Related QA**: `docs/qa/orchestrator/151-process-console-operational-metrics.md`  
**Created**: 2026-07-14  
**Last Updated**: 2026-07-14

## Background

The Process Console already has durable timelines, Attention, handoffs, reviewed resume, sessions, source bindings, and an Attention-first shell. The remaining roadmap gap was an operator-facing answer to whether those loops are autonomous, responsive, recoverable, and healthy. Developer-console telemetry was neither durable nor an authoritative product surface.

This design adds a versioned, project-scoped local read model. Authoritative outcomes are derived from existing SQLite state. A narrow observation sink covers only measurements that cannot be reconstructed, such as UI page load, stream reconnects, timeline response size, and source-adapter duplicate detection.

## Goals

- Give every roadmap product metric an exact formula, time boundary, label set, and source.
- Keep metric collection local, bounded, low-cardinality, and independent from execution authority.
- Expose one read model through gRPC, CLI, Tauri, and System → Operations.
- Make rollups rebuildable and projector failure/lag visible.
- Prove deterministic values and release-mode budgets against large fixtures.

## Non-goals

- Hosted analytics, Prometheus, or an OpenTelemetry Collector requirement.
- Per-person productivity rankings or cross-installation aggregation.
- Replacing raw audit, event, timeline, or log evidence.
- Persisting prompts, transcripts, source bodies, evidence, handoff content, secrets, or raw error bodies.

## Scope

- In scope: project-scoped operational metric snapshots, optional observation rollups, projector health, maintenance commands, the Operations dashboard, and deterministic QA/performance gates.
- Out of scope: configurable business SLAs, billing/cost analytics, organization-wide aggregation, and arbitrary user-defined metric names or dimensions.

## UI Interactions

- Entry: visible "System" navigation destination, then the "Operations" section.
- Windows: "1h", "24h", and "7d"; the UI chooses a supported bucket for each window.
- States: loading, empty, error, fresh, stale, disabled, and partial historical coverage.
- The view is read-only for every role. Rebuild and prune remain admin CLI/RPC maintenance operations.

## API

- `ProcessMetricsGet(project_id, window, bucket)` returns `schema_version` and serialized `ProcessOperationsMetrics` JSON.
- `ProcessMetricRecord(project_id, metric_name, dimensions, value, source_key)` accepts only the closed observation and dimension allowlists.
- `ProcessMetricsRebuild(project_id)` rebuilds materialized rollups from retained observations.
- `ProcessMetricsPrune(retention_days)` removes only expired optional observations and rollups.
- CLI: `orchestrator metrics process --project {project} --window 24h --bucket 1h -o json`.
- CLI maintenance: `orchestrator metrics rebuild --project {project}` and `orchestrator metrics prune --retention-days {days}`.

Queries require a 1-128 character project ID. Durations accept positive `m`, `h`, or `d` values. The default maximum window is 30 days, supported buckets are `1m`, `5m`, `15m`, `1h`, `6h`, and `1d`, the bucket cannot exceed the window, and a response cannot exceed 744 buckets.

`ProcessOperationsMetrics` schema version 1 contains `project_id`, `[window_start, window_end)`, `bucket_seconds`, `generated_at`, optional `coverage_start`, `partial`, `collection_enabled`, sorted aggregates, and up to 64 projector-health records. Each aggregate includes labels, count/sum/min/max/value, optional ratio numerator/denominator, cumulative histogram counts, and optional buckets.

## Database Changes

Forward-only migration 32 adds:

- `process_metric_observations`: idempotent optional samples keyed by source family and internal correlation key.
- `process_metric_rollups`: materialized fixed-width buckets for accepted observations.
- `process_metric_projector_state`: last successful cursor, lag, bounded failure count/category, and freshness.
- project/state columns on `attention_changes` so Attention episode reconstruction remains project-scoped and replayable.

Indexes cover project/time/bucket reads. Existing task, item, command, audit, handoff, session, Attention, and source tables remain authoritative. Optional metric state has no foreign-key authority over execution.

## Metric Definitions

All windows are `[start, end)` and use daemon timestamps. Empty denominators produce value `0` while retaining numerator and denominator.

| Metric | Formula and window behavior | Source | Labels / privacy |
|---|---|---|---|
| `attention_open_total` | Count each `open` or `reopen` episode whose transition is in the window. A reopen starts a new episode. | `attention_changes` + `attention_items` | `kind`, `severity`; public low-cardinality |
| `attention_time_to_claim_seconds` | For each episode first claimed in the window, first claim time minus episode open/reopen time. Later claims do not add samples. | Attention changes | none; histogram |
| `attention_time_to_resolution_seconds` | For each episode resolved in the window, resolve time minus open/reopen time. Snoozed wall time remains part of resolution latency. | Attention changes | none; histogram |
| `process_human_attention_seconds` | Sum actionable `open`/`claimed` intervals clipped to the query window. `snoozed` and `resolved` intervals are excluded. | Attention changes | none |
| `process_autonomous_completion_ratio` | Denominator: tasks completed in the window. Numerator: those tasks with no successful pause/resume/retry/recover, Attention mutation, `resume.execute`, `session.send_input`, or `session.close` during their lifecycle. | tasks, events, action audit | none; numerator and denominator returned |
| `handoff_generation_seconds` | Successful terminal `handoff.generate` completion time minus its audit creation time, completed in the window. | `control_action_audit` | none; histogram |
| `handoff_to_productive_action_seconds` | First durable `step_started`, successful `resume.execute`, `session.writer_attach`, or `session.send_input` after a handoff snapshot, when that productive action lands in the window, minus snapshot creation. | handoffs, events, action audit, sessions | none; histogram |
| `resume_attempt_total` | Count terminal, non-reserved `resume.execute` audits completed in the window. Mode comes from the referenced plan; missing legacy mode is `unknown`. | action audit + resume plans | `mode`, `result` |
| `session_attachment_total` | Successful reader/writer attachment rows in the window plus terminal failed/denied writer-attach audits. | session attachments, sessions, tasks, action audit | `mode`, `result` |
| `source_event_deduplicated_total` | Count adapter-boundary duplicate observations in the window. Coverage is partial for history before instrumentation. | accepted optional observations | `provider` |
| `process_repeated_failure_rate` | Failed command runs divided by all command runs started in the window. A non-zero exit is failed. | command runs, items, tasks | none; numerator and denominator returned |
| `process_degenerate_loop_rate` | Item/phase groups with at least three consecutive failed command runs divided by all item/phase groups observed in the window. A success resets the streak. | command runs, items, tasks | none; numerator and denominator returned |
| `timeline_projection_seconds` | Server-side semantic timeline projection duration. | bounded optional observation | none; buckets |
| `timeline_response_bytes` | Serialized timeline response byte size. | bounded optional observation | none; buckets |
| `stream_reconnect_total` | UI reconnect outcomes at the stream boundary. | bounded optional observation | `page`, `result` |
| `ui_page_load_seconds` | UI page-load duration for the closed page enum. | bounded optional observation | `page` |

`attention_active{kind,severity}` is an instantaneous convenience gauge derived from active Attention rows. Attention history lacking the new resulting-state field sets `partial=true` instead of inventing precise episode semantics.

## Key Design

1. Authoritative metrics use query-time aggregation over indexed durable state; observations use rollups at `1m/5m/15m/1h/6h/1d`.
2. The observation sink rejects unknown names, unknown dimension keys/values, non-finite values, oversized source keys, and duplicate source keys.
3. Internal `source_key` may correlate idempotent writes but is never returned as an aggregate label.
4. Projector health is best effort. A failed batch increments a stable low-cardinality error code, updates lag, and retains the last successful cursor.
5. Timeline/source/UI observation failures warn but never fail task processing, event ingestion, timeline reads, or UI interaction.

## Alternatives And Tradeoffs

- Query all metrics directly: simplest, but unsuitable for UI-only duration/reconnect samples and repeated time-series scans.
- Roll up every authoritative event: faster reads, but introduces replay drift and another derived truth. The chosen hybrid keeps domain state authoritative and limits rollups to optional observations.
- Export to hosted analytics: easier cross-install reporting, but violates the local-first and privacy constraints.

## Risks And Mitigations

- SQLite load: bounded windows, supported buckets, indexed reads, 100,000-row scan caps, and release performance fixtures.
- Semantic drift: formulas live beside deterministic exact-value fixtures and this versioned contract.
- Identity/content leakage: fixed label allowlists, bounded values, stable error categories, and no content-bearing fields.
- False completeness: `coverage_start`, `partial`, and collection state are visible in the UI.

## Observability

- Logs: best-effort recording failures use structured warnings without metric source content; projector health stores only a stable error code.
- Metrics: all families in the table above, plus projector `cursor`, `lag_count`, `failure_count`, `last_success_at`, and `updated_at`.
- Tracing: no new distributed trace dependency; existing request correlation and audit surfaces remain separate forensic evidence.

## Operations / Release

- Config: `RuntimePolicy.spec.observability.process_metrics.{enabled,ui_telemetry_enabled,retention_days,max_window_days}`. Defaults are enabled, enabled, 90 days, and 30 days.
- Retention: pruning removes optional observations/rollups only; authoritative history follows its existing lifecycle policies.
- Rebuild: admin rebuild deletes and regenerates one project's rollups from retained accepted observations.
- Disable: stops new optional/UI samples and reports `collection_enabled=false`; authoritative snapshot metrics remain readable.
- Migration: additive migration 32 is restart-safe and does not rewrite existing task/session/source records.
- Rollback: deploy an older binary while retaining additive tables, or disable metric writers first. No down migration or authoritative data deletion is required.
- Compatibility: FR-088 `qa doctor` continues to read `task_execution_metrics`; agent-selection health remains in `core/src/metrics.rs`. Neither is renamed or reinterpreted.

## Test Plan

- Unit: duration/bucket validation, allowlists, idempotency/rebuild, exact formula golden, retention, cursor recovery, disable configuration, and large fixture.
- Integration: gRPC record/get/reject/rebuild against the isolated harness.
- UI: React loading/error/data behavior, production build, navigation-first read-only Playwright flow, window switch, axe, and existing reduced-motion coverage.
- Performance: 50,000 events plus 5,000 active Attention rows under 300 ms and 256 KiB for metrics; a 50,000-event timeline under 750 ms and 512 KiB. On 2026-07-14 both release fixtures passed (test wall times approximately 0.22 s and 0.30 s respectively, including fixture setup).

## QA Docs

- `docs/qa/orchestrator/151-process-console-operational-metrics.md`
- `scripts/qa/test-process-console-metrics.sh`

## Acceptance Criteria

- Every roadmap metric has a stable, privacy-classified definition and exact fixture.
- Bounded project queries work through gRPC, CLI, Tauri, and Operations UI.
- No high-cardinality or content-bearing value becomes an aggregate label or log field.
- Projection health, rebuild, retention, disable, migration, rollback, and performance are reproducible.
- Existing task information, logs, watch, `qa doctor`, and agent selection stay compatible.
