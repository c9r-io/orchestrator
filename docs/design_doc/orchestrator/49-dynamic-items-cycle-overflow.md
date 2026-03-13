# Design Doc 49: Dynamic Items Cycle Overflow — Proactive max_cycles Enforcement

**关联 FR**: FR-037
**状态**: Implemented
**日期**: 2026-03-13

---

## 1. 问题

在 `mode: fixed, max_cycles: 2` 的工作流中，`generate_items` post-action 创建的 dynamic items 导致 cycle 计数器突破 `max_cycles` 限制，一路攀升到 cycle 10 才被 FR-035 L2 退化循环检测兜底拦停。

### 根因

`run_task_loop_core` 中的 cycle 计数器 (`current_cycle += 1`) 在循环顶部**无条件递增**，而 `max_cycles` 的检查仅在 cycle 执行完成后通过 `evaluate_loop_continuation()` → `evaluate_loop_guard_rules()` 进行。Dynamic items 的并行 segment 完成在这两者之间制造了窗口，使新 cycle 在 continuation 检查前就被触发。

## 2. 设计决策

### 采用方案：Proactive max_cycles gate

在 `current_cycle += 1` **之前**插入前置检查，使用 `proactive_max_cycles()` 函数计算当前 loop policy 的 cycle 上限：

```rust
// core/src/scheduler/loop_engine/mod.rs
let proactive_max = proactive_max_cycles(&task_ctx.execution_plan.loop_policy);
if task_ctx.current_cycle >= proactive_max {
    // emit max_cycles_enforced event and break
    break;
}
task_ctx.current_cycle += 1;
```

`proactive_max_cycles()` 对各模式的返回值：
- `Fixed`: `max_cycles.unwrap_or(1)` — 与 `evaluate_loop_guard_rules` 一致
- `Infinite`: `max_cycles.unwrap_or(u32::MAX)` — 有上限时生效，无上限时不干预
- `Once`: `u32::MAX` — 由 `evaluate_loop_guard_rules` 处理，proactive 不干预

### 未采用方案：created_at_cycle 字段

FR-037 Section 4.2 提议为 dynamic items 添加 `created_at_cycle` 列。此方案被推迟，因为：
- Proactive gate 已阻止新 cycle 启动，不存在"跨 cycle 处理"的场景
- 需要 schema migration，复杂度不匹配收益
- 如有需要可作为独立 FR 追加

## 3. 改动范围

| 文件 | 改动 |
|------|------|
| `core/src/scheduler/loop_engine/mod.rs` | 新增 `proactive_max_cycles()` 函数；在 cycle 递增前调用检查并 emit `max_cycles_enforced` 事件 |
| `core/src/scheduler/loop_engine/tests.rs` | 5 个单元测试覆盖 Fixed/Infinite/Once 模式 |

## 4. 向后兼容性

- `LoopMode::Once` 和 `LoopMode::Infinite`（无 max_cycles）行为不变
- FR-035 L1/L2 机制不受影响（proactive gate 在 L2 检测之前触发）
- 现有 `evaluate_loop_guard_rules` 作为 post-cycle 兜底继续生效

## 5. 事件

新增事件类型 `max_cycles_enforced`，payload:
```json
{
  "current_cycle": 2,
  "max_cycles": 2
}
```
