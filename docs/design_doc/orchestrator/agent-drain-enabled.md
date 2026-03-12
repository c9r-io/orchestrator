# Agent Drain 与 Enabled 开关

**Related FR**: `FR-017`
**Related QA**: `docs/qa/orchestrator/agent-drain-enabled.md`

## 背景与目标

Orchestrator 的 agent 一旦在 workflow manifest 中声明并随 task 启动，默认持续参与调度直至 task 自然完成。这种全有或全无的控制粒度在以下运维场景下不够用：

- 临时下线某个 agent（其依赖的外部服务维护），不希望取消整个 task。
- 多 agent 协作时逐步收缩活跃 agent 范围，平滑降载。
- 调试时仅保留特定 agent，暂停其余 agent 的新 item 分发。

FR-017 参考 Kubernetes `cordon` / `drain` 范式，为 orchestrator agent 引入两层控制：

- **`enabled` 字段**：manifest 级别的持久化开关，决定 agent 是否参与 task 调度。
- **运行态生命周期状态**：内存中的 `Active → Cordoned → Draining → Drained` 状态机，支持运行时动态操控。

目标：

- 支持在 manifest 中将 agent 声明为 `enabled: false`，task 启动时直接跳过该 agent。
- 通过 CLI 或 gRPC 在运行时动态 cordon / uncordon / drain agent。
- drain 期间不中断正在执行的 step，允许当前工作完成后再退出调度循环。
- 提供 `drain_timeout` 防止 drain 永久挂起。

非目标：

- agent 自动健康检查或自动 drain（属于未来自愈能力）。
- workflow 级别的暂停/取消语义（`task pause` / `task cancel` 不变）。
- agent 热替换或在线重载。

## 状态机

```
Active ──cordon──▶ Cordoned ──drain──▶ Draining ──(in_flight==0)──▶ Drained
  ▲                   │                                                  │
  └──────uncordon─────┘◀──────────────────uncordon───────────────────────┘
```

状态语义：

| 状态 | 接受新 item | 正在执行的 item | 说明 |
|------|------------|----------------|------|
| Active | 是 | 允许继续 | 正常调度 |
| Cordoned | 否 | 允许继续 | 标记不可调度，不等待排空 |
| Draining | 否 | 允许继续 | 等待所有进行中 item 完成后转为 Drained |
| Drained | 否 | 无（已完成） | 已完全退出调度循环 |

`uncordon` 可从 Cordoned 或 Drained 恢复到 Active，重新参与调度。

## `enabled` 字段与运行态状态

### `enabled`（manifest 字段，持久化）

`enabled` 是 agent CRD/manifest 中的 `bool` 字段，默认 `true`。

- `enabled: false`：task 启动时跳过该 agent，不创建运行态记录，不参与选择。
- 运行时可通过 CLI / gRPC 写入持久化层，下次 task 启动生效；或结合 cordon/uncordon 实现即时效果。
- `enabled` 反映 **配置意图**，独立于运行态生命周期状态。

### 运行态生命周期状态（内存，非持久化）

生命周期状态存储在 `AgentRuntimeState` 结构体中，仅在 task 运行期间有效，task 结束后随 task 上下文销毁。不写入数据库，不影响 manifest。

两者关系：

```
manifest.enabled == false  →  task 启动时跳过，不进入状态机
manifest.enabled == true   →  task 启动后进入状态机，初始状态为 Active
运行时 cordon/drain        →  仅修改内存中的生命周期状态，不回写 manifest.enabled
```

## 集成点

### `selection.rs` — 调度选择过滤

选择器在 `select_agent` 路径中增加生命周期状态检查：

- 生命周期状态为 `Active` 且 manifest `enabled == true` 的 agent 才进入选择候选集。
- `Cordoned`、`Draining`、`Drained` 状态的 agent 直接过滤掉，不参与负载/能力评分。

### `phase_runner` — 并发计数 increment/decrement

`phase_runner` 在分发 item 前调用 `increment_in_flight(agent)`，在 item 完成后调用 `decrement_in_flight(agent)`。

- `increment_in_flight` 仅对 `Active` 状态的 agent 生效；`Cordoned`/`Draining` agent 拒绝新 item 分配（返回调度跳过）。
- `decrement_in_flight` 在 item 完成后触发，若 agent 处于 `Draining` 且 `in_flight_count == 0`，自动转为 `Drained` 并发出事件。

### `drain_timeout_sweep` — 超时强制排空

后台 sweep 任务定期检查处于 `Draining` 状态超过 `drain_timeout`（默认 5 分钟，可通过 CLI `--timeout` 覆盖）的 agent：

- 超时后强制将该 agent 标记为 `Drained`，记录警告日志并发出 `AgentDrainTimedOut` 事件。
- 防止因 step 长期挂起导致 drain 永久无法完成。

当最后一个 `Active` agent 被 drain 时，`drain_timeout_sweep` 在日志中发出警告，提示 task 可能无法继续推进。

## CLI 接口

```
orchestrator agent list <task-id>
    显示当前 task 所有 agent 的名称、enabled 值、运行态状态及 in_flight 计数

orchestrator agent cordon <task-id> <agent-name>
    将 agent 标记为 Cordoned，不再接受新 item，不等待排空

orchestrator agent uncordon <task-id> <agent-name>
    将 agent 从 Cordoned 或 Drained 恢复为 Active，重新参与调度

orchestrator agent drain <task-id> <agent-name> [--timeout <seconds>]
    将 agent 标记为 Draining，等待当前 in_flight item 完成后自动转为 Drained
    --timeout 覆盖默认超时（秒）
```

`task status` 输出中增加 agent 状态列，Draining 过程中持续输出剩余 in_flight 计数。

## gRPC API

控制面新增以下 RPC（定义在 `orchestrator.proto`）：

| RPC | 请求 | 响应 | 说明 |
|-----|------|------|------|
| `AgentList` | `AgentListRequest { task_id }` | `AgentListResponse { agents: [AgentStatus] }` | 查询所有 agent 状态 |
| `AgentCordon` | `AgentCordonRequest { task_id, agent_name }` | `AgentCordonResponse` | 执行 cordon |
| `AgentUncordon` | `AgentUncordonRequest { task_id, agent_name }` | `AgentUncordonResponse` | 执行 uncordon |
| `AgentDrain` | `AgentDrainRequest { task_id, agent_name, timeout_secs? }` | `AgentDrainResponse` | 触发 drain，支持可选超时 |

`AgentStatus` 包含：`name`、`enabled`（manifest 字段）、`lifecycle_state`（Active/Cordoned/Draining/Drained）、`in_flight_count`。

## 关键设计决策与权衡

### `enabled` 与生命周期状态解耦

`enabled` 反映配置意图，生命周期状态反映运行态。两者解耦的好处：

- 运维操作（cordon/drain）不污染 manifest，重启 task 后恢复到 manifest 声明的初始状态。
- 支持"本次运行临时下线，下次运行仍参与"的语义，无需修改 manifest。

### Draining 到 Drained 由 decrement 驱动

由 `phase_runner` 的 `decrement_in_flight` 在 item 完成时驱动状态转换，而非后台轮询，确保状态转换与实际执行完成严格同步，避免竞争条件。

### drain_timeout 作为安全兜底

drain_timeout 仅作超时强制排空，不影响正常 drain 流程。step 级别的超时（已由 `phase_runner` 管理）是第一道防线；drain_timeout 是第二道，防止异常挂起的 step 永久阻塞 drain。

## 风险与缓解

风险：所有 agent 被 drain 后 task 无法推进。
缓解：最后一个 Active agent 被 drain 时发出警告；task 调度循环检测到无 Active agent 时记录可操作日志。

风险：drain 与 `task pause` 语义混淆。
缓解：文档明确区分：`task pause` 暂停整个 task（包括所有 agent）；agent drain 仅影响单个 agent 的新 item 分发，已运行的 step 不受影响。

风险：并发 cordon + uncordon 产生状态竞争。
缓解：生命周期状态的修改通过 task 级别的异步锁串行化（遵循 FR-016 async lock model）。

## 可观测性

- `AgentStateChanged` 事件在每次生命周期状态转换时发出，携带 `(agent_name, old_state, new_state, in_flight_count)`。
- `AgentDrainTimedOut` 事件在 drain_timeout_sweep 强制排空时发出。
- `task status` 和 `agent list` 输出实时反映各 agent 状态。
- 无新的数据库 schema 变更（生命周期状态不持久化）。

## 验收标准

- manifest `enabled: false` 的 agent 在 task 启动时不进入调度。
- CLI `drain` 后 agent 不再接收新 item，所有 in_flight item 完成后状态自动变为 Drained。
- `uncordon` 后 agent 恢复 Active，重新参与调度。
- `agent list` 正确显示所有 agent 的实时状态与 in_flight 计数。
- 正在执行的 step 在 drain 过程中不被中断。
- drain_timeout 超时后强制排空并记录警告。
- 工作区验证通过：
  - `cargo test -p agent-orchestrator`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo fmt --all --check`

Executable verification 见 `docs/qa/orchestrator/agent-drain-enabled.md`。
