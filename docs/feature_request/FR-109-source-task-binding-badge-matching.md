# FR-109: Source Task Binding And Badge Matching Resource

## 优先级: P1

## 状态: Proposed

## 依赖: FR-107, FR-108

## 计划闭环产物

- `docs/design_doc/orchestrator/120-source-task-binding-badge-matching.md`
- `docs/qa/orchestrator/157-source-task-binding-badge-matching.md`
- `scripts/qa/test-source-task-binding.sh`

## Background

一个 Slack installation 需要同时支持多个 badge，例如实现、代码分析和文档工作。现有 source router 要求 installation 只解析到一个 Trigger，且该 Trigger 只有一个固定 action，无法把 reaction name 安全地映射到不同的 SourceTaskTemplate。

本 FR 引入 project-scoped SourceTaskBinding。Binding 引用现有 Slack Trigger 作为 authenticated installation boundary，使用 exact normalized reaction、target kind、channel 和 actor role 选择 SourceTaskTemplate。它只负责配置与 deterministic match，不调用 Slack API 或创建任务。

## Goals

- 新增 native `SourceTaskBinding` resource with apply/get/describe/delete/export lifecycle。
- 支持 `triggerRef + eventKind + reaction + targetKind + channel policy + actor role policy + templateRef`。
- 在 apply 时检测明显重复、不可达或冲突的 binding。
- 在 route/simulate 时保证 exactly-one-match；zero/multiple matches 返回稳定原因，不猜测。
- 支持 suspend/resume 和 atomic hot reload。
- 产生不含 message URL/body 的 safe match explanation。

## Non-goals

- Slack permalink resolution。
- Template rendering or task creation。
- Regex、wildcard emoji 或基于 message text 的动态 Skill selection。
- Hosted Slack OAuth installation management。
- Reaction removal-driven task cancellation。

## Scope

### In scope

- Binding resource model、project config、cross-resource validation and projection。
- Exact reaction normalization and match precedence policy。
- Optional channel allowlist and trusted actor-role allowlist。
- Config conflict detector and pure route simulation/match service。
- Suspend/resume mutation through canonical audit envelope。
- Reference-safe template/trigger deletion behavior。

### Out of scope

- Network/API client。
- Route persistence and retry lifecycle。
- Task idempotency row/schema。
- React management UI。

## Proposed Manifest

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: slack-code-analysis
spec:
  triggerRef: slack-main
  match:
    eventKind: reaction_added
    reaction: agent-analyze
    targetKind: message
    channels: [C01234567]
  templateRef: analyze-from-slack
  allowedActorRoles: [operator, admin]
  suspend: false
```

## Matching Semantics

1. Resolve the referenced Trigger in the same project。
2. Verify Trigger is an enabled Slack source installation and installation identity matches the event。
3. Normalize reaction to the canonical no-colon form and require exact case-sensitive provider name semantics。
4. Require message target and optional channel membership。
5. Resolve external actor through existing `actorRoles`; unknown/unallowed roles do not match and must be explainable。
6. Collect enabled matching bindings. Continue only when exactly one remains。

Specificity does not break ties. Two enabled bindings that can match the same event are a configuration error, because implicit precedence could select a privileged Skill unexpectedly.

## Interfaces And Data Changes

- Add SourceTaskBinding to resource dispatch/config/proto/CLI aliases。
- Add a typed pure `match/simulate` service returning match result, candidate IDs and safe reason codes。
- Existing Trigger schemas and fixed action remain valid. A Slack installation without SourceTaskBinding keeps existing DD-109 behavior unless an explicit rollout flag selects reaction automation mode。
- Binding revision/config generation must be available to FR-110 for provenance and idempotency。

## Key Design Constraints

- Binding and referenced Trigger/template must share project scope。
- `triggerRef` cannot reference cron/filesystem/non-Slack triggers。
- `reaction` cannot be empty, colon-wrapped, wildcarded or source-provided as a template variable。
- Channel and role restriction defaults must be explicit and fail-safe; design doc must choose a secure default for omitted lists。
- Unknown actor does not inherit operator/admin。
- Apply-time validation uses an atomic candidate config and cannot leave partial active state。
- Simulation never creates task, resolves permalink or contacts Slack。
- Every mutation is audited; read-only clients may inspect only safe binding metadata。

## Acceptance Criteria

- [ ] Valid binding applies and round-trips with stable revision through daemon restart。
- [ ] Exact reaction on allowed channel/role selects the referenced template。
- [ ] Wrong reaction、target kind、channel、role、installation or suspended state produces no match with stable reason。
- [ ] Two overlapping enabled bindings are rejected at apply, or route-time ambiguity fails closed if introduced by a race/legacy config。
- [ ] Trigger/template cross-project、missing or invalid-kind references fail validation。
- [ ] Suspend/resume is immediate, audited and does not require daemon restart。
- [ ] Deleting a referenced Trigger/template or force-deleting a binding follows explicit reference policy。
- [ ] Simulation returns the same binding decision as live matching for identical normalized input。
- [ ] Existing Slack fixed-action routing remains compatible when reaction automation is not enabled。

## QA Plan

- Unit matrix for reaction/channel/role/target/installation matching。
- Resource tests for apply conflicts, cross-references, hot reload, delete and round-trip。
- CLI simulation fixtures for one-match, no-match and multiple-match outcomes。
- Concurrency test proves route sees one immutable config generation during simultaneous apply。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Overlapping badge rules choose wrong Skill | No implicit precedence; exactly-one-match |
| Omitted channel/role widens authority unexpectedly | Secure explicit defaults and apply warnings/errors |
| Config update races with routing | Atomic active snapshot plus recorded binding revision |
| Binding resource tightly couples core to Slack | Provider-neutral event matcher; Slack-specific adapter owns normalization |
