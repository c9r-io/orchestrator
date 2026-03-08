# 无限进化循环：Evolution ↔ Bootstrap 交替迭代

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

在 pipeline 中引入 `git_commit` 步骤类型（或 post-action），在关键节点自动 commit：

```yaml
- id: git_commit
  kind: shell
  command: |
    cd {workspace_root}
    git add -A
    git diff --cached --quiet && echo "nothing to commit" || \
    git commit -m "[orchestrator] {task_name} cycle {cycle} — {step_id}"
  run_after:
    - self_test   # 只在测试通过后才 commit
```

关键设计点：

1. **Commit 时机**: `self_test` 通过后、下一轮开始前
2. **Commit 消息**: 包含 task_name、cycle 号、workflow 名，便于追溯
3. **空 commit 保护**: `git diff --cached --quiet` 避免无变更时报错
4. **Branch 策略**: 在 feature branch 上操作（`evo/{task_id}`），不直接动 main
5. **Rollback 支持**: checkpoint 与 git commit 关联，失败时可 `git revert`

### 实现范围

- `core/src/scheduler/item_executor/dispatch.rs`: 新增 `shell` step 类型的 `git_commit` 语义
- 或更通用：在现有 `shell` step 中直接写 git 命令（无需引擎改动，纯 YAML 编排）
- 可选增强：引擎内置 `git_commit` post-action，自动关联 checkpoint tag

---

## 编排方案

### 方案 A: 单 Workflow 内交替（推荐起步）

一个 workflow 包含完整的 evolution + bootstrap 段，用 `LoopMode::Converge` 驱动多轮：

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

**优点**: 紧凑，状态在 pipeline vars 内自然传递，无需跨 workflow 通信
**缺点**: workflow YAML 较长

### 方案 B: Workflow 级联

两个独立 workflow 通过 finalize rule 互相触发：

```
self-evolution:
  finalize:
    - when: "task_status == 'completed'"
      action: create_task
      params:
        workflow: self-bootstrap
        goal: "{prev_evolution_goal}"

self-bootstrap:
  finalize:
    - when: "task_status == 'completed' && improvement_delta > threshold"
      action: create_task
      params:
        workflow: self-evolution
        goal: "继续探索 {next_topic}"
```

**优点**: 各 workflow 独立演进，职责清晰
**缺点**: 需要实现 `create_task` finalize action（当前 finalize 只支持状态变更）

### 建议

先用方案 A 验证概念，等稳定后再拆分为方案 B。

---

## 收敛条件

无限循环需要合理的停止条件，避免无意义空转：

1. **Diff 收敛**: 连续 N 轮的 diff 行数低于阈值（如 < 5 行）
2. **Score 收敛**: evolution 阶段两个候选的 benchmark score 差距低于阈值
3. **测试稳定**: 连续 N 轮无新增/修复测试
4. **Budget 上限**: 最大循环次数 or 最大 agent 调用次数
5. **人工中断**: `task pause` 随时可介入

可通过 `LoopMode::Converge` 的 `convergence_expr` 表达：

```yaml
loop:
  mode: converge
  max_cycles: 10
  convergence_expr: "diff_lines < 5 && score_delta < 3"
```

---

## 实现优先级

| 优先级 | 任务 | 依赖 |
|--------|------|------|
| P0 | Git commit 机制（shell step 方式，纯 YAML） | 无 |
| P0 | Branch 策略（自动创建 feature branch） | Git commit |
| P1 | 单 workflow 交替编排（方案 A） | Git commit |
| P1 | 收敛条件表达式 | LoopMode::Converge |
| P2 | Workflow 级联触发（方案 B） | Finalize action 扩展 |
| P2 | 课题自动发现（从 TODO/issue 中提取下一轮课题） | Evolution pipeline |

---

## 预期效果

完成后，只需一条命令即可启动持续自主进化：

```bash
./scripts/run-cli.sh task create \
  -n "continuous-evolution" \
  -w self -W self-evolve-bootstrap \
  -g "持续改进 orchestrator 代码质量、性能和功能"
```

引擎将自主循环：探索新方案 → 选出最优 → 打磨实现 → commit → 探索下一个改进点 → ...

直到收敛条件满足或人工介入为止。
