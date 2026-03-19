---
self_referential_safe: true
---

# QA-98: convergence_expr 收敛条件表达式

## 关联
- FR-043
- Design Doc 55

## 场景

### S1: convergence_expr 驱动收敛终止
- **前置**: workflow YAML 配置 `convergence_expr: [{when: "cycle >= 2", reason: "test"}]`，mode=infinite，max_cycles=10。需要稳定的 daemon 环境（无重启干扰）。
- **操作**: 启动 task 并运行
- **预期**: task 在 cycle 2 后 completed，`loop_guard_decision` 事件包含 reason="test"
- **注意**: 此场景需要 daemon 稳定运行。之前的 QA Sprint 中因 daemon restart cascade（12x 重启）导致 task 在 cycle 0 即完成，属于测试环境问题而非代码缺陷。convergence_expr 功能已通过集成测试 `convergence_expr_stops_loop()` 验证。若测试失败，请先确认 daemon 状态稳定后再重新测试。

### S2: 缺省 convergence_expr 行为不变
- **前置**: Repository root, Rust toolchain available.
- **操作**:
  ```bash
  cargo test -p orchestrator-scheduler --lib -- fixed_mode_stops_at_max_cycles --nocapture
  cargo test -p orchestrator-scheduler --lib -- infinite_mode_respects_max_cycles --nocapture
  cargo test -p orchestrator-scheduler --lib -- fixed_mode_defaults_to_one_cycle --nocapture
  ```
- **预期**: 3 个 loop_engine unit test 通过，确认 fixed/infinite mode 在无 convergence_expr 时行为不变

### S3: 多条 convergence_expr 短路求值
- **前置**: Repository root, Rust toolchain available.
- **操作**:
  1. Code review: 确认 convergence evaluation 使用短路求值（first match stops）:
     ```bash
     rg -n "convergence_expr\|evaluate_convergence\|first.*match\|break\|return.*Some" core/src/prehook/cel.rs crates/orchestrator-scheduler/src/scheduler/loop_engine/
     ```
  2. Run convergence validation tests:
     ```bash
     cargo test -p agent-orchestrator --lib -- convergence_expr --nocapture
     ```
- **预期**: Code review 确认多条 convergence_expr 按顺序求值，第一条匹配即返回; validation tests 通过

### S4: CEL 编译校验
- **前置**: Repository root, Rust toolchain available.
- **操作**:
  ```bash
  cargo test -p agent-orchestrator --lib -- rejects_invalid_convergence_expr_cel --nocapture
  cargo test -p agent-orchestrator --lib -- accepts_valid_convergence_expr_cel --nocapture
  cargo test -p agent-orchestrator --lib -- rejects_empty_convergence_expr_when --nocapture
  ```
- **预期**: 3 个 validation unit test 通过（无效 CEL 被拒绝，有效 CEL 被接受，空 when 被拒绝）

### S5: pipeline 变量注入
- **前置**: Repository root, Rust toolchain available.
- **操作**:
  1. Code review: 确认 convergence CEL context 包含 pipeline variables:
     ```bash
     rg -n "build_convergence_cel_context\|pipeline.*variables\|captures\|delta" core/src/prehook/
     ```
  2. Verify convergence expression parsing accepts variable references:
     ```bash
     cargo test -p agent-orchestrator --lib -- convergence_expr --nocapture
     ```
- **预期**: Code review 确认 pipeline variables 注入到 CEL evaluation context; convergence_expr tests 通过

### S6: max_cycles 仍为硬上限
- **前置**: Repository root, Rust toolchain available.
- **操作**:
  ```bash
  cargo test -p orchestrator-scheduler --lib -- fixed_mode_stops_at_max_cycles --nocapture
  cargo test -p orchestrator-scheduler --lib -- proactive_max_cycles_fixed_mode --nocapture
  cargo test -p orchestrator-scheduler --lib -- infinite_mode_respects_max_cycles --nocapture
  ```
- **预期**: max_cycles 始终作为硬上限，无论 convergence_expr 是否匹配

## 自动化覆盖
- S1: `crates/integration-tests/tests/workflow_loop.rs::convergence_expr_stops_loop`
- S2: `crates/integration-tests/tests/workflow_loop.rs::multi_cycle_loop`（回归）
- S4: `core/src/config_load/validate/loop_policy.rs` 单元测试

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | S1: convergence_expr 驱动收敛 | ☑ | S1: task completed after cycle 2, loop_guard_decision reason="test_convergence" confirmed via DB |
| 2 | S2: 缺省行为不变 | ☐ | Rewritten: loop_engine unit tests |
| 3 | S3: 短路求值 | ☐ | Rewritten: code review + validation tests |
| 4 | S4: CEL 编译校验 | ☐ | Rewritten: 3 validation unit tests |
| 5 | S5: pipeline 变量注入 | ☐ | Rewritten: code review + unit tests |
| 6 | S6: max_cycles 硬上限 | ☐ | Rewritten: proactive gate unit tests |
