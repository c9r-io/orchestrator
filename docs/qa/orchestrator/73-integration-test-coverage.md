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

## 场景 6: 多轮循环执行

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test workflow_loop multi_cycle_loop -- --test-threads=1
```

**预期**: 测试通过。3 轮 Fixed 循环全部执行，事件中包含多轮 cycle 记录。

## 场景 7: gRPC 协议兼容性

**步骤**:
```bash
cargo test -p orchestrator-integration-tests --test grpc_compat -- --test-threads=1
```

**预期**: 3 个子测试全部通过：
- `ping_roundtrip`: Ping 返回版本号和 lifecycle 状态
- `task_crud_roundtrip`: Task CRUD（create/list/info/delete）round-trip 正确
- `apply_get_describe_roundtrip`: Resource apply/get/describe round-trip 正确

## 全量回归

**步骤**:
```bash
cargo test --workspace
```

**预期**: 所有测试通过（包含集成测试）。总执行时间 ≤ 5 分钟。
