# QA 93: Daemon Restart In-Flight Step Completion Race Condition

**关联 FR**: FR-038
**关联 Design Doc**: `docs/design_doc/orchestrator/50-inflight-step-completion-race.md`
**日期**: 2026-03-14

---

## Preconditions (all scenarios)

```bash
# Build latest
cd core && cargo build --release && cd ..
```

---

## Scenario 1: Unit tests — new DB queries pass

**步骤**:
```bash
cargo test --package agent-orchestrator find_inflight
cargo test --package agent-orchestrator completed_runs_for_pending
cargo test --package agent-orchestrator stale_pending
cargo test --package agent-orchestrator find_completed_runs_excludes
```

**预期**:
- `find_inflight_command_runs_empty`: 无 in-flight runs 时返回空
- `find_inflight_command_runs_returns_active`: exit_code=-1 + 空 ended_at 的 run 被返回
- `find_inflight_ignores_completed_runs`: exit_code=0 的 run 不被返回
- `find_completed_runs_for_pending_items_returns_matching`: pending item 的已完成 run 被返回
- `find_completed_runs_excludes_non_pending_items`: qa_passed item 的 run 不被返回
- `count_stale_pending_items_zero_with_no_runs`: 无 runs 时返回 0
- `count_stale_pending_items_counts_stale`: pending + 有已完成 run → 计数 1
- `count_stale_pending_items_ignores_inflight`: pending + 仅 in-flight run → 计数 0

---

## Scenario 2: Full test suite regression check

**步骤**:
```bash
cargo test --package agent-orchestrator
```

**预期**: 所有既有测试继续通过（无回归）

---

## Scenario 3: wait_for_inflight_runs 无 in-flight 时无延迟

**验证方式**: Code review

`wait_for_inflight_runs()` 在 `find_inflight_command_runs_for_task()` 返回空时立即 return，不进入 poll loop。正常（非重启）执行路径不会引入额外延迟。

---

## Scenario 4: compensate_pending_items 正确重建 accumulator

**验证方式**: Code review + unit test 覆盖

- `CompletedRunRecord` 提供 phase, exit_code, confidence, quality_score
- Accumulator 的 `exit_codes`, `step_ran`, `qa_confidence`, `flags` 从 DB 记录正确填充
- `finalize_item_execution()` 调用后 item 状态从 `pending` 转为终态

---

## Scenario 5: effective_unresolved 包含 stale pending items

**验证方式**: Code review

Post-loop 判定使用 `effective_unresolved = unresolved + stale_pending`，确保：
- 正常 items（无 command_runs）不被误计（`count_stale_pending_items` 要求 EXISTS completed run）
- in-flight items 不被误计（要求 NOT EXISTS exit_code=-1 run）
- 仅 recovery 后遗留的真正 stale items 被计入

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
