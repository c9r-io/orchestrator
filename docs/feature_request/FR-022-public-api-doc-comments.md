# FR-022: 补充公共 API 文档注释

**优先级**: P1
**状态**: In Progress
**目标**: 提升 API 可发现性

## 背景与目标

项目作为 orchestrator 平台，其公共 API（gRPC service、CLI 子命令、核心 crate 的 `pub` 接口）是外部集成和内部开发的主要接触面。当前大量公共接口缺少 `///` 文档注释，导致：

- `cargo doc` 生成的文档缺少描述，开发者难以理解接口用途和约束。
- IDE 悬停提示无内容，降低开发效率。
- 新贡献者理解成本高，需要阅读实现代码才能了解接口语义。

目标：

- 为所有公共 crate 的 `pub` 函数、结构体、枚举、trait 补充 `///` 文档注释。
- 文档注释包含：功能描述、参数说明（如适用）、返回值语义、panic 条件（如有）、示例（关键接口）。
- `cargo doc --workspace --no-deps` 无 `missing_docs` 警告。
- 建立 lint 规则防止未来公共接口缺少文档。

## 当前实现状态

已完成：

- [x] 在 `crates/cli` 与 `crates/daemon` 启用 `#![warn(missing_docs)]`，将缺失文档纳入常规编译/文档检查信号。
- [x] 为 `cli` 与 `daemon` 的主要公共入口补充类型级 `///` 文档注释，包括 CLI 子命令模型、客户端连接入口、控制面安全配置与流量保护层。
- [x] 修复一个现存 rustdoc 失效链接，确保 `cargo doc --workspace --no-deps` 当前无文档告警。
- [x] 验证 `cargo check -p orchestrator-cli -p orchestratord`、`cargo doc --workspace --no-deps`、`cargo clippy -p orchestrator-cli -p orchestratord --all-targets -- -D warnings` 与 `cargo test --doc --workspace` 通过。
- [x] 在 `core/src/lib.rs` 启用 `#![warn(missing_docs)]`，将核心 crate 的缺失文档正式纳入编译信号。
- [x] 为 `core` crate 根与第一批高接触面 API（`dto`、`error`、`anomaly`、`async_database`、`ticket`）补充英文 `///` 文档注释，并为 crate 根新增 `# Examples` 代码块。
- [x] 重新审计 `core` 文档缺口，确认 `cargo check -p agent-orchestrator --all-targets` 仍通过，`missing_docs` 告警从 2038 条下降到 1797 条。
- [x] 为第二批核心公开 API 补充英文 `///` 文档注释，覆盖 `cli_types`、`collab::{artifact,context,dag,message,output}`、`task_repository::{mod,trait_def,types,command_run}`、`config::{mod,agent}`、`session_store`、`state` 等主要对外模型与仓储入口。
- [x] 重新验证 `cargo check -p agent-orchestrator --all-targets`，确认 `core` 的 `missing_docs` 告警进一步从 1797 条下降到 1213 条。

剩余：

- [ ] `core` crate 仍有大规模公共 API 缺失文档；最新基线为 `cargo check -p agent-orchestrator --all-targets` 产生 1213 条 `missing_docs` 告警，尚未达到 FR 要求的“核心 crate 全量补齐”。
- [ ] 尚未将任何 crate 升级到 `#![deny(missing_docs)]`，因为 `core` 仍未完成全面治理。
- [ ] 关键 `core` API 仍缺少更细粒度的 `# Examples` 代码块，当前仅 crate 根新增示例，尚不足以覆盖核心集成入口。
- [ ] 当前缺口已主要收敛到 `config::{execution,execution_profile,invariant,...}`、`service::*`、`store::*`、`secret_*`、`selection`、`self_referential_policy` 等模块，仍需继续分批治理。

非目标：

- 为私有函数或内部模块补充文档（按需即可）。
- 编写用户指南或教程（已由 `docs/guide/` 承载）。
- 修改现有接口签名。

## 实施方案

### 第一步：范围界定

- 在 workspace 根 `Cargo.toml` 或各 crate 的 `lib.rs` 中启用 `#![warn(missing_docs)]`。
- 执行 `cargo doc --workspace --no-deps 2>&1 | grep "missing documentation"` 统计缺失点。
- 按 crate 分组，优先处理核心 crate（`core`、`daemon`、`cli`）。

### 第二步：分 crate 补充

按以下优先级逐 crate 补充：

1. **`core`**：数据模型、状态机、调度器接口 — 其他 crate 的基础。
2. **`daemon`**：gRPC service handlers、server 配置 — 外部集成点。
3. **`cli`**：命令定义、输出格式 — 用户直接接触面。
4. **其他 crate**：按依赖关系顺序补充。

### 第三步：质量保障

- 文档注释遵循 Rust API Guidelines（`#[doc]` 约定）。
- 关键接口包含 `# Examples` 代码块，确保 `cargo test --doc` 通过。
- 启用 `#![deny(missing_docs)]` 防止回归（对已完成 crate）。

## 文档注释规范

```rust
/// 简要描述（一行，句号结尾）。
///
/// 详细描述（如需要，可多段）。
///
/// # Arguments
///
/// * `param` - 参数说明
///
/// # Returns
///
/// 返回值语义说明。
///
/// # Errors
///
/// 返回 `Err` 的条件说明。
///
/// # Panics
///
/// panic 条件说明（如有）。
///
/// # Examples
///
/// ```rust
/// // 使用示例（关键接口必须提供）
/// ```
```

## CLI / API 影响

无。本 FR 为文档补充，不修改任何运行时行为。

## 关键设计决策与权衡

### `warn` 先行，逐步 `deny`

先以 `warn` 级别启用 `missing_docs`，在各 crate 文档补充完成后逐个升级为 `deny`。避免一次性阻断 CI。

### 中文 vs 英文

公共 API 文档注释统一使用英文（Rust 生态惯例），用户指南保持中英双语。

## 风险与缓解

风险：文档注释与实现不一致，产生误导。
缓解：`cargo test --doc` 验证示例代码可编译运行；code review 环节检查文档与实现一致性。

风险：大量文档补充 PR 导致 review 负担。
缓解：按 crate 分批提交，每批聚焦单一 crate。

## 验收标准

- [x] `cargo doc --workspace --no-deps` 无当前文档告警。
- [ ] 核心 crate（`core`、`daemon`、`cli`）的所有 `pub` 接口包含 `///` 文档注释。
- [ ] 关键接口包含 `# Examples` 代码块且 `cargo test --doc` 通过。
- [ ] 各已完成 crate 的 `lib.rs`/crate root 中启用 `#![deny(missing_docs)]`。
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 通过。

## 后续治理建议

1. 以 `core/src/lib.rs` 暴露的顶层模块为边界，按模块批次补齐公开 API 文档，而不是一次性横扫整个 crate。
2. 每完成一批 `core` 模块后，移除对应 `#[allow(missing_docs)]` 豁免并缩小预警面，最终再升级为 `#![deny(missing_docs)]`。
3. 优先为真正面向外部集成的 `core::service::*`、`core::config::*` 与 `core::dto::*` 补充 `# Examples`。
4. 下一轮治理建议从 `core/src/config/execution.rs`、`core/src/service/` 与 `core/src/store/` 开始，这三块仍是当前最大告警簇。
