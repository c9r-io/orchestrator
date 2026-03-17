---
self_referential_safe: false
---

# QA: Integration Test Coverage — Advanced (FR-023)

**Split from**: `docs/qa/orchestrator/73-integration-test-coverage.md`

## 前置条件

- 项目已编译通过
- 工作目录为项目根目录

## 场景 1: 多轮循环执行

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test workflow_loop multi_cycle_loop -- --test-threads=1
```

**预期**: 测试通过。3 轮 Fixed 循环全部执行，事件中包含多轮 cycle 记录。

## 场景 2: gRPC 协议兼容性

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test grpc_compat -- --test-threads=1
```

**预期**: 3 个子测试全部通过：
- `ping_roundtrip`: Ping 返回版本号和 lifecycle 状态
- `task_crud_roundtrip`: Task CRUD（create/list/info/delete）round-trip 正确
- `apply_get_describe_roundtrip`: Resource apply/get/describe round-trip 正确

## 场景 3: 全量回归

**步骤**:
```bash
cargo test --workspace
```

**预期**: 所有测试通过（包含集成测试）。总执行时间 ≤ 5 分钟。

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
