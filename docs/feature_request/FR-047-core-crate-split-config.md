# FR-047: Core Crate 拆分 Phase 1 — orchestrator-config 提取

## 背景

审计组指出 `core` crate 单体承载 89,642 行 Rust 代码，建议拆分为多个子 crate。

经分析，`core` 内部模块按 LOC 分布如下：

| 模块 | LOC | 职责域 |
|------|-----|--------|
| `scheduler/` | 23,772 | 调度引擎 |
| `config/` + `config_load/` | 10,391 | 配置模型与加载 |
| `resource/` | 7,935 | 资源 CRUD |
| `crd/` | 4,695 | CRD 定义 |
| `task_repository/` | 4,790 | 任务持久化 |
| `persistence/` + `db*` + `migration` | 5,653 | 数据库层 |
| 其余 | ~32K | 混杂功能 |

依赖拓扑分析表明 `config/` 和 `config_load/` 是 **叶依赖**——被 scheduler、runner、service 等广泛引用，但自身不反向依赖 scheduler 或 persistence。这使其成为最低风险的首选拆分目标。

## 优先级

P2 — 工程质量改进，不影响功能或安全，但对编译速度和代码组织有实质改善。

## 目标

1. 将 `config/` 和 `config_load/` 提取为独立 workspace member `crates/orchestrator-config`
2. `core` crate 通过 `workspace.dependencies` 依赖 `orchestrator-config`
3. 保持 `pub use` re-export 兼容层，确保 cli/daemon 零修改或极少修改
4. 编译缓存粒度细化——修改 config 模型不触发 scheduler 重编译（反之亦然）

## 拆分范围

### 移入 `crates/orchestrator-config/src/`

- `core/src/config/` (4,372 LOC) — 配置模型定义（WorkflowConfig, TaskExecutionPlan, SafetyProfile 等）
- `core/src/config_load/` (6,019 LOC) — YAML/TOML 加载、overlay、验证
- `core/src/cli_types.rs` (1,186 LOC) — K8s-style 声明式资源类型定义，与 config 强关联
- `core/src/env_resolve.rs` (若仅被 config_load 引用)

### 保留在 `core`

- `scheduler/`, `runner/`, `service/`, `persistence/`, `db*`, `crd/`, `resource/` — 均依赖 config 但不应被 config 反向引用
- `state.rs`, `events.rs`, `metrics.rs` — 运行时状态，与 config 无关

### 边界判定原则

如果某个 `.rs` 文件同时被 config 和 scheduler 双向引用，保留在 `core` 并通过 trait 或 re-export 解决。

## 实施步骤

### Step 1: 创建 crate 骨架

```
crates/orchestrator-config/
├── Cargo.toml
└── src/
    └── lib.rs
```

在 `Cargo.toml` 的 workspace members 中添加 `"crates/orchestrator-config"`。

### Step 2: 迁移源文件

将 `core/src/config/` 和 `core/src/config_load/` 整体移动，调整 `mod` 声明和 `use` 路径。

### Step 3: 建立兼容 re-export

在 `core/src/lib.rs` 中添加：

```rust
pub use orchestrator_config::config;
pub use orchestrator_config::config_load;
```

确保 `crate::config::*` 路径在 core 内部和 cli/daemon 中继续可用。

### Step 4: 修复编译

- 处理 `crate::` 路径引用变更
- 将 config 模块内部的 `crate::error` 等依赖通过 trait 或类型参数解耦，或暂时让 orchestrator-config 依赖一个轻量的 `orchestrator-types` 包

### Step 5: 验证

- `cargo build --workspace`
- `cargo test --workspace`
- 确认增量编译行为：修改 config 文件后仅重编译 orchestrator-config + core（不触发 scheduler 全量重编译）

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| `config` 内部引用 `crate::error` 等 core 类型 | 提取共享类型到 `orchestrator-types` 或让 config 定义自己的 error 类型 |
| re-export 路径变更导致下游编译失败 | Step 3 兼容层 + CI 全量编译验证 |
| `config_load` 依赖 `crd` 的部分类型 | 分析实际依赖，必要时将共享类型下沉到 config crate |

## 预期收益

- **编译速度**：config 变更不再触发 scheduler(24K) 重编译
- **代码组织**：config 职责边界显式化，新贡献者可快速定位
- **为后续拆分铺路**：验证 re-export 兼容层模式，降低 Phase 2 风险

## 验收标准

- [ ] `crates/orchestrator-config` 独立编译通过
- [ ] `cargo test --workspace` 全部通过
- [ ] `core` 中 `config` 和 `config_load` 目录已移除，仅保留 re-export
- [ ] cli/daemon crate 无源码修改或仅 `use` 路径调整
