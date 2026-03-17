---
self_referential_safe: false
---

# QA: Integration Test Coverage (FR-023)

## 验证范围

验证集成测试框架正确覆盖 CLI → daemon → core 的 7 个核心交互场景。

## 前置条件

- 项目已编译通过
- 工作目录为项目根目录

## 场景 1: 任务生命周期 — 创建 → 启动 → 完成

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test lifecycle task_create_start_complete -- --test-threads=1
```

**预期**: 测试通过。任务经历 created → enqueued → completed 状态转换。

## 场景 2: 任务暂停 → 恢复

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test lifecycle task_pause_resume -- --test-threads=1
```

**预期**: 测试通过。任务可被暂停（paused 状态），恢复后重新入队（enqueued 状态）。

## 场景 3: Agent 生命周期 — cordon → drain → uncordon

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test agent_drain agent_cordon_drain_uncordon -- --test-threads=1
```

**预期**: 测试通过。Agent 状态依次为 Active → Cordoned → Drained → Active。

## 场景 4: 失败步骤的错误传播

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test workflow_loop workflow_failing_step -- --test-threads=1
```

**预期**: 测试通过。使用 `exit 1` agent 命令的任务最终状态为 failed。

## 场景 5: Prehook 条件跳过

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test workflow_loop workflow_prehook_skip -- --test-threads=1
```

**预期**: 测试通过。带 `when: "is_last_cycle"` 的步骤在第 1 轮被跳过，第 2 轮执行。

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |

See also: `docs/qa/orchestrator/73b-integration-test-coverage-advanced.md` for multi-cycle loop, gRPC compat, and full regression.
