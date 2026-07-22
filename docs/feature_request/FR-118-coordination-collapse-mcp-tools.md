# FR-118: 协调塌缩 — 用 orchestrator-owned MCP 工具替换过渡态 CEL 层

## 优先级: P1

## 状态: Proposed

## 依赖

- `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`：本 FR 执行其 Migration Plan 的 phase 2–4（tool 落地 + pilot 迁移 + 度量）
- FR-116（Agent Driver 抽象，已闭环 → DD-127）：driver seam、`DriverEvent`/`DriverInput`、typed tool 契约、`allowedTools`/permission 治理
- FR-049：Prehook CEL 接入 Pipeline Variables（本 FR 要迁移离开的机制）
- FR-092：Pipeline 变量 Spill 路径可配置（本 FR 度量的 4KB spill 通道）

## 计划闭环产物

- `docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md`
- `docs/qa/orchestrator/167-coordination-collapse-mcp-tools.md`
- `docs/guide/coordination-tools.md`（EN + ZH）
- `fixtures/manifests/bundles/coordination-collapse-pilot.yaml`
- `scripts/qa/test-coordination-collapse.sh`
- `docs/design_doc/orchestrator/101-streaming-agent-runner-architecture-pivot.md`（更新实现状态：pivot 完成）
- `docs/architecture.md`、`CHANGELOG.md`（更新）

## Background And Product Decision

DD-101 提出把协调逻辑从声明式层（CEL prehook、`captures`/`json_path`、pipeline vars + 4KB spill、`post_actions`、`builtin:` 魔法字符串）塌缩进 agent 在 step 中调用的 orchestrator-owned typed 工具。FR-116 落地了 driver **seam**（管道），但塌缩的**目的**尚未执行：

- `crates/orchestrator-runner/src/bin/orch_mcp_tools.rs` 的 `run_tests` / `mark_done` 仍是 **canned stub**（头注释明写 "Replace the canned bodies with real orchestrator logic as the pivot progresses"）。
- 无任何 workflow 迁移：5 个 workflow 仍用 CEL prehook/captures，唯一 streaming 产物是 `streaming-mark-done-convergence` 的 demo。

因此当前 CEL 层是**过渡态**——`self-bootstrap.yaml` 443 行中约 70% 是为黑箱 agent 契约支付的协调"赎金"。本 FR 把 stub 换成真实 orchestrator 逻辑、迁移一个 pilot workflow 脱离 CEL/captures，并度量塌缩效果。

### 排序决策（与 typed pipeline vars 的关系）

本 FR 是「协调状态住在哪里」这一战略问题的第一步，**刻意排在 typed pipeline vars（LangGraph-style state+reducer）之前**。理由：

1. DD-101 的 coordination-collapse 映射表明确列出 `pipeline vars + 4KB spill → tool I/O（移除）`。给 pipeline var 层加类型系统，与塌缩方向相反。
2. 流式模型本身就是 typed-state 的另一个答案——state 住在 **agent session + typed tool 契约**里，而非中央 store。LangGraph 需要中央 typed store 是因为其 node 无状态；本项目的流式 agent 有状态，中央 store 大部分冗余。
3. 真正的残余需求是**跨进程/跨 agent 的窄通道**，其范围只有在 pilot 迁移后才能看清。

因此本 FR 把「度量残余 pipeline-var 流量」列为一等交付物（见 Goals），作为未来 typed-channel FR 的定范围依据。**本 FR 不引入 typed pipeline vars。**

## Goals

- 将 `orch_mcp_tools` 的 stub 替换为真实 orchestrator 逻辑：工具结果由 daemon 计算（in-process HTTP MCP，DD-101 首选）或经回调获得，不再 canned。
- 落地一组 orchestrator-owned typed 协调工具，覆盖现有声明式机制的等价能力：至少 `mark_item`（替代 finalize 规则/终态）、`create_ticket` 与 `scan_tickets`（替代 `post_actions`）、`run_tests`（替代 QA exit-code capture）、`generate_items`（替代动态 item 生成）。
- 迁移**一个** pilot workflow（候选：QA fix-loop 或 `self-bootstrap`）脱离 CEL prehook / `captures` / `json_path` / pipeline-var / `post_actions`，改由 agent 调用工具。
- 工具治理复用 FR-116：`allowedTools` 白名单 + permission mode + per-run MCP 隔离；工具调用全量进 `events` 表。
- **度量并记录**：pilot 迁移前后的手写 YAML/CEL 行数、行为等价性，以及**迁移后仍存活的跨步 pipeline-var 流量清单**（typed-channel FR 的输入）。

## Non-goals

- 移除 shell/CEL 路径或迁移全部 workflow；本 FR additive，只做一个 pilot，shell/CEL 保持默认与可用。
- 引入 typed pipeline vars / reducer（排序在本 FR 之后，按度量结果定范围）。
- 删除声明式治理层——Workspace/Agent 策略/Safety/沙箱/Trigger 保持声明式（DD-101 的 governance 边界不变）。
- 改变数据库持久化内核；工具 I/O 事件摄入为加法。
- 让工具绕过 FR-116 的沙箱不变量（工具执行仍在 daemon 边界内，agent 副作用仍经 spawn 路径）。

## Design（承接 DD-101，落地细节留 DD-130）

- **工具宿主**：orchestrator-hosted HTTP MCP（in-process，工具直接共享 daemon 状态），本地绑定 + per-run token（DD-101 风险段的缓解）。stdio shim 作为回退。
- **工具即协调原语**，逐条对照被替换的声明式机制：
  - CEL finalize / 终态 → `mark_item`
  - `captures` + `json_path`（QA exit code）→ `run_tests` 返回结构化结果
  - `post_actions`（create_ticket / scan_tickets / generate_items）→ 同名工具
  - `builtin:` 魔法字符串 → 普通工具/函数
- **保持声明式的**：capability/cost 选择、safety/sandbox profile、permission、trigger——即 agent 不应替自己决定的部分（FR-116 的 `allowedTools` 是执行点）。
- **残余通道度量点**：在 pilot 运行时记录每个仍跨 step 传递的 pipeline var（key、来源 step、消费 step、是否 spill），输出为 typed-channel FR 的范围依据。

## Risks And Mitigations

- 风险：把决策委托给 agent，丢失 CEL 的确定性/可审计控制流。
  - 缓解：硬护栏留在 code/policy（safety、budget cap、`allowedTools`）；agent 在栅栏内决策，不决定栅栏本身（DD-101 原则）。
- 风险：pilot 迁移后行为与 shell 版本不等价。
  - 缓解：并排跑 shell 版与工具版，终态/事件逐项比对为准入条件；shell 路径保持默认直到 parity 证明。
- 风险：HTTP MCP 引入本地 IPC 面，任意本地进程可打端点。
  - 缓解：本地绑定 + per-run token 经 `--mcp-config` 传入；沿用 FR-116 的 per-run 隔离，不复用共享临时路径。
- 风险：残余流量度量被草草带过，导致后续 typed-channel FR 失去依据。
  - 缓解：度量清单列为显式验收项，格式化落入 DD-130，供下一 FR 直接引用。
- 风险：工具面膨胀成第二个 CEL（把复杂度从 YAML 搬到工具数量）。
  - 缓解：本 FR 限定 4–5 个工具覆盖现有机制等价能力，不新增协调概念；工具清单变更需 DD-130 记录理由。

## Acceptance Criteria

- `orch_mcp_tools` 不再返回 canned 结果；工具结果由 orchestrator 计算，且工具执行在 daemon 边界内。
- 至少 `mark_item`、`create_ticket`/`scan_tickets`、`run_tests`、`generate_items` 落地并经 FR-116 治理（`allowedTools` + permission + per-run 隔离）。
- pilot workflow 在工具模型下与 shell/CEL 版本行为等价（终态与事件逐项比对通过），手写 YAML/CEL 行数下降有量化记录（目标对齐 DD-101 的 ~80%）。
- 工具 `tool_use`/`tool_result` 全量进入 `events` 表。
- 迁移后仍存活的跨步 pipeline-var 流量以结构化清单记录于 DD-130，明确标注其为未来 typed-channel FR 的定范围输入。
- DD-101 实现状态更新为「pivot 完成」；`cargo build --workspace` / `cargo test --workspace` 通过，既有 shell/CEL workflow 回归全绿。

## Follow-up（不在本 FR 范围）

- **Typed 跨步通道**：基于本 FR 的残余流量度量，评估是否需要给存活的跨进程数据流引入类型/校验（对标 LangGraph typed state，但范围收窄至残余通道，非中央 store）。此为独立 FR，待本 FR 度量产出后立项。
