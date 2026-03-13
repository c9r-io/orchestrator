# FR-035: 退化循环检测与熔断机制

## 状态

Open

## 优先级

P1 — 导致 task 无限快速循环，浪费 API 资源且无法自愈

## 背景

### 问题发现

2026-03-13 执行 `follow-logs-callback-execution.md` 测试计划时，task 在 Cycle 3+ 进入退化循环：

```
09:39:16 cycle_started → implement started
09:39:30 cycle_started → implement started  (14s later)
09:39:44 cycle_started → implement started  (14s later)
09:39:57 cycle_started → implement started  (13s later)
09:40:09 cycle_started → implement started  (12s later)
09:40:22 cycle_started → implement started  (12s later)
09:40:33 cycle_started → implement started  (11s later)
09:40:46 cycle_started → implement started  (13s later)
09:40:55 cycle_started → implement started  (9s later)
```

所有 implement run 均以 exit=-1 失败。在约 10 分钟内产生了 13+ 个失败的 implement run，每个 run 消耗 API tokens。

### 根因分析

#### 1. 无连续失败计数器

当同一 item 在同一 phase 连续失败 N 次时，scheduler 没有判断机制。每次失败后，ticket_fix 触发新 cycle，新 cycle 又执行 implement，implement 又以相同原因失败，如此往复。

#### 2. 无 cycle 间隔检测

正常的 self-bootstrap cycle 持续 5-15 分钟。当 cycle 间隔缩短到 <30s 时，说明步骤几乎立即失败，属于退化模式。但 scheduler 没有检测这一模式。

#### 3. implement agent 的失败原因固定且不可自愈

agent 失败的根因是 `plan_output.txt`（80K tokens）超过了 Claude 的单次 Read 限制（25K tokens）。这是一个结构性限制，无论重试多少次都不会成功。agent 无法缩小文件或用其他方式获取信息。

#### 4. loop_guard 仅在完整 pipeline 末端生效

`loop_guard` 步骤在 pipeline 的最后阶段，但当 implement 反复失败时，pipeline 永远无法推进到 loop_guard。

## 需求

### 需求 1：连续失败熔断

当同一 item 在同一 phase 连续失败 N 次时，自动熔断：

```
规则：
  IF item X 在 phase Y 连续失败 >= max_consecutive_failures (默认 3)
  THEN 标记该 item 为 `blocked`，跳过后续步骤
  AND 生成 anomaly event: `item_blocked_consecutive_failures`
  AND 继续处理其他 items
```

实现位置：`core/src/scheduler/engine.rs` 在 step 完成后检查连续失败计数。

### 需求 2：快速循环检测

检测 cycle 间隔异常缩短的退化模式：

```
规则：
  IF 最近 3 个 cycle 的间隔均 < min_cycle_interval (默认 60s)
  THEN 触发 task 级别的 pause
  AND 生成 anomaly event: `degenerate_cycle_detected`
  AND 记录最近 N 个 run 的 exit_code 和 phase 信息
```

实现位置：`core/src/scheduler/engine.rs` 在 `cycle_started` 事件发出前检查历史间隔。

### 需求 3：指数退避重试

对于 implement/ticket_fix 等可重试步骤，引入指数退避：

```
第 1 次重试：立即
第 2 次重试：等待 30s
第 3 次重试：等待 120s
第 4 次重试：熔断，标记 blocked
```

### 需求 4：失败原因聚合

在 `task trace` 和 `task info` 输出中，对重复失败进行聚合显示：

```
DEGENERATE LOOP DETECTED:
  item f1cd078f (02-cli-task-lifecycle.md) implement failed 13 times
  Last exit: -1
  Pattern: rapid cycle (avg interval: 12s)
  Recommendation: investigate agent logs or manually fix
```

## 验收标准

1. 同一 item 在同一 phase 连续失败 3 次后，自动标记为 `blocked` 并跳过
2. 连续 3 个 cycle 间隔 < 60s 时，task 自动 pause 并生成 `degenerate_cycle_detected` anomaly
3. `task trace` 输出中包含退化循环检测结果
4. 熔断后的 item 可通过 `task resume --reset-blocked` 重新尝试
5. 指数退避不影响正常的 self-bootstrap 流程（正常 cycle 间隔 > 5min）

## 关联

- 发现于：`follow-logs-callback-execution.md` 测试计划，Cycle 3+ implement 退化循环
- 相关现象：13+ 个 implement run 全部 exit=-1，每个 run 消耗 API tokens
- 相关机制：现有 `loop_guard` 步骤只在 pipeline 末端生效，无法拦截中途退化
