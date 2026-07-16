# FR-107: Slack Reaction Source Event Contract

## 优先级: P1

## 状态: Proposed

## 依赖: FR-099 closure artifacts (DD-109, QA-146)

## 计划闭环产物

- `docs/design_doc/orchestrator/118-slack-reaction-source-event-contract.md`
- `docs/qa/orchestrator/155-slack-reaction-source-event-contract.md`
- `scripts/qa/test-slack-reaction-source.sh`

## Background

现有 Slack adapter 只把 human message、allowlisted command 和 system event 映射到 provider-neutral source contract。Slack `reaction_added` payload 不包含普通 message event 的 `channel`、`ts`、`text` 结构：被 reaction 的消息坐标位于 `event.item.channel` 和 `event.item.ts`，badge 名位于 `event.reaction`。当前 normalizer 因此会把 reaction 当作 `System`，无法被后续 badge routing 安全消费。

本 FR 只建立 durable reaction 语义和查询证据，不创建任务。这样可以先冻结 provider-neutral contract、兼容与隐私边界，再让后续 FR 在稳定输入上实现模板和路由。

## Goals

- 为“actor 对 external artifact 添加 reaction”定义 provider-neutral normalized event contract。
- 正确解析 Slack `reaction_added` 的 actor、reaction name、message target、channel、timestamp 和 occurrence time。
- 保留现有签名校验、persist-before-ack、event ID 去重、payload size 和 redaction 边界。
- 只接受 message target；file/file_comment 或不完整 target 进入明确的 ignored/rejected 状态。
- 让 CLI/gRPC/Sources UI 可以安全识别 reaction event，而不暴露 raw payload。

## Non-goals

- 解析 Slack permalink。
- 选择 badge binding、渲染 Skill 或创建任务。
- 获取、存储或展示 Slack 消息正文、附件或 thread transcript。
- 处理 `reaction_removed` 的任务取消语义。
- 为非 Slack provider 实现 adapter；provider-neutral model 必须允许未来复用。

## Scope

### In scope

- Additive `SourceEventKind`/normalized reaction value object。
- Slack event envelope parsing and validation。
- Stable external message reference based on channel + message timestamp。
- Database JSON round-trip、source list/get projection 与 timeline provenance。
- Unit/integration fixtures for valid, retry, malformed, non-message and bot/self reaction cases。

### Out of scope

- New task/template/binding tables。
- Slack outbound API token or network client。
- Actor authorization beyond preserving authenticated external actor identity。
- GUI automation management screens。

## Interfaces And Data Changes

建议新增 provider-neutral reaction descriptor，概念结构如下：

```text
ReactionRef {
  name,
  target: ExternalArtifactRef(kind="message", external_id=<stable message ref>, url=None)
}
```

`NormalizedSourceEvent` 必须能区分 `reaction_added` 与普通 artifact update。Slack-only envelope 字段不得泄漏到 core public model；adapter 将 `item.channel`/`item.ts` 编码为有界、可验证的 external message reference。

现有 `source_events` normalized payload 可用 additive JSON 字段承载；若 schema 需要新增可查询列，migration 必须 forward-only、restart-safe，并保留旧行反序列化兼容。

## Key Design Constraints

- `event_id` 继续是 delivery identity；相同 event ID 的重试必须返回同一个 source event。
- Reaction name 必须规范化为不带冒号的 Slack emoji name，并限制长度/字符集。
- Actor 取 `event.user`；缺失 actor、item、channel、message ts 或 reaction 时 fail closed。
- `event.item.type != message` 不得进入自动化路由。
- Normalization 不进行网络调用，因此 webhook acknowledgement 不依赖 Slack Web API。
- Raw message coordinates 可以作为受限 provenance，但不得写入 metric labels 或非必要日志。
- Existing message、command、interactive action 和 URL verification fixtures 必须保持行为不变。

## Acceptance Criteria

- [ ] 一个签名有效的 Slack `reaction_added` message event 被持久化为 typed reaction source event。
- [ ] Normalized event 精确保留 external actor、规范 reaction name、message target、channel/message identity 和 occurred_at。
- [ ] 相同 Slack `event_id` 重放不会新增 source row 或 routing attempt。
- [ ] Missing actor/reaction/item coordinates、oversized body、stale timestamp 和 invalid signature 按稳定错误码失败。
- [ ] File/file_comment reaction 不创建可路由 reaction；状态与原因可查询。
- [ ] Reaction event 本身不会触发现有 Trigger fixed action，也不会错误 append 到 thread-bound task。
- [ ] Source list/get 与 Process provenance 展示 bounded reaction metadata，不展示 raw payload/message body。
- [ ] Existing QA-146 Slack message/command scenarios 全部回归通过。
- [ ] 非 Slack fixture 可以构造同一 provider-neutral reaction model，core 类型不依赖 Slack 字段名。

## QA Plan

- Unit tests 覆盖 Slack payload normalization、name/target validation 和 serde round-trip。
- Isolated daemon script 发送签名 webhook，验证 durable ack、dedupe 和 public source reads。
- Negative fixtures 覆盖 missing fields、non-message item、bot/self actor、invalid signature 和 stale timestamp。
- Regression 调用 `scripts/qa/test-source-events-slack.sh`。

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Reaction 被当成普通 message 并立即创建错误任务 | Dedicated event kind；FR-109 binding 前保持 non-mutating |
| Slack-specific coordinates 污染 core | Adapter 映射为 provider-neutral artifact reference |
| Raw payload 或 URL 泄漏 | Typed allowlist projection；不存正文；日志仅使用 hash/ID |
| 新 enum variant 破坏旧数据库行 | Additive serde compatibility 与 populated migration fixture |
