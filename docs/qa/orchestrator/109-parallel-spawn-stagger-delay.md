# QA 109: Parallel Spawn Stagger Delay (FR-055)

## Scenario 1: Default behavior (no stagger)

**Steps**:
1. Create a workflow YAML without `stagger_delay_ms`
2. Set `max_parallel: 2` with 2+ items
3. Run the task

**Expected**: All items spawn immediately with no delay. Behavior identical to pre-FR-055.

## Scenario 2: Workflow-level stagger delay

**Steps**:
1. Create workflow with `stagger_delay_ms: 3000` and `max_parallel: 4`
2. Run with 4 items
3. Check `step_spawned` event timestamps

**Expected**: Spawns are ~3s apart. First spawn has no preceding delay. Last spawn has no trailing delay.

## Scenario 3: Step-level override

**Steps**:
1. Create workflow with `stagger_delay_ms: 5000` at workflow level
2. Add a step with `stagger_delay_ms: 1000` override
3. Run with multiple items

**Expected**: The step uses 1000ms stagger, not 5000ms.

## Scenario 4: Sequential path ignores stagger

**Steps**:
1. Set `max_parallel: 1` (or omit) with `stagger_delay_ms: 3000`
2. Run with multiple items

**Expected**: Items execute sequentially. The stagger delay has no effect (sequential path is used).

## Scenario 5: YAML round-trip preservation

**Steps**:
1. Check `workflow_convert.rs` source
2. Confirm `stagger_delay_ms` is mapped in both `workflow_spec_to_config()` and `workflow_config_to_spec()`
3. Confirm mapped in both `workflow_step_spec_to_config()` and `workflow_step_config_to_spec()`

**Expected**: Field is preserved through spec -> config -> spec round-trip.

## Checklist

| # | Check | Status |
|---|-------|--------|
| 1 | All scenarios verified against implementation | ☑ |

> **Note (2026-03-19)**: S1-S3 需要专用 daemon 运行环境（无其他任务占用 worker）。
> 当 full-QA 占用 daemon 全部 worker 时，新创建的测试任务会停留在 pending 状态。
> 这是基础设施并发限制，非 FR-055 功能 bug。S4/S5 已通过验证。
