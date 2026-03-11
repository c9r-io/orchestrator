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
  store_inputs: [...]               # （可选）执行前从工作流存储读取
  store_outputs: [...]              # （可选）执行后写入工作流存储
```

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

### captures（捕获）

从步骤结果中提取值到管道变量：

```yaml
behavior:
  captures:
    - var: build_output
      source: stdout       # stdout | stderr | exit_code | failed_flag | success_flag
```

### post_actions（后置动作）

步骤完成后运行的动作：

```yaml
behavior:
  post_actions:
    - type: create_ticket          # 创建失败工单
    - type: scan_tickets           # 扫描工单目录
    - type: store_put              # 写入工作流存储（WP01）
      store: context
      key: finding
      from_var: plan_output
    - type: spawn_task             # 派生子任务（WP02）
      goal: "verify-changes"
      workflow: verify_workflow
    - type: generate_items         # 生成动态项（WP03）
      from_var: candidates
```

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
  stop_when_no_unresolved: false   # false = 始终运行所有循环
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
