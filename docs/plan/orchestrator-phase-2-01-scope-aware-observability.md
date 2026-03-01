# Orchestrator Phase 2 Task 01

## Title

按真实 step scope 修正控制面与 trace 的 item 归属表达

## Goal

让 `task watch`、`task trace --verbose`、相关 post-mortem 输出在展示 task-scoped 步骤时，不再把 anchor item 误呈现为“真实 item 归属”。

## Problem

当前系统虽然已经能正确执行 segment-based scope 调度，但观测层仍有一个语义残留：

- task-scoped segment 运行时会借用第一个 task item 作为 anchor
- 因此在事件和 trace 中，task-scoped 步骤可能仍带一个 `task_item_id`
- `task watch --verbose` / `task trace --verbose` 看到的是“有 item”，但这并不代表该步骤真的是 item-scoped

这会造成：

- 读输出的人误以为步骤是按 item fan-out 执行的
- 调试时对 scope 的理解被 anchor item 污染

## Scope

- 明确区分“执行锚点 item”和“真实步骤 scope”
- 为观测层提供更准确的 task-scoped / item-scoped 表达
- 更新对应的 CLI 和 trace QA 断言

## Out Of Scope

- 不改调度引擎的 segment 执行模型
- 不移除 task-scoped segment 对 anchor item 的内部依赖
- 不改数据库 schema

## Acceptance Criteria

1. `task trace --verbose` 不再把 task-scoped 步骤的 anchor item 呈现为真实 item 归属。
2. `task watch` 或相关运行期输出能明确体现 step 是 task-scoped 还是 item-scoped。
3. QA 文档中关于 verbose item 展示的说明不再需要“当前实现仍可能显示 anchor item”这类保守措辞。

## Suggested Verification

- `cargo test --lib scheduler::loop_engine`
- `cargo test --lib scheduler::trace`
- 使用一个 task-scoped workflow 和一个 item-scoped workflow 对照执行：
  - `./scripts/orchestrator.sh task watch <task_id>`
  - `./scripts/orchestrator.sh task trace <task_id> --verbose`
