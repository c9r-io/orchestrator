# FR-130: Core Crate 拆分 Phase 3 — persistence 提取

## 优先级: P1

## 状态: Proposed

## 背景

FR-047（Phase 1，提取 `orchestrator-config`）与 FR-048（Phase 2，提取 `orchestrator-scheduler`）已闭环，但 `core`（`agent-orchestrator`）仍是事实上的 god crate。2026-07-25 实测：

| 指标 | 值 |
|---|---|
| 生产代码行数 | 81194（占工作区约 46%） |
| 文件数 | 157 |
| `lib.rs` 顶层 `pub mod` | 52 |
| 公开项（`pub fn/struct/enum/trait/type/const`） | 742 |
| 下游依赖者 | scheduler、daemon、cli、integration-tests（即全部上层 crate） |

单个 crate 内混装了：领域模型、SQLite 持久化与 51 张表的迁移内核、gRPC DTO、prehook/CEL、可观测性、ticket、source automation、attention、stream_json。

churn 数据印证边界从未稳定——近 400 次提交中 `core/src/lib.rs` 被修改 **40 次**：模块清单在被持续编辑，说明没有稳定的对外缝。同期 `core/src/migration.rs` 45 次、`core/src/persistence/migration.rs` 28 次、`migration_steps.rs` 27 次——**持久化层是 core 内部最高频的变更簇**，也是最容易独立出去的一块（它对领域逻辑的依赖窄，对下游的暴露面集中）。

另有 **43 个 `#[allow(clippy::too_many_arguments)]`**：宽签名是耦合的直接症状，目前被静音而非解决，其中相当一部分出现在跨越持久化边界的函数上。

FR-125/126 的 strangler 实践已证明本项目具备"划定缝 → 冻结棘轮 → 分阶段退役"的能力；本 FR 是把同一方法用在 crate 结构本身，而不是执行路径上。

## 目标

- 提取 `orchestrator-persistence` crate，承载 SQLite 连接管理、迁移内核与迁移步骤、repository 实现。
- 使 `core` 通过窄 trait（port）依赖持久化，而非直接依赖 `rusqlite` 类型。
- 建立 crate 边界棘轮：`core` 的 `pub mod` 数量与 `rusqlite` 直接引用数单调不增。

## 非目标

- **不**在本 FR 内完成 core 的完整分层（domain / service / adapter 三层收口留待后续 Phase）。
- **不**更换数据库或引入 ORM——持久化实现保持 `rusqlite`，只改变它所处的 crate 与暴露方式。
- **不**改变任何 schema 或迁移语义；迁移版本号、执行顺序、幂等性保持不变。
- **不**改变对外 gRPC/CLI 契约。
- **不**要求消除全部 43 个 `too_many_arguments` 豁免；只要求跨越新 crate 边界的那些被消除或书面记录。

## 需求

### 1. 边界识别与冻结基线

- 建立机器可读清单：`core` 中直接引用 `rusqlite`（`Connection`、`params!`、`OptionalExtension` 等）的文件与函数，作为提取前基线。
- 记录 `core` 当前的 `pub mod` 数（52）与公开项数（742）作为棘轮起点。
- 与 FR-124/125 的 `sourceBaseline` 棘轮机制保持同一口径与同一门禁脚本风格。

### 2. orchestrator-persistence crate 提取

- 迁移 `core/src/persistence/**`、`db.rs`、`db_write.rs`、`async_database.rs`、`migration.rs`、`migration_steps.rs` 及 `task_repository/**` 中的实现部分。
- 新 crate 保持 `#![deny(missing_docs)]` 与既有 lint 约定。
- `core` 保留领域侧的 port trait 定义；实现由新 crate 提供并在组装层（daemon/cli bootstrap）注入。

### 3. 迁移语义等价证明

- 提取前后对同一初始数据库执行完整迁移链，比对最终 schema（表、索引、约束）逐项一致。
- 51 张表的存在性与列定义作为可复现断言，而非人工核对。
- 证明迁移的幂等性与断点续跑行为未变。

### 4. 边界棘轮

- 新增门禁：`core` 的 `pub mod` 数与 `rusqlite` 直接引用数不得高于记录基线。
- 跨新边界的 `#[allow(clippy::too_many_arguments)]` 逐个消除或在设计文档中书面记录保留理由。

## 验收标准

- [ ] `orchestrator-persistence` crate 存在，承载迁移内核与 repository 实现，且 `#![deny(missing_docs)]` 通过
- [ ] `core` 不再直接依赖 `rusqlite`（或残余引用点被显式清单化并冻结）
- [ ] 提取前后完整迁移链产出的 schema 逐表逐列一致，以可复现脚本为证
- [ ] 迁移幂等性与断点续跑回归通过
- [ ] `core` 的 `pub mod` 数与公开项数相对基线单调不增，门禁存在且有负向 fixture
- [ ] 跨边界的 `too_many_arguments` 豁免已消除或书面记录
- [ ] 对外 gRPC/CLI 契约无变化（既有集成测试全绿，无需修改断言）
- [ ] `cargo test --workspace`、strict Clippy、边界层覆盖率治理、协调 strangler 门禁全部通过

## QA 计划

- **Schema 等价性**：以提取前的 commit 构建二进制，对空库执行全量迁移并 dump schema；以提取后的二进制重复；逐行 diff 必须为空。此为本 FR 最关键的行为证据——纯结构性的"符号已移动"检查不足以证明迁移仍然正确。
- **幂等与续跑**：对已迁移库重复执行迁移（应无操作）；人为中断在第 N 步后重启（应从 N+1 继续），提取前后行为一致。
- **棘轮负向 fixture**：在 `core/lib.rs` 新增一个 `pub mod` → 门禁失败；在 `core` 中新增一处 `rusqlite::Connection` 引用 → 门禁失败。
- **下游无感证明**：daemon/cli/scheduler 的既有集成测试在不修改断言的前提下全绿。
- **回滚可行性**：证明提取 commit 可机械回退（与 FR-126 的 reverse-applicable removal patch 同一标准）。
