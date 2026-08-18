# 03 - 工作流配置

本章涵盖工作流设计：步骤定义、执行作用域、循环策略、终结规则和安全配置。

## 工作流结构

工作流在 `spec` 下定义，包含三个主要部分：

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: my_workflow
spec:
  steps: [...]        # 有序步骤列表
  loop: {...}         # 循环策略
  finalize: {...}     # 项终态规则（可选）
  safety: {...}       # 安全限制（可选）
  max_parallel: 4     # item 作用域段的默认并行度（可选）
```

## 步骤定义

每个步骤是工作流流水线中的一个工作单元。

### 完整字段参考

```yaml
- id: plan                          # （必填）唯一步骤标识符
  type: plan                        # （可选）步骤类型 —— 默认与 id 相同
  scope: task                       # （可选）"task" 或 "item" —— 基于 id 自动推断
  enabled: true                     # （必填）是否执行此步骤
  repeatable: true                  # （可选）能否在后续循环中重新运行（默认：true）
  required_capability: plan         # （可选）所需的代理能力（从 id 自动推断）
  template: plan                    # （可选）用于提示词注入的 StepTemplate 名称
  execution_profile: sandbox_write  # （可选）agent step 运行时 profile
  builtin: self_test                # （可选）内置步骤处理器名称
  command: "cargo check"            # （可选）直接 shell 命令（无需代理）
  is_guard: false                   # （可选）标记为循环终止守卫步骤
  tty: false                        # （可选）为交互式代理分配 TTY
  max_parallel: 2                   # （可选）每步骤并行度覆盖
  timeout_secs: 600                 # （可选）每步骤超时秒数
  cost_preference: balance          # （可选）"performance" | "quality" | "balance"
  prehook: {...}                    # （可选）条件执行 —— 参见第 04 章
  behavior: {...}                   # （可选）on_failure、captures、post_actions
```

> `store_inputs`、`store_outputs`、`step_vars` 与 `store_put` 后置动作已移除。
> 携带其中任意一项的 manifest 会被 `[legacy_pipeline_variables_removed]` 拒绝。
> 步骤改为直接读写 store —— 参见[持久化存储](05-advanced-features.md#持久化存储wp01)。

### 步骤执行模式

步骤可以在四种模式之一中执行，自动解析：

| 模式 | 触发条件 | 说明 |
|------|---------|------|
| **Builtin（内置）** | `builtin: self_test` 或已知 id | 由引擎内部处理 |
| **Agent（代理）** | `required_capability: plan` | 分派给匹配的代理 |
| **Command（命令）** | `command: "cargo check"` | 直接 shell 执行，无需代理 |
| **Chain（链式）** | `chain_steps: [...]` | 顺序子步骤容器，并继承当前 `pipeline_vars` |

如果未指定 `builtin` 或 `required_capability`，引擎从步骤 `id` 推断：

- 已知内置 ID（`init_once`、`loop_guard`、`ticket_scan`、`self_test`、`self_restart`、`item_select`）→ 自动内置
- 已知代理 ID（`plan`、`implement`、`qa`、`fix` 等）→ 自动能力匹配

Chain 运行契约：

- 当步骤声明了 `chain_steps` 后，父步骤本身作为容器存在，不再直接运行自己的 agent 或 command。
- 子步骤按顺序执行，并继承当前 `pipeline_vars`。
- 子步骤输出应通过正常的 `captures` / pipeline variables 提升，不依赖隐式特殊变量。
- 子步骤先应用自己的 `behavior.on_failure`；父步骤随后再对整条链的汇总结果应用自己的 `behavior.on_failure`。

### 执行 Profile

`execution_profile` 用于选择该 agent step 的执行边界：

- 未设置时，默认使用隐式 `host`
- 仅 agent step 可设置该字段
- profile 必须引用同 project 下的 `ExecutionProfile` 资源

推荐做法：

- `implement` / `ticket_fix` → `sandbox`
- `qa_testing` → `host`

示例：

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: sandbox_write
spec:
  mode: sandbox
  fs_mode: workspace_rw_scoped
  writable_paths:
    - src
    - docs
  network_mode: deny
```

> **省略时的默认值：** `mode: host`、`fs_mode: inherit`、`network_mode: inherit`。

```yaml
- id: implement
  type: implement
  required_capability: implement
  execution_profile: sandbox_write

- id: qa_testing
  type: qa_testing
  required_capability: qa_testing
  execution_profile: host
```

运行时说明：

- 在当前 macOS sandbox 后端上，`network_mode: deny` 既可能表现为连接失败，也可能表现为 DNS 解析失败；两者都会归类为 `sandbox_network_blocked`。
- 在 Linux `linux_native` 后端上，只要 daemon 以 `root` 运行、系统存在 `ip`/`nft`，并且 profile 使用 `fs_mode: inherit`，`network_mode: allowlist` 就是受支持的真实边界。
- sandbox 相关事件现在会携带稳定的 `reason_code`；自动化优先依赖该字段，再回退到 `stderr_excerpt`。
- `network_target` 只是 best-effort 元数据，某些错误形态下可能为空。
- `network_mode: allowlist` 在 macOS 上仍然不受支持；系统会返回 `reason_code=unsupported_backend_feature`，而不是静默降级到宽松网络访问。
- `network_mode: allowlist` 的条目必须是精确 hostname/IP，可选端口，例如 `api.example.com`、`api.example.com:443`、`10.203.0.1` 或 `[::1]:8443`。

#### 沙箱能力矩阵

| 功能 | macOS (Seatbelt) | Linux (native) | 备注 |
|------|:----------------:|:--------------:|------|
| `mode: sandbox` | 支持 | 支持 | Linux 需要 `ip`/`nft` 和 root |
| `fs_mode: inherit` | 支持 | 支持 | |
| `fs_mode: workspace_readonly` | 支持 | **不支持** | Linux 要求 `fs_mode: inherit` [^1] |
| `fs_mode: workspace_rw_scoped` | 支持 | **不支持** | Linux 要求 `fs_mode: inherit` [^1] |
| `network_mode: deny` | 支持 | 支持 | |
| `network_mode: allowlist` | **不支持** | 支持 | macOS 返回 `reason_code=unsupported_backend_feature` 快速失败 |
| `writable_paths` | 支持 | **不支持** | 需要非 inherit 的 `fs_mode` [^1] |
| 资源限制 (`max_memory_mb` 等) | 支持 | 支持 | |

[^1]: Linux `linux_native` 目前要求 `fs_mode: inherit`，文件系统隔离后端尚未实现。运行 `orchestrator check` 可在预检时发现此限制。

> **提示：** 运行 `orchestrator check` 可在运行前检测平台限制。
> `orchestrator manifest validate` 检查结构正确性；`orchestrator check` 额外检测平台特定的运行时限制。

### 已知步骤 ID

| ID | 默认作用域 | 默认模式 | 说明 |
|----|-----------|---------|------|
| `init_once` | task | 内置 | 一次性初始化 |
| `plan` | task | 代理 | 实施规划 |
| `qa_doc_gen` | task | 代理 | 生成 QA 测试文档 |
| `implement` | task | 代理 | 代码生成 |
| `self_test` | task | 内置 | `cargo check` + `cargo test --lib` |
| `self_restart` | task | 内置 | 重建二进制 + 重启进程 |
| `review` | task | 代理 | 代码审查 |
| `build` | task | 代理 | 构建步骤 |
| `test` | task | 代理 | 测试步骤 |
| `lint` | task | 代理 | 代码检查步骤 |
| `align_tests` | task | 代理 | 重构后对齐测试 |
| `doc_governance` | task | 代理 | 审计 QA 文档质量 |
| `git_ops` | task | 代理 | Git 操作 |
| `qa` | item | 代理 | QA 执行（按文件） |
| `qa_testing` | item | 代理 | QA 场景执行（按文件） |
| `ticket_scan` | item | 内置 | 扫描活动工单 |
| `ticket_fix` | item | 代理 | 修复 QA 工单 |
| `fix` | item | 代理 | 应用修复 |
| `retest` | item | 代理 | 修复后重新测试 |
| `evaluate` | task | 代理 | 评估结果 |
| `item_select` | task | 内置 | WP03：按策略选择项 |
| `loop_guard` | task | 内置 | 循环终止检查 |
| `smoke_chain` | task | 代理 | 链式冒烟测试 |

### 执行作用域

步骤在两种作用域之一中执行：

- **`task` 作用域**：每个循环运行**一次**。用于规划、实现、测试。
- **`item` 作用域**：每个**任务项**（QA 文件）运行一次。用于 QA 测试、工单修复。

步骤按相同作用域的连续段分组为**作用域段**。在 item 作用域段内，项可以并行执行，最多到 `max_parallel`。

```
┌─── Task 段 ────────────────┐  ┌── Item 段 ──────┐  ┌── Task 段 ────────────┐
plan + implement + self_test    qa_testing + ticket_fix  align_tests + doc_governance
```

## 行为配置

`behavior` 块控制步骤成功/失败时的行为以及如何提取结果。

### on_failure / on_success

```yaml
behavior:
  on_failure:
    action: continue       # 默认 —— 继续执行
  # 或
  on_failure:
    action: set_status
    status: "build_failed"
  # 或
  on_failure:
    action: early_return
    status: "aborted"

  on_success:
    action: continue       # 默认
  # 或
  on_success:
    action: set_status
    status: "verified"
```

### post_actions（后置动作）

步骤完成后运行的动作：

```yaml
behavior:
  post_actions:
    - type: create_ticket          # 创建失败工单
    - type: scan_tickets           # 扫描工单目录
    - type: spawn_task             # 派生子任务（WP02）
      goal: "verify-changes"
      workflow: verify_workflow
```

这三个就是全集。`behavior.captures`、`generate_items`、`spawn_tasks` 与 `store_put`
随协调收敛退休，并在 v0.7 窗口被移除；声明其中任何一个的步骤会被具名拒绝。需要读取
前序步骤产出的步骤，请自己从 store 读 —— `orchestrator store get <store> <key>
--project {project_id}` —— 而不是让引擎代为传递。

## 失败去了哪里

步骤失败并不必然导致任务失败，任务完成也不代表一切正常。本节完整陈述这条链：
非零退出码会发生什么、任务终态如何推导、以及哪些事件会进入 attention 收件箱。

### 非零退出码会发生什么

两条执行路径的失败语义不同：

- **Agent（驱动器）步骤** —— 所有由带类型驱动器的 Agent 执行的步骤。非零退出码
  直接使输出校验失败，工作项变为 `unresolved`，任务以 `failed` 结束。此行为不可配置。
- **Builtin 与直连命令步骤**（`agent: builtin`、引擎自有命令）—— 只要输出本身
  有效，*即使退出码非零*，输出校验也会通过。后续行为由 `on_failure` 决定，而默认的
  `continue` 什么都不改变：项状态不动，任务可以 `completed` 结束，唯一的痕迹是一条
  `success: false` 的 `step_finished` 事件 —— attention 收件箱会将其投影为
  `step_failed` 项（见下方路由表）。

对退出码非零的 builtin/直连步骤，`on_failure` 三个动作的后果：

| 动作 | 效果 |
|---|---|
| `continue`（默认） | 状态不变。步骤失败仅记录为事件与收件项，别无其他。 |
| `set_status` | 项状态被 `status:` 覆盖，段继续执行。状态为 `unresolved` 或 `qa_failed` 时任务将在循环结束时失败。 |
| `early_return` | 设置项状态并立即终止当前段。 |

### 任务如何结束

每个调度循环结束时，任务终态由其项推导，从不直接读取退出码：

- `failed` —— 当 `unresolved + stale_pending > 0`（状态为 `unresolved` 或
  `qa_failed` 的项，加上过期的 pending 项）。
- `completed` —— 其余情况。

退出码只通过项状态到达这条推导：驱动器步骤是直接路径，builtin 步骤经由
`on_failure` 或 finalize 规则。

### 哪些事件进入 attention 收件箱

attention 投影器把持久化任务事件转化为收件项。下表由投影器源码
（`crates/orchestrator-scheduler/src/service/attention.rs`）生成，并由
`scripts/qa/test-attention-routing-doc.sh` 校验：表中存在而代码未声明的行 ——
或表格漏掉的路由臂 —— 都会使 CI 失败。没有行的事件类型是刻意不路由的。

<!-- attention-routing:begin -->
| Source event(s) | Condition | Inbox kind | Severity |
|---|---|---|---|
| approval_required, approval_requested | - | `approval_required` | intervention |
| agent_question, decision_required | - | `agent_question` | intervention |
| retry_exhausted | - | `retry_exhausted` | intervention |
| policy_blocked | - | `policy_blocked` | intervention |
| sandbox_denied, sandbox_network_blocked, sandbox_resource_exceeded | - | `sandbox_denied` | intervention |
| budget_threshold, budget_exhausted | - | `budget_threshold` | attention |
| step_timeout, task_stalled | - | `stalled` | intervention |
| task_failed | - | `task_failed` | intervention |
| degenerate_loop, degenerate_cycle, degenerate_cycle_detected | - | `degenerate_loop` | intervention |
| step_failed, output_validation_failed | - | `step_failed` | intervention |
| task_spawn_failed | - | `task_spawn_failed` | intervention |
| step_finished, chain_step_finished, dynamic_step_finished | payload.success == false | `step_failed` | intervention |
| step_finished, chain_step_finished, dynamic_step_finished | confidence < 0.5 | `low_confidence` | attention |
<!-- attention-routing:end -->

### 任务完成会清除什么、保留什么

终态与恢复事件会解决打开的收件项：

<!-- attention-resolution:begin -->
| Trigger event(s) | Scope | Preserved kinds | Resolution reason |
|---|---|---|---|
| task_completed, task_finished | whole task | low_confidence, step_failed, task_spawn_failed | task_completed |
| resume_executed | whole task | (none) | condition_cleared |
| step_finished, chain_step_finished, dynamic_step_finished (success != false) | matching step | n/a | condition_cleared |
<!-- attention-resolution:end -->

任务完成会清扫*条件*类项（审批、停滞、提问）：已经结束的任务不可能仍在等待。
但它**不会**清扫*证据*类项 —— `step_failed`、`low_confidence` 与
`task_spawn_failed` 记录的是已经发生的事实，会一直可见，直到人工解决、该步骤
重试成功、或任务被显式恢复。因此一个带失败 builtin 步骤的绿色任务，其失败仍会
出现在收件箱里。

### 来源侧（无任务）收件项

有些收件项完全不经任务事件投影：webhook 与 source 侧的失败直接物化，不携带
任务 ID，也永远不会被任务完成清扫。当前的 kind 集合（同样由代码生成并做漂移
检查）：

<!-- attention-external-kinds:begin -->
- `inbox_projection_gap` —— 收件箱关闭期间未投影的事件；每项目一条合并项，重开时写入。
- `source_auth_failed` —— 某个已配置 trigger 的 webhook 投递持续签名/密钥校验失败；按 trigger 合并，首次成功投递自动解决。
- `source_automation_binding_ambiguous` —— source 反应无法唯一选中一个 binding。
- `source_automation_configuration_invalid` —— 匹配到的 source 反应无法完成预约。
- `source_automation_needs_attention` —— source automation 路由被阻塞，需要操作员。
- `source_connection_provisioning_attention` —— 专属 Slack 应用的 provisioning 需要操作员。
- `source_connection_reauthorization_required` —— 托管 source 连接需要重新授权。
- `source_connection_revoked` —— 提供方吊销了托管连接。
- `source_route_missing` —— webhook 投递命名了项目中不存在的 trigger；按项目合并，未知名称只以摘要形式出现。
- `source_routing_ambiguous` —— 一条 source 事件匹配到多个路由目标。
<!-- attention-external-kinds:end -->

### 收件箱关闭时

在项目的 RuntimePolicy 中设置 `attention_inbox_enabled: false` 会停止新的
物化，但不会停住投影游标：关闭窗口内到达的事件按项目计数，重开收件箱时会浮出
一条 `inbox_projection_gap` 项，说明有多少事件（及其 id 区间）从未被投影。
静默丢失不再是选项；若计数非零，请回查该窗口的任务历史。

## 循环策略

循环策略控制工作流运行多少个循环。

```yaml
loop:
  mode: once              # 运行一个循环后停止（默认）
```

```yaml
loop:
  mode: fixed             # 精确运行 N 个循环
  max_cycles: 2
  enabled: true
  stop_when_no_unresolved: false   # false = 始终运行所有循环（默认值：true）
```

```yaml
loop:
  mode: infinite          # 运行直到守卫停止或达到 max_cycles
  max_cycles: 10          # 安全上限
```

### 循环模式

| 模式 | 行为 |
|------|------|
| `once` | 单次循环后停止 |
| `fixed` | 精确 `max_cycles` 个循环 |
| `infinite` | 重复直到 `loop_guard` 步骤决定停止，受 `max_cycles` 限制 |

`loop_guard` 内置步骤应作为 infinite/fixed 工作流的最后一个步骤。它评估是否还有未解决的项，并决定是否继续。

## 终结规则

终结规则确定每个任务项在循环结束时的终态。它们使用 CEL 表达式（与预钩子相同的引擎）。

```yaml
finalize:
  rules:
    - id: qa_passed_no_tickets
      engine: cel
      when: "active_ticket_count == 0 && qa_ran"
      status: qa_passed
      reason: "QA 通过，无活动工单"

    - id: fix_verified
      engine: cel
      when: "fix_ran && retest_success"
      status: fix_verified
      reason: "修复已应用且重测通过"

    - id: fallback_pending
      engine: cel
      when: "true"
      status: pending
      reason: "默认回退"
```

规则按顺序评估；第一个匹配的规则生效。终结上下文变量详见[第 04 章](04-cel-prehooks.md)。

## 安全配置

`safety` 块防止失控或破坏性工作流。

```yaml
safety:
  max_consecutive_failures: 3     # N 次失败后自动回滚（默认：3）
  auto_rollback: true             # 启用自动回滚
  checkpoint_strategy: git_tag    # none | git_tag | git_stash
  binary_snapshot: true           # 在循环开始时快照二进制（自引导）
  step_timeout_secs: 1800         # 全局步骤超时（30 分钟）
  max_spawned_tasks: 10           # WP02：每个父任务最大子任务数
  max_spawn_depth: 3              # WP02：最大父→子→孙深度
  invariants:                     # WP04：不可变安全断言
    - id: no_delete_main
      check:
        command: "git branch --list main | wc -l"
        expect: "1"
      on_violation: abort
```

## 组合示例

一个完整的自引导风格工作流：

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: self-bootstrap
spec:
  max_parallel: 4

  steps:
    # ── Task 段：plan → implement → self_test ──
    - id: plan
      scope: task
      template: plan
      enabled: true
      repeatable: false

    - id: implement
      scope: task
      template: implement
      enabled: true

    - id: self_test
      scope: task
      builtin: self_test
      enabled: true

    # ── Item 段：qa_testing → ticket_fix ──
    - id: qa_testing
      scope: item
      template: qa_testing
      enabled: true
      prehook:
        engine: cel
        when: "is_last_cycle"
        reason: "QA 延迟到最后一个循环"

    - id: ticket_fix
      scope: item
      template: ticket_fix
      enabled: true
      max_parallel: 2
      prehook:
        engine: cel
        when: "is_last_cycle && active_ticket_count > 0"

    # ── 循环守卫 ──
    - id: loop_guard
      builtin: loop_guard
      enabled: true
      is_guard: true

  loop:
    mode: fixed
    max_cycles: 2

  safety:
    max_consecutive_failures: 3
    auto_rollback: true
    checkpoint_strategy: git_tag
```

## 下一步

- [04 - CEL 预钩子](04-cel-prehooks.md) —— 动态步骤门控和所有可用变量
- [05 - 高级特性](05-advanced-features.md) —— CRD、存储、任务派生
