# FR-111: Source Automation Reliability, Policy, And Operations

## 优先级: P1

## 状态: Proposed

## 依赖: FR-110, Attention Inbox and Process Metrics closure artifacts

## 计划闭环产物

- `docs/design_doc/orchestrator/122-source-automation-reliability-operations.md`
- `docs/qa/orchestrator/159-source-automation-reliability-operations.md`
- `scripts/qa/test-source-automation-operations.sh`

## Background

FR-110 proves one successful badge-to-task route, but daily operation also requires explainable non-happy paths. Slack can retry delivery, rate-limit permalink resolution or become unavailable；credentials can expire；bindings/templates can be changed while an event is pending；daemon can restart during external calls or task creation。

Operators need bounded retry、Attention materialization、route replay、dry-run simulation and CLI diagnostics. Without these capabilities, a failed automation either disappears silently or requires direct database intervention。

## Goals

- 定义 durable route state machine、claim lease、retry/backoff、dead-letter 和 restart recovery。
- Respect Slack rate-limit responses and bounded transient retry without blocking workers indefinitely。
- Materialize actionable Attention only for operator-fixable states, with dedupe and auto-resolution。
- 提供 CLI list/get/watch/replay/simulate/suspend/resume/status surfaces。
- 暴露 privacy-safe metrics、projector/worker health 和 stable error taxonomy。
- 定义 template/binding update/delete、credential rotation and rollout/rollback policy。
- Prove route simulation and live routing use identical matcher/renderer logic。

## Non-goals

- GUI implementation。
- Posting route status back to Slack。
- Automatic task cancellation on reaction removal。
- Multi-region/distributed queue。
- Retaining raw Slack payloads indefinitely。

## Scope

### In scope

- Route states such as received/matched/resolving/rendered/creating/routed/retrying/needs_attention/ignored/failed。
- Atomic claim, lease expiry, attempt budget, jittered backoff and stable retry categories。
- `Retry-After` handling for Slack 429 and bounded timeout/network retry。
- Attention policies for invalid credential、binding ambiguity、exhausted retry and orphaned reservation。
- Audited replay from safe boundary with optimistic version/idempotency。
- CLI query/filter/follow and pure simulation using fixture/sample message URL。
- Metrics and System → Operations health extension where appropriate。
- Retention and redaction policy for route attempts/permalink metadata。

### Out of scope

- Editing templates/bindings through CLI beyond existing resource apply/edit/export commands。
- Hosted alert delivery。
- General external provider workflow engine。

## Route State Rules

- Transient network/429/5xx failures retry with bounded exponential backoff and provider `Retry-After` when valid。
- Invalid credential、forbidden message visibility、binding ambiguity、invalid template and exhausted attempts become `needs_attention` or stable terminal failure according to operator actionability。
- No-match due to intentionally unbound badge/channel is `ignored`, not Attention noise。
- Config revision selected at match time remains pinned through retries. Replay after an operator changes config requires explicit preview and a new audited route generation。
- Attention auto-resolves when replay/reroute reaches `routed` or an operator deliberately marks it ignored。

## Proposed CLI Outcomes

Exact command spelling is finalized in design against `orchestrator guide`, but the capability set must include:

- list/get route attempts by project, state, provider, binding and task;
- show safe match/render explanation and template revision;
- simulate a normalized reaction without mutation or Slack network access;
- replay an eligible route with expected version and reason;
- suspend/resume reaction automation at installation or binding scope;
- report worker backlog, oldest age, retry count and failure categories。

## Interfaces And Data Changes

- Additive route attempt fields/tables and indexes for claim/retry/status filters。
- Additive gRPC/CLI APIs with cursor pagination and bounded responses。
- Attention candidate stores source event/route/binding/task references, not raw message body/permalink。
- Process metrics add allowlisted dimensions only; installation/message/template names should be hashed or omitted where cardinality/privacy requires。
- Control mutations require canonical request context、role、reason、idempotency and audit result。

## Key Design Constraints

- Retry never creates a new task after an idempotency reservation has resolved to an existing task。
- At most one active lease owns a route attempt; stale lease recovery is restart-safe。
- Simulation cannot read secrets, contact Slack, create Attention or reserve task identity。
- Operator replay cannot bypass actor/channel/binding policy; privileged override, if any, must be a separate explicit action with consequence preview。
- Metrics/logs exclude message bodies, rendered goals, permalinks and credentials。
- CLI watch streams are bounded, cancellable and reconnectable with cursor semantics。
- Existing Process Console metrics failure must never gate task execution。

## Acceptance Criteria

- [ ] Transient timeout/5xx/429 routes retry with bounded schedule and ultimately converge to one task after recovery。
- [ ] Invalid credential、forbidden URL lookup、ambiguous binding and exhausted retry produce correct Attention/noise behavior。
- [ ] Daemon restart reclaims stale route lease and preserves pinned binding/template revision。
- [ ] CLI list/get/watch exposes stable states/reasons with pagination and no sensitive content。
- [ ] Simulation matches live binding/render results for identical safe inputs and produces no mutation/network request。
- [ ] Replay requires authorization、reason、expected version and idempotency; duplicate replay is safe。
- [ ] Attention resolves after successful replay and repeated failures deduplicate to one actionable item/version policy。
- [ ] Suspend/resume takes effect immediately and preserves in-flight/history semantics defined by design。
- [ ] Metrics expose accepted/matched/resolved/created/retried/failed counts and route latency without high-cardinality sensitive labels。
- [ ] Existing source router, Attention, audit and process metrics release tests remain green。

## QA Plan

- Deterministic fake clock/network fixtures for backoff, Retry-After, timeout and attempt exhaustion。
- Restart/lease tests around every durable state boundary。
- CLI contract tests for JSON output, filters, pagination, watch reconnect and redaction。
- Attention tests for create/dedupe/auto-resolve and non-actionable no-match silence。
- Metrics cardinality/privacy and projector-failure non-gating tests。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Retry storms overload Slack or workers | Attempt budget, jitter, Retry-After and per-installation occupancy |
| Attention becomes a feed of normal unbound emoji | Ignore intentional no-match; materialize only actionable policy failures |
| Replay uses changed template silently | Pin revision; require preview/new generation for config migration |
| CLI leaks private message URL | Safe projections and role-aware explicit deep-link reads |
