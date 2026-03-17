---
self_referential_safe: false
self_referential_safe_scenarios: [S3, S4, S5, S6, S7]
---

# QA 102: Core Crate Split Phase 2 — orchestrator-scheduler Extraction

## 验证目标

确认 `scheduler/` 模块成功从 core 提取至 `crates/orchestrator-scheduler/`，core LOC 降至 65K 以下，且所有功能保持完整。

## 验证场景

### 场景 1: 编译验证

| 步骤 | 预期结果 |
|------|---------|
| `cargo build --workspace` | 全部编译通过，无 warning |
| `cargo build -p orchestrator-scheduler` | 独立编译通过 |
| `cargo build -p agent-orchestrator` | Core 独立编译通过（不含 scheduler） |

### 场景 2: 测试验证

| 步骤 | 预期结果 |
|------|---------|
| `cargo test --workspace` | 全部通过 |
| `cargo test -p orchestrator-scheduler` | 所有 scheduler 测试通过（~411 tests） |
| `cargo test -p agent-orchestrator` | 所有 core 测试通过（~1,390 tests） |

### 场景 3: LOC 目标验证

| 指标 | 验收标准 | 实际值 |
|------|---------|-------|
| Core LOC (`core/src/`) | < 65,000 | 57,172 |
| Core LOC 降幅 | > 20% | 31.2% |

### 场景 4: 依赖方向验证

| 检查项 | 预期 |
|--------|------|
| `orchestrator-scheduler` Cargo.toml 依赖 `agent-orchestrator` | 是 |
| `agent-orchestrator` Cargo.toml **不**依赖 `orchestrator-scheduler` | 是（无循环依赖） |
| daemon/cli/integration-tests 同时依赖 core 和 scheduler | 是 |

### 场景 5: 死代码清理验证

| 检查项 | 预期 |
|--------|------|
| `core/src/scheduler/` 目录 | 已删除 |
| `core/src/scheduler.rs` | 已删除 |
| `core/src/service/task.rs` | 已删除 |
| `core/src/lib.rs` 无 `pub mod scheduler;` | 已移除 |
| `core/src/service/mod.rs` 无 `pub mod task;` | 已移除 |

### 场景 6: 消费方导入路径验证

| 消费方 | 旧路径 | 新路径 |
|--------|--------|--------|
| daemon | `agent_orchestrator::scheduler::*` | `orchestrator_scheduler::scheduler::*` |
| daemon | `agent_orchestrator::service::task::*` | `orchestrator_scheduler::service::task::*` |
| daemon | `agent_orchestrator::service::system::run_check` | `orchestrator_scheduler::service::system::run_check` |
| integration-tests | 同上 | 同上 |

### 场景 7: 运行时性能验证

| 检查项 | 预期 |
|--------|------|
| 无 `dyn Trait` 动态分发 | 是（直接函数调用） |
| 无泛型参数传播 | 是（inverted dependency 模式） |
| 无额外 allocation/clone | 是 |

## 回归风险

- 如果 core 修改了 `state.rs`、`events.rs`、`db_write.rs` 等被 scheduler 引用的模块，scheduler 需要同步更新
- `trigger_engine.rs` 中的 `cancel_task_for_trigger()` 是 `stop_task_runtime()` 的简化版，如 scheduler 的任务取消逻辑变化需同步更新

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
