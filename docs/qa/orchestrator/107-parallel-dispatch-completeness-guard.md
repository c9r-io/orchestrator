# QA Doc 107: Parallel Dispatch Completeness Guard (FR-053)

**关联**: Design Doc 65, FR-037 (Design Doc 49)

---

## 验证场景

### Scenario 1: 所有 items 正常分发 — 无错误

**前提**: workflow 配置 max_parallel=2, 4 个 items

**步骤**:
1. 运行 item-scoped workflow，所有 items 均可正常执行
2. 检查 events 表中是否存在 `parallel_dispatch_incomplete` 事件

**预期**: 无 `parallel_dispatch_incomplete` 事件，task 正常完成，所有 items 获得 command_runs

> **Note**: S1 验证需要 workspace 配置了 `qa_targets` 指向包含多个文件的目录。
> 不要使用 `--target-file` 指向 bundle manifest（bundle manifest 本身会被视为单个 item，
> 而不是扫描其中引用的 qa_targets 目录）。

### Scenario 2: dispatched_count 计数器准确性

**步骤**:
1. 检查 `segment.rs` 源码
2. 确认 `dispatched_count` 初始化为 0（line 323）
3. 确认 `dispatched_count += 1` 位于 `join_set.spawn()` 之后（line 365 在 line 346 之后）
4. 确认 completeness check 使用 `items.len()` 作为 expected（line 398）

**预期**: 计数器在每次成功 spawn 后递增，expected 等于 items 总数

### Scenario 3: parallel_dispatch_incomplete 事件 payload 结构

**步骤**:
1. 检查 `segment.rs` lines 405-414 的 `insert_event` 调用
2. 验证 payload JSON 结构

**预期**: payload 包含整数字段 `dispatched` 和 `expected`

### Scenario 4: 错误传播阻止 max_cycles_enforced

**前提**: dispatched_count < items.len()

**步骤**:
1. 追踪 bail!()（line 416）的传播路径：
   - `execute_item_segment` 返回 `Err` → `execute_cycle_segments` via `?` → `run_task_loop_core` breaks cycle loop
2. 确认 `max_cycles_enforced` 检查位于 cycle loop 顶部（`mod.rs:156-183`），loop break 后不再执行

**预期**: completeness check 失败时，`max_cycles_enforced` 永远不会被触发

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | 2026-03-16 code review + unit tests 411 passed |
| 2 | S1: Normal dispatch - no `parallel_dispatch_incomplete` | ☑ | 2026-03-27: Task c6cdb921 completed 4/4 items, no incomplete event |
| 3 | S2: dispatched_count counter accuracy | ☑ | 2026-03-27: Line 343=0, line 396 increment after spawn, line 434 uses items.len() |
| 4 | S3: Event payload structure | ☑ | 2026-03-27: Lines 449-451 show `dispatched` and `expected` integer fields |
| 5 | S4: Error propagation blocks max_cycles_enforced | ☑ | 2026-03-27: bail! at line 455 propagates via ?, breaks cycle loop before line 162 |

## Re-verification 2026-03-28

| # | Check | Status | Notes |
|---|-------|--------|-------|
| S1 | Normal dispatch - no `parallel_dispatch_incomplete` | ⚠ | 2026-03-28: Task c245e1db stuck in pending (daemon issue) - prior verification c6cdb921 valid |
| S2 | dispatched_count counter accuracy | ☑ | 2026-03-28: Verified - line 343=0, line 396 increment after spawn, line 434 uses items.len() |
| S3 | Event payload structure | ☑ | 2026-03-28: Verified - lines 449-451 show `dispatched` and `expected` integer fields |
| S4 | Error propagation blocks max_cycles_enforced | ☑ | 2026-03-28: Verified - bail! at line 455 propagates via ?, breaks cycle loop before max_cycles_enforced |

**Note**: S1 runtime verification blocked by daemon task scheduling issue (ticket: fixtures/ticket/qa107-s1-daemon-stuck-pending.md). Code review confirms FR-053 implementation is correct.
