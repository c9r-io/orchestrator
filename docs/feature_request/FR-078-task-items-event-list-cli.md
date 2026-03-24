# FR-078: Task Items 与 Event List CLI 命令

## 优先级: P1

## 状态: Proposed

## 背景

当前部分运维场景（self-evolution 候选监控、QA 安全跳过验证、daemon 热重载观测）仍需直接查询 SQLite 数据库。这违反了 CLI 作为唯一用户交互面的设计原则，增加了用户操作门槛，也暴露了内部数据库 schema。

具体缺失的能力：
1. 无法通过 CLI 查看某个 task 下所有 item 的详细状态（label、source、status）
2. 无法通过 CLI 查看某个 task 的事件流（event_type、payload）

## 需求

### 1. `orchestrator task items <task_id>`

列出指定 task 下所有 task item 的详细信息。

```bash
orchestrator task items <task_id> -o table
orchestrator task items <task_id> -o json
```

输出字段：
- `id` — item 唯一标识
- `label` — 人类可读标签（如文件路径、候选名）
- `source` — 来源（static / dynamic / generated）
- `status` — 当前状态（unresolved / running / resolved / failed / blocked / skipped）
- `order_no` — 执行顺序
- `created_at` — 创建时间

支持过滤：
- `--status <STATUS>` — 按状态过滤
- `--project <PROJECT>` — 项目范围

### 2. `orchestrator event list --task <task_id>`

列出指定 task 的事件流。

```bash
orchestrator event list --task <task_id>
orchestrator event list --task <task_id> --type step_skipped
orchestrator event list --task <task_id> -o json --limit 50
```

输出字段：
- `id` — 事件 ID
- `event_type` — 事件类型（step_started、step_finished、items_generated、step_skipped、self_restart 等）
- `payload` — 事件负载（JSON，table 模式下截断显示）
- `created_at` — 时间戳

支持过滤：
- `--type <EVENT_TYPE>` — 按事件类型过滤（支持前缀匹配，如 `self_restart%`）
- `--limit <N>` — 最多返回 N 条（默认 50）
- `--task <TASK_ID>` — 必填，按 task 过滤

## 验收标准

- [ ] `orchestrator task items <task_id>` 输出所有 item 及其状态
- [ ] `orchestrator task items <task_id> --status running` 正确过滤
- [ ] `orchestrator event list --task <task_id>` 输出事件流
- [ ] `orchestrator event list --task <task_id> --type step_skipped` 正确过滤
- [ ] `-o json` 输出完整 JSON，`-o table` 输出人类可读表格
- [ ] docs/showcases/ 中的 PARTIALLY_REPLACEABLE sqlite 查询全部替换为 CLI 命令
- [ ] docs/showcases/ 中至少 4 个 DEBUG_ONLY 事件查询替换为 CLI 命令

## 风险

- gRPC proto 需新增 RPC 方法，需协调 proto → daemon → CLI 三层变更
- event payload 可能包含大量 JSON，table 模式需合理截断
