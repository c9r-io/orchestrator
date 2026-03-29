# 无限进化循环：Evolution ↔ Bootstrap 交替迭代

> **Harness Engineering 执行计划**：本文档是一个 agent 可执行场景，用来展示 orchestrator 这个 control plane 如何组织环境、工作流、约束与反馈闭环，而不是一次性的 prompt 调用。
>
> **Agent 协作**：本文档是一个 Agent 可执行的计划。在 AI 编码 Agent（Claude Code、OpenCode、Codex 等）中打开本项目，Agent 读取本计划后，通过 orchestrator CLI 调度其他 Agent 协作完成任务 — 从资源部署、任务执行到结果验证，全程自主完成。

## 背景

self-evolution 和 self-bootstrap 已分别完成端到端验证：

- **self-evolution**: 多路径竞争探索，自动选出最优方案（2026-03-08 验证通过）
- **self-bootstrap**: 单路径迭代打磨，反复自测直到稳定（2026-02-28 验证通过）

下一步目标：将两者组合为 **evolution → bootstrap → evolution → bootstrap → ...** 的无限迭代循环，让 orchestrator 持续自主进化。

---

## 前置条件：Git Commit 机制

### 问题

当前 agent 的代码变更直接落在工作区，没有自动 commit。多轮迭代的变更会混在一起，无法区分、审查或回滚。

### 方案

使用现有的 command step，在 workflow YAML 中编排 git 操作，**无需核心代码改动**：

```yaml
- id: git_commit
  command: |
    cd {workspace_root}
    git add -A
    git diff --cached --quiet && echo "nothing to commit" || \
    git commit -m "[orchestrator] {task_name} cycle {cycle} — {step_id}"
```

关键设计点：

1. **Commit 时机**: 放在 `self_test` 通过之后，通过 prehook 确保只在测试通过时执行
2. **Commit 消息**: 包含 task_name、cycle 号、workflow 名，便于追溯
3. **空 commit 保护**: `git diff --cached --quiet` 避免无变更时报错
4. **Branch 策略**: 在 feature branch 上操作（见下节），不直接动 main
5. **Rollback 支持**: checkpoint 与 git commit 关联，失败时可 `git revert`

### Feature Branch 自动管理

同样通过 command step 实现，放在 `init_once` 或 cycle 1 的首个步骤：

```yaml
- id: init_once
  command: |
    cd {workspace_root}
    git checkout -b auto/{task_name} 2>/dev/null || git checkout auto/{task_name}
```

任务结束后由人工决定是否 merge 到 main。

---

## 编排方案

### 方案 A: 单 Workflow 内交替（推荐起步）

一个 workflow 包含完整的 evolution + bootstrap 段，用循环模式驱动多轮：

```
Cycle N:
  [Evolution 段]
  evo_plan → generate_items → evo_implement (×2) → evo_benchmark (×2) → select_best
  → evo_apply_winner → self_test → git_commit

  [Bootstrap 段]
  plan → implement → self_test → align_tests → self_test → git_commit

  [收敛判断]
  loop_guard: 检查本轮 diff 是否足够小 / 测试是否全绿 / 无新 clippy warning
```

所有步骤均可用现有 workflow 原语表达：
- evolution 段的 `generate_items` post-action 生成候选项
- item-scoped 步骤并行实现和评测
- `captures` 提取 benchmark 分数到 pipeline 变量
- prehook CEL 表达式控制条件执行

**优点**: 紧凑，状态在 pipeline vars 内自然传递，无需跨 workflow 通信
**缺点**: workflow YAML 较长

### 方案 B: Workflow 级联（通过 Trigger 资源）

两个独立 workflow 通过 Trigger 资源（FR-039，已实现）互相触发：

```yaml
# Trigger: evolution 完成后启动 bootstrap
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: evo-to-bootstrap
  project: self-evolution
spec:
  event:
    source: task_completed
    filter:
      workflow: self-evolution
  action:
    workspace: self
    workflow: self-bootstrap
    goal: "打磨上一轮 evolution 的产出"
  concurrency_policy: Forbid

# Trigger: bootstrap 完成后启动下一轮 evolution
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: bootstrap-to-evo
  project: self-evolution
spec:
  event:
    source: task_completed
    filter:
      workflow: self-bootstrap
  action:
    workspace: self
    workflow: self-evolution
    goal: "探索下一个改进方向"
  concurrency_policy: Forbid
```

**优点**: 各 workflow 独立演进，职责清晰，可单独测试
**缺点**: 跨 workflow 状态传递需通过 Store 资源中转

### 建议

先用方案 A 验证概念，等稳定后再拆分为方案 B。

---

## 收敛条件

无限循环需要合理的停止条件，避免无意义空转。

已提交 **FR-043**（`docs/feature_request/FR-043-convergence-expression.md`），为 `loop_guard` 增加 CEL 表达式驱动的收敛判断：

```yaml
loop:
  mode: infinite
  max_cycles: 10          # 硬上限安全阀
  convergence_expr:
    engine: cel
    when: "delta_lines < 5 && cycle >= 2"
    reason: "code diff converged"
```

在 FR-043 实现之前，可通过现有机制近似实现：
- `max_cycles` 硬停
- `loop_guard` builtin 的 `stop_when_no_unresolved` 标志
- prehook 条件跳过不必要的步骤

收敛维度参考：

1. **Diff 收敛**: 连续 N 轮的 diff 行数低于阈值（如 < 5 行）
2. **Score 收敛**: evolution 阶段两个候选的 benchmark score 差距低于阈值
3. **测试稳定**: 连续 N 轮无新增/修复测试
4. **Budget 上限**: 最大循环次数 or 最大 agent 调用次数
5. **人工中断**: `task pause` 随时可介入

---

## 课题自动发现

通过 agent step + `spawn_tasks` post-action 实现，**无需核心代码改动**：

```yaml
- id: discover_topics
  required_capability: plan
  template: topic_discovery    # prompt 引导 agent 分析代码库找改进点
  behavior:
    post_actions:
      - type: spawn_tasks
        from_var: discover_output
        json_path: "$.topics"
        mapping:
          goal: "$.description"
          workflow: "self-evolution"
          name: "$.slug"
        max_tasks: 3
```

agent 分析代码库输出 JSON 列表 → `spawn_tasks` 自动创建子任务。配合 Trigger 资源可形成持续发现闭环。

---

## 实现优先级

| 优先级 | 任务 | 实现方式 | 依赖 |
|--------|------|----------|------|
| P0 | Git commit 机制 | command step（纯 YAML） | 无 |
| P0 | Feature branch 自动管理 | command step（纯 YAML） | 无 |
| P1 | 单 workflow 交替编排（方案 A） | workflow YAML 编写 | Git commit |
| P1 | 收敛条件表达式（FR-043） | 核心代码改动 | loop_guard CEL 扩展 |
| P2 | Workflow 级联触发（方案 B） | Trigger 资源（纯 YAML） | 已具备（FR-039） |
| P2 | 课题自动发现 | agent step + spawn_tasks（纯 YAML） | topic_discovery template |

> **注**: P0/P1 中仅 FR-043 需要核心代码改动，其余均可通过 workflow YAML 编排实现。

---

## 预期效果

完成后，只需一条命令即可启动持续自主进化：

```bash
orchestrator task create \
  -n "continuous-evolution" \
  -w self -W self-evolve-bootstrap \
  --project self-evolution \
  -g "持续改进 orchestrator 代码质量、性能和功能"
```

引擎将自主循环：探索新方案 → 选出最优 → 打磨实现 → commit → 探索下一个改进点 → ...

直到收敛条件满足或人工介入为止。
