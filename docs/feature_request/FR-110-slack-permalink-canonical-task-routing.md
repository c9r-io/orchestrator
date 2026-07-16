# FR-110: Slack Permalink Resolution And Canonical Task Routing

## 优先级: P0

## 状态: Proposed

## 依赖: FR-107, FR-108, FR-109, FR-101 action audit envelope

## 计划闭环产物

- `docs/design_doc/orchestrator/121-slack-permalink-canonical-task-routing.md`
- `docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md`
- `scripts/qa/test-slack-reaction-task-routing.sh`

## Background

FR-107 至 FR-109 建立 reaction、template 和 binding 的纯契约。本 FR 交付第一个实际纵向结果：一个经过签名验证的 Slack badge event 选择唯一 binding，解析被 reaction 消息的 permalink，使用 SourceTaskTemplate 渲染 Skill goal，并通过 daemon canonical task path 创建一个任务。

Slack reaction event 只提供 channel/message timestamp。Permalink 解析需要 outbound Slack API token，因此网络失败、rate limit、credential rotation 和 URL validation 都位于新的可信边界。该边界直接触发付费 agent 工作，必须以 P0 安全和幂等要求实现。

## Goals

- 为 Slack source installation 配置 SecretStore-backed outbound API credential。
- 使用 Slack `chat.getPermalink` 将 message coordinates 解析为 HTTPS permalink。
- 将 permalink 与 trusted Skill invocation 交给 daemon-authoritative template renderer。
- 通过 existing queue/canonical task service 创建 template-selected workflow/workspace task。
- 建立 message/badge/binding-level task idempotency，抵抗 Slack retry、worker retry 和 daemon restart。
- 持久化 source event → route attempt → binding revision → template hash → task/request ID provenance。
- 在 Process timeline/Sources 中暴露安全、可导航的 source deep link。

## Non-goals

- 下载 Slack message body、attachments 或 thread replies。
- 从 message text 推断 Skill/workflow。
- 在 Slack 中回复、添加 reaction 或同步任务状态。
- Reaction removal cancellation。
- GUI template/binding editor。

## Scope

### In scope

- Slack API credential reference and rotation-safe resolution。
- Bounded HTTP client for permalink lookup, URL validation and safe error classification。
- Durable route state additions for selected binding/template revision and resolution result。
- Trusted render inputs and canonical task create/enqueue integration。
- Strong task idempotency reservation and crash recovery。
- SourceBinding/provenance and semantic timeline projection。
- Feature flag/rollout switch separating existing message routing from reaction automation。

### Out of scope

- General-purpose Slack Web API SDK。
- Message content retention/search。
- Route administration CLI beyond minimum inspection; FR-111 owns full operations。
- Management UI; FR-112 owns it。

## End-to-End Flow

1. Slack adapter validates and persists `reaction_added`, then acknowledges without outbound API work。
2. Source router claims the durable event and reads one atomic config generation。
3. Binding matcher resolves exactly one enabled SourceTaskBinding and records its revision/template reference。
4. Slack provider client resolves `channel + message_ts` to permalink using SecretStore credential。
5. Renderer validates URL/provider consistency and renders the exact template revision。
6. Router reserves a deterministic automation key before task creation。
7. Canonical task service creates/enqueues the selected workflow/workspace task with bounded goal and initial vars。
8. Route attempt, source binding, action audit and timeline provenance are linked by request ID/task ID。

## Interfaces And Data Changes

The design may extend Trigger webhook installation config with a dedicated outbound credential reference. Signing secret and API token must remain logically distinct even if both are stored in SecretStore.

Route persistence must record at least:

- source event ID and route attempt;
- binding name/revision and template name/content hash;
- normalized reaction and external message identity;
- permalink resolution status and a protected URL reference/value;
- deterministic automation idempotency key;
- canonical request ID and task ID;
- stable terminal/retry error code.

Task initial variables should include only bounded provider-neutral fields such as source event ID, source provider, reaction, message URL and template identity. Raw payload/token/message text are forbidden.

## Idempotency Contract

Default task identity is one task per:

```text
project + installation + message identity + reaction + binding identity
```

Slack delivery event ID remains a separate lower-level dedupe key. Retrying the same route, receiving the same event again, or restarting after task reservation must return the existing task. `reaction_removed` does not release the identity. A new run requires an explicit audited retry/manual action.

## Key Design Constraints

- Webhook response path performs no Slack API or task creation network work after durable persistence。
- Outbound token is resolved only inside daemon provider adapter; never returned by gRPC/GUI/CLI。
- Permalink must be HTTPS and match a documented Slack host policy; redirects and unexpected hosts fail closed。
- HTTP timeouts, response/body limits, TLS validation and rate-limit responses are bounded。
- Template render uses the exact selected revision and cannot read arbitrary source payload keys。
- Task mutation uses canonical service/action audit, not direct SQLite writes from router/adapter。
- Task creation reservation and route completion are crash-safe; ambiguous post-crash state reconciles by idempotency key。
- Existing fixed Trigger message routing remains unchanged outside the reaction feature gate。

## Acceptance Criteria

- [ ] A valid signed `reaction_added` for an allowed badge/channel/actor creates exactly one task using the selected Skill/workflow/workspace。
- [ ] Created task goal contains the configured Skill invocation and resolved Slack permalink, with no raw message body or token。
- [ ] Source event、route attempt、binding revision、template hash、request ID and task ID form one queryable provenance chain。
- [ ] Replaying delivery, retrying route, concurrent workers and daemon restart do not create a second task。
- [ ] Wrong/ambiguous binding、unknown actor、invalid URL host、missing credential and malformed Slack response create no task。
- [ ] Signing secret and outbound token can rotate without exposing values or requiring destructive state reset。
- [ ] Process timeline/Sources provides a role-aware clickable permalink and safe template/badge summary。
- [ ] Existing Slack thread/message routing and canonical task lifecycle regressions pass。
- [ ] Feature disable stops new reaction task routes while preserving existing tasks/source evidence。

## QA Plan

- Fake Slack HTTP server implements success, invalid JSON, error, redirect, timeout and rate-limit fixtures。
- Isolated daemon E2E sends real signed webhook bytes and verifies public APIs/database provenance without paid agents。
- Crash tests stop daemon after reservation and after task insertion, then verify single-task convergence。
- Concurrency test delivers duplicate events to multiple workers。
- Security tests verify token/redaction, host validation, role/channel policy and no raw message body。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Retry creates duplicate paid tasks | Durable automation reservation plus canonical idempotency key |
| Slack URL/token becomes sensitive output | Protected storage, role-aware projection and strict redaction |
| Outbound API delays Slack acknowledgement | Persist/ack first; asynchronous resolver |
| Router bypasses task governance | Reuse canonical service, queue, audit and source binding paths |
| Config changes during retry alter task meaning | Record binding revision/template hash before external resolution |
