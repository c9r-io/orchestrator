# QA-115: Agent 间通信接口草案 — Mailbox + Session Control Plane

> 验证对象：`docs/design_doc/orchestrator/75-agent-mailbox-session-communication.md`
> 来源：FR-065

## 概述

本 QA 文档验证 FR-065 接口草案的设计完整性。FR-065 的交付物是正式接口草案文档，不涉及代码实现。验证范围为设计文档的覆盖度与一致性。

## 前置条件

- 设计文档 `docs/design_doc/orchestrator/75-agent-mailbox-session-communication.md` 已创建

## 场景

### S1: 设计文档完整性 — Mailbox 与 Session 双层覆盖

**步骤**：

1. 打开设计文档
2. 确认文档明确区分 Mailbox（异步 store-and-forward）与 Session（实时 interactive stream）两层抽象
3. 确认各层有独立的功能定位、状态模型、CRD 草案

**预期**：文档第 2、4、5 节分别覆盖总体分层、Mailbox 抽象、Session 抽象，无遗漏。

### S2: 标识优先级 — `session_id` 为主，`pid` 为辅

**步骤**：

1. 检查设计文档第 3 节标识模型
2. 确认 `session_id`/`run_id` 标记为"高"稳定性
3. 确认 `pid` 标记为"低"稳定性
4. 确认文档明确约束"禁止以 `pid` 作为持久化通信对象主键"

**预期**：标识优先级表完整，`pid` 降级约束明确。

### S3: CRD 字段覆盖

**步骤**：

1. 检查 AgentSession CRD 草案（第 5.4 节），确认包含：`spec.agent_id`, `spec.task_id`, `spec.task_item_id`, `spec.step_id`, `spec.mode`, `spec.access.*`, `spec.retention.*`, `status.session_id`, `status.state`, `status.pid`, `status.run_id`
2. 检查 AgentMailbox CRD 草案（第 4.4 节），确认包含：`spec.agent_id`, `spec.retention_days`, `spec.max_pending_messages`, `spec.delivery_mode`, `status.unread_count`, `status.in_flight_count`
3. 检查 AgentMessage 模型（第 4.2 节），确认包含：`spec.sender`, `spec.receiver`, `spec.message_type`, `spec.subject`, `spec.reply_to`, `spec.correlation_id`, `spec.ttl_secs`, `spec.require_ack`, `spec.payload`

**预期**：所有建议字段均有文档记录。

### S4: CLI 形状覆盖

**步骤**：

1. 检查 Mailbox CLI（第 6.1 节），确认包含：`list`, `send`, `ack`, `reply`, `wait`, `get`
2. 检查 Session CLI（第 6.2 节），确认包含：`list`, `open`, `attach`, `send-input`, `read`, `detach`, `close`, `inspect`, `resolve`
3. 确认 `--pid` 仅作查询过滤，写操作不得只接受 `--pid`

**预期**：所有建议 CLI 子命令均有文档记录，`pid` 约束明确。

### S5: gRPC API 形状覆盖

**步骤**：

1. 检查 Mailbox RPC（第 7.1 节），确认包含 6 个 RPC 定义
2. 检查 Session RPC（第 7.2 节），确认包含 8 个 RPC 定义
3. 确认关键返回字段有文档说明

**预期**：所有建议 RPC 均有文档记录。

### S6: 实施阶段与依赖

**步骤**：

1. 检查第 8 节实施顺序
2. 确认 4 个阶段按依赖排序：观测 → Mailbox → Session 控制 → 高阶能力
3. 确认各阶段范围边界清晰

**预期**：实施阶段表完整，依赖关系合理。

### S7: 实施前对齐约束

**步骤**：

1. 检查第 11 节
2. 确认文档显式记录"后续实现前，必须再次与用户进行需求对齐"

**预期**：约束明确记录，避免需求记忆漂移。

## Checklist

- [x] S1: Mailbox 与 Session 双层抽象已文档化
- [x] S2: 标识优先级 `session_id` > `pid` 已明确
- [x] S3: AgentSession、AgentMailbox、AgentMessage CRD 字段覆盖完整
- [x] S4: Mailbox/Session CLI 形状覆盖完整
- [x] S5: gRPC API 形状（Mailbox 6 RPC、Session 8 RPC）覆盖完整
- [x] S6: 实施阶段依赖排序合理
- [x] S7: 实施前对齐约束已记录

---

**QA 结果**：全部通过（2026-03-21）
