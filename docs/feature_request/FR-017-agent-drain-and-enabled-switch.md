# FR-017 Agent Drain 与 Enabled 开关

- ID: `FR-017`
- Priority: `P1`
- Status: `Proposed`
- Owner: `orchestrator core`

## 背景

当前 orchestrator 的 agent 一旦在 workflow 中声明并随 task 启动，就会持续参与调度直至 task 完成或被强制取消。运维人员和开发者在以下场景下缺乏优雅的中间控制手段：

- 需要临时下线某个 agent（例如其依赖的外部服务维护中），但不希望取消整个 task。
- 希望在多 agent 协作的 workflow 中逐步收缩活跃 agent 范围，而不是一刀切停止。
- 调试时希望仅保留特定 agent 运行，其余暂停。

Kubernetes 提供了 `kubectl drain` / `kubectl cordon` 的成熟范式：将节点标记为不可调度，等待现有工作完成后安全退出。本 FR 旨在为 orchestrator agent 引入类似机制。

## 问题陈述

- 当前无法在 task 运行中优雅地停止单个 agent 的调度，只能取消整个 task 或等待自然结束。
- 缺少 agent 级别的 `enabled/disabled` 状态管理。
- 运维场景下无法做到"排空后下线"（drain），存在中断正在执行步骤的风险。

## 目标

- 为 agent 引入 `enabled` 开关，支持在 manifest 中声明以及在运行时通过 CLI 动态切换。
- 实现 `drain` 语义：将 agent 标记为不可调度新 item，但允许其完成当前正在执行的 item/step 后再退出调度循环。
- 提供 CLI 命令供用户查看和操控 agent 状态。

## 非目标

- 不在本 FR 中实现 agent 的自动健康检查或自动 drain（属于未来自愈能力）。
- 不改变 workflow 级别的暂停/取消语义（`task pause` / `task cancel` 保持不变）。
- 不引入 agent 热替换或在线重载能力。

## 范围

### In

- Agent manifest 中的 `enabled` 字段（默认 `true`）
- CLI：`task agent drain <task-id> <agent-name>`、`task agent cordon/uncordon`、`task agent list`（含状态列）
- Scheduler 调度循环中检查 agent enabled 状态，跳过 disabled/draining agent 的新 item 分发
- Agent 状态机：`Active → Cordoned → Draining → Drained`
- gRPC 控制面支持 agent 状态查询与变更

### Out

- Agent 自动缩扩容策略
- Agent 进程级别的信号管理（SIGTERM 等，属于 daemon 生命周期治理 FR-005 范畴）

## 需求

### 1. Agent Enabled 开关

- Agent CRD/manifest 中新增 `enabled: bool` 字段，默认为 `true`。
- `enabled: false` 的 agent 在 task 启动时即被跳过，不参与调度。
- 运行时可通过 CLI 或 gRPC 切换 enabled 状态。

### 2. Drain 语义

- `drain` 操作将 agent 标记为 `Draining` 状态：
  - 调度器不再向该 agent 分发新 item。
  - 该 agent 当前正在执行的 item/step 允许继续完成。
  - 所有进行中工作完成后，agent 状态转为 `Drained`。
- `uncordon` 操作将 agent 恢复为 `Active` 状态，重新参与调度。

### 3. CLI 命令

- `orchestrator task agent list <task-id>`：显示所有 agent 及其状态（Active / Cordoned / Draining / Drained）。
- `orchestrator task agent drain <task-id> <agent-name>`：触发 drain。
- `orchestrator task agent cordon <task-id> <agent-name>`：标记为不可调度但不等待排空。
- `orchestrator task agent uncordon <task-id> <agent-name>`：恢复调度。

### 4. 状态可观测性

- `task status` 输出中应体现各 agent 的当前状态。
- Draining 过程中应有日志输出，指示剩余进行中 item 数量。

## 验收标准

- 在 manifest 中设置 `enabled: false` 的 agent 不参与 task 调度。
- CLI `drain` 后 agent 不再接收新 item，完成当前工作后状态变为 `Drained`。
- `uncordon` 后 agent 恢复正常调度。
- `task agent list` 正确显示所有 agent 及其实时状态。
- 对正在执行的 step 无中断，排空过程平滑完成。

## 风险与缓解

- 风险：drain 期间若 agent 的当前 step 长期挂起，drain 永远无法完成。
  - 缓解：支持 `--timeout` 参数，超时后强制标记为 Drained 并记录警告。
- 风险：多 agent workflow 中所有 agent 被 drain 导致 task 无法推进。
  - 缓解：当最后一个 active agent 被 drain 时发出警告提示。
- 风险：与现有 `task pause` 语义混淆。
  - 缓解：文档明确区分 task 级暂停与 agent 级 drain 的粒度差异。
