# FR-043: loop_guard 收敛条件表达式

- **优先级**: P1
- **状态**: Proposed
- **来源**: self-bootstrap / self-evolution 执行经验

## 背景

当前 `loop_guard` 步骤是一个 builtin，通过硬编码逻辑判断是否终止 workflow 循环。
判定条件主要依赖 `max_cycles` 计数和 `stop_when_no_unresolved` 标志，
无法表达更细粒度的收敛语义，例如：

- "连续两轮 diff 行数 < 5 则认为收敛"
- "benchmark score 提升幅度 < 1% 则停止"
- "active_ticket_count == 0 且 self_test 通过则收口"

## 需求

在 workflow YAML 的 `loop` 配置中增加 `convergence_expr` 字段，
允许用户通过 CEL 表达式定义自定义收敛条件。

### 期望语法

```yaml
loop:
  mode: infinite          # 不再依赖 max_cycles 硬停
  max_cycles: 20          # 仍保留为安全上限
  convergence_expr:
    engine: cel
    when: "delta_lines < 5 && cycle >= 2"
    reason: "code diff converged"
```

或支持多条件组合：

```yaml
loop:
  convergence_expr:
    - engine: cel
      when: "benchmark_improvement_pct < 1.0 && cycle >= 3"
      reason: "benchmark plateau"
    - engine: cel
      when: "active_ticket_count == 0 && self_test_passed"
      reason: "all tickets resolved"
```

### 所需 pipeline 变量

`convergence_expr` 的 CEL 上下文应包含：

| 变量 | 类型 | 含义 |
|------|------|------|
| `cycle` | int | 当前 cycle 编号 |
| `delta_lines` | int | 本 cycle 的 `git diff --stat` 总行数 |
| `active_ticket_count` | int | 未解决 ticket 数量 |
| `self_test_passed` | bool | 本 cycle self_test 是否通过 |
| `benchmark_score` | float | 最近一次 benchmark 分数（如有） |
| `benchmark_improvement_pct` | float | 与上一 cycle 的 benchmark 提升百分比 |

部分变量需由对应步骤的 `captures` 写入 pipeline，不需要全部由框架提供。

## 实现要点

1. `loop_guard` builtin 在现有逻辑之外增加 `convergence_expr` 求值分支
2. 复用已有的 CEL 引擎（与 prehook/finalize 相同）
3. `convergence_expr` 为可选字段，缺省时行为不变
4. 表达式求值为 `true` 时，`loop_guard` 返回 `terminate` 决策并附带 `reason`

## 非目标

- 不改变 `max_cycles` 语义——它始终是硬上限
- 不引入新的表达式引擎——复用 CEL
- 不改变 `loop_guard` 之外的步骤行为

## 验收标准

1. workflow YAML 中可定义 `convergence_expr`，daemon 能正确解析
2. `loop_guard` 在每个 cycle 末尾求值 `convergence_expr`
3. 表达式为 `true` 时任务正常收口，`task trace` 显示收敛原因
4. 缺省 `convergence_expr` 时行为与当前完全一致
5. 至少一个集成测试验证 CEL 表达式驱动的收敛终止
