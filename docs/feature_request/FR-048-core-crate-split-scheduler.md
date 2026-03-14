# FR-048: Core Crate 拆分 Phase 2 — orchestrator-scheduler 提取

## 背景

延续 FR-047 的 core crate 拆分策略。`scheduler/` 模块以 23,772 LOC 占据 core crate 的 26.5%，是最大的单一模块。将其提取为独立 crate 是降低 core 复杂度的关键步骤，但因其依赖关系复杂，需在 FR-047 完成后再执行。

## 前置依赖

- **FR-047** (orchestrator-config 提取) 必须先完成——scheduler 重度依赖 config 类型，config 独立后 scheduler 可直接依赖 orchestrator-config 而非 core

## 优先级

P2 — 工程质量改进。依赖 FR-047 完成后评估实际收益再决定是否执行。

## 目标

1. 将 `scheduler/` 提取为独立 workspace member `crates/orchestrator-scheduler`
2. 梳理并最小化 scheduler 对 core 其他模块的依赖
3. 保持 `core` re-export 兼容层

## 依赖复杂度分析

`scheduler/` 模块当前引用的 core 内部模块：

| 被引用模块 | 引用方式 | 解耦难度 |
|-----------|---------|---------|
| `config` / `config_load` | 类型 + 函数调用 | 低（FR-047 后变为 crate 依赖） |
| `db` / `db_write` | 直接 SQL 操作 | 中（需抽象为 trait 或提取 persistence crate） |
| `events` / `event_cleanup` | 事件写入 | 中（可通过 trait 抽象） |
| `runner` | 命令执行 | 高（紧耦合，可能需同时迁移） |
| `state` | 运行时状态共享 | 高（共享可变状态） |
| `metrics` | 指标记录 | 低（trait 抽象） |
| `prehook` | CEL 表达式求值 | 中 |
| `dynamic_orchestration` | 动态 item 生成 | 中 |

总计 **8+ 个模块** 的交叉依赖，说明直接提取 scheduler 需要大量 trait 抽象或同时迁移部分模块。

## 拆分范围

### 移入 `crates/orchestrator-scheduler/src/`

- `core/src/scheduler/` (23,772 LOC) — 全部调度逻辑
- `core/src/scheduler.rs` (1,698 LOC) — 顶层调度入口

### 可能需要同时迁移的模块

- `core/src/runner/` (1,776 LOC) — 与 scheduler 紧耦合的命令执行层
- `core/src/prehook/` (3,061 LOC) — 被 scheduler 独占使用的 CEL 钩子引擎

### 需要 trait 抽象的依赖

```rust
// scheduler 需要的 persistence 抽象
pub trait SchedulerPersistence {
    async fn load_task(&self, id: &str) -> Result<TaskRecord>;
    async fn update_item_status(&self, id: &str, status: ItemStatus) -> Result<()>;
    async fn emit_event(&self, event: Event) -> Result<()>;
    // ...
}
```

## 实施步骤

### Step 1: 依赖图精确化

使用 `cargo depgraph` 或手工分析，绘制 scheduler 对 core 其余模块的完整 use 图，量化每条边的调用频次。

### Step 2: 定义 trait 边界

为 scheduler 对 persistence、events、state 的依赖定义 trait 接口。trait 放在 orchestrator-config 或新建的 `orchestrator-types` crate 中。

### Step 3: 创建 crate 并迁移

```
crates/orchestrator-scheduler/
├── Cargo.toml  (deps: orchestrator-config, orchestrator-types)
└── src/
    ├── lib.rs
    ├── runtime.rs
    ├── phase_runner/
    ├── loop_engine/
    ├── item_executor/
    └── ...
```

### Step 4: core 中实现 trait 并注入

core 的 `lib.rs` 组装 scheduler，通过 trait impl 注入 persistence、events 等依赖。

### Step 5: 验证

- `cargo build --workspace`
- `cargo test --workspace`
- 增量编译测试：修改 scheduler 不触发 persistence 重编译

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| trait 抽象引入运行时开销 | 使用泛型而非 `dyn Trait`，保持零成本抽象 |
| 解耦工作量超预期 | Step 1 精确量化后再决策，如 trait 边界 > 20 个方法则考虑降级方案 |
| runner/prehook 与 scheduler 循环依赖 | 将 runner + prehook 同时移入 scheduler crate |
| 拆分后调试困难 | 保持 `core` re-export，gdb/lldb 符号路径不变 |

## 降级方案

如果 Step 1 分析表明解耦成本过高，可降级为以下方案之一：

1. **仅迁移 scheduler + runner + prehook 作为一个粗粒度 crate**（~28.6K LOC），减少 trait 边界数量
2. **保持 core 单 crate，仅做模块内 `pub(crate)` 可见性治理**——不拆 crate，但通过限制模块间可见性实现逻辑隔离
3. **推迟执行**——等待项目进入维护期、迭代频率降低后再拆分

## 验收标准

- [ ] 依赖分析报告完成，trait 边界清单确认
- [ ] `crates/orchestrator-scheduler` 独立编译通过
- [ ] `cargo test --workspace` 全部通过
- [ ] core crate LOC 降至 65K 以下
- [ ] 无运行时性能回归（trait 使用泛型单态化）
