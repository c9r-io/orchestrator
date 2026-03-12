# FR-022: 补充公共 API 文档注释

**优先级**: P1
**状态**: Proposed
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

- `cargo doc --workspace --no-deps` 无 `missing_docs` 警告。
- 核心 crate（`core`、`daemon`、`cli`）的所有 `pub` 接口包含 `///` 文档注释。
- 关键接口包含 `# Examples` 代码块且 `cargo test --doc` 通过。
- 各已完成 crate 的 `lib.rs` 中启用 `#![deny(missing_docs)]`。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
