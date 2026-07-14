# FR-104: Process Console Operational Metrics And Local Dashboard

## 优先级: P1

## 状态: Proposed

## 依赖: FR-095 through FR-100 closure artifacts, FR-101, FR-102, FR-103

## 计划闭环产物

- `docs/design_doc/orchestrator/114-process-console-operational-metrics.md`
- `docs/qa/orchestrator/151-process-console-operational-metrics.md`
- `scripts/qa/test-process-console-metrics.sh`

## Background

The roadmap names product metrics for Attention latency, autonomous completion, handoff productivity, resume/session outcomes, source deduplication, and degenerate loops. The current implementation records execution metrics and local UI events, but the named Process Console metrics do not exist as stable definitions or query surfaces. UI telemetry currently writes a small event set to the developer console, and System has no process-operations dashboard.

The project is local-first, so the missing capability should be implemented as durable, privacy-safe local observability rather than requiring a hosted analytics provider.

## Goals

- Define precise numerators, denominators, timestamps, labels, and exclusion rules for every roadmap metric.
- Derive metrics from authoritative durable events/tables and bounded UI action telemetry without storing content.
- Provide project-scoped snapshot/time-window queries through gRPC and CLI.
- Add a local System → Operations dashboard for Attention load, response latency, recovery outcomes, session re-entry, source routing, and loop health.
- Add projection failure counters and timeline latency/response-size histograms.
- Establish deterministic performance fixtures and regression budgets for large timelines and active Attention queues.

## Non-goals

- Requiring Prometheus, OpenTelemetry Collector, or a hosted analytics service.
- Collecting prompt text, transcript/input bytes, source bodies, evidence contents, handoff briefings, or secrets.
- High-cardinality labels for task, actor, session, source event, or request IDs in aggregate series.
- Replacing raw audit/event queries used for forensic investigation.
- Defining business SLA targets without operator configuration.

## Scope

### In scope

The following metric families from the roadmap:

- `attention_open_total{kind,severity}`;
- `attention_time_to_claim_seconds`;
- `attention_time_to_resolution_seconds`;
- `process_human_attention_seconds`;
- `process_autonomous_completion_ratio`;
- `handoff_generation_seconds`;
- `handoff_to_productive_action_seconds`;
- `resume_attempt_total{mode,result}`;
- `session_attachment_total{mode,result}`;
- `source_event_deduplicated_total{provider}`;
- repeated-failure and degenerate-loop rates.

Also in scope: timeline projection duration/response size, Attention/source projector failures and lag, stream reconnect counts, and dashboard freshness/error state.

### Out of scope

- Organization-wide aggregation across independent local installations.
- User productivity ranking or per-actor performance scoring.
- Billing/cost analytics.

## Interfaces And Data Changes

Define a versioned `ProcessOperationsMetrics` read model with project and bounded time-window filters. The design should choose between query-time aggregation over indexed authoritative tables and an additive materialized rollup table. Rollups must be rebuildable from durable source records where feasible and use forward-only migrations.

Add read-only gRPC/CLI surfaces, proposed as:

- `ProcessMetricsGet(project_id, window, bucket)`;
- `orchestrator metrics process --project {project} --window 24h -o json`.

The GUI dashboard consumes the same read model. UI-only telemetry must enter through a bounded local sink rather than `console.info`, with a closed event enum and field allowlist. Request/task IDs may be used for correlation records but never as aggregate labels.

## Metric Semantics

- Attention durations use durable item/action timestamps and define reopen behavior explicitly.
- Human-attention time counts only intervals in actionable states, not total task duration.
- Autonomous completion excludes tasks that required a successful human mutation and reports the denominator.
- Handoff-to-productive-action ends only on a durable resume/session/input/task-progress event, not button click.
- Resume/session/source counters use terminal result codes from authoritative audit records.
- Degenerate-loop rate uses the existing anomaly/event semantics and a documented task/cycle denominator.

## Key Design Constraints

- Metrics never become execution authority and cannot block task processing.
- Projection/rollup failure retains its cursor and exposes lag/error without silently dropping records.
- Dashboard queries are read-only, bounded, indexed, cancellable, and protected as control-plane read traffic.
- Cardinality and retention limits are explicit.
- Clock calculations use daemon timestamps; client clocks are diagnostic only.
- Disabling metrics collection stops new optional UI telemetry without deleting authoritative task/audit history.

## Acceptance Criteria

- [ ] Every roadmap metric has a documented formula, source records, labels, window semantics, reopen/retry behavior, and privacy classification.
- [ ] Deterministic fixtures produce exact expected values for Attention, autonomous completion, handoff, resume, session, source, and loop metrics.
- [ ] Project-scoped gRPC and CLI queries return bounded snapshots and reject invalid windows/buckets.
- [ ] System → Operations renders Attention volume/latency, recovery outcomes, session/source activity, autonomous ratio, and loop health with empty/error/freshness states.
- [ ] No prompt, transcript, source body, evidence content, handoff content, secret, or high-cardinality identifier appears in aggregate labels or logs.
- [ ] Timeline projection latency/size and projector failure/lag metrics are observable with stable names.
- [ ] A populated fixture with a large event history meets documented query/projection budgets and does not regress existing TaskInfo/log/watch behavior.
- [ ] Metrics rollup/rebuild, migration, retention, feature-disable, and rollback behavior are reproducible.
- [ ] Existing `qa doctor` and agent-selection metrics remain backward compatible and semantically distinct.

## QA Plan

- Pure unit tests for formulas, buckets, reopen/retry semantics, cardinality, and redaction.
- Repository tests for indexed windows, rollup idempotency/rebuild, cursor failure recovery, and retention.
- Isolated daemon script seeds deterministic events through public/domain-safe fixtures and compares exact CLI JSON.
- Browser tests cover dashboard loading, empty/error states, time windows, reduced motion, and read-only access.
- Performance test records timeline and dashboard latency against fixed fixture sizes with explicit budgets.

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Metrics definitions drift from product behavior | Versioned formulas and fixture-derived golden values |
| Aggregation adds SQLite load | Indexed bounded windows, optional rollups, cancellation, performance gates |
| High-cardinality data leaks identity | Fixed label allowlist and automated cardinality/privacy tests |
| Dashboard is mistaken for forensic truth | Link aggregate panels to existing bounded audit/read surfaces |
| UI telemetry is lost when the app closes | Durable bounded local sink for accepted event types; authoritative outcomes remain daemon-derived |
