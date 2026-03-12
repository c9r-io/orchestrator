# FR-021: 审计并减少 expect() 调用

**优先级**: P1
**状态**: Proposed
**目标**: 提升运行时稳定性

## 背景与目标

Rust 中 `expect()` / `unwrap()` 在遇到 `None` 或 `Err` 时会触发 panic，导致线程或进程崩溃。在 daemon 长时间运行的场景下，panic 意味着服务中断，对生产可用性构成直接威胁。

当前代码库中存在大量 `expect()` 调用，其中部分位于请求处理路径或状态机关键转换中，一旦触发将导致不可恢复的服务中断。

目标：

- 审计全部 `expect()` 和 `unwrap()` 调用点，按风险等级分类。
- 将高风险路径（请求处理、状态机转换、I/O 操作）中的 `expect()` 替换为显式错误处理（`?` 操作符或 `match`）。
- 保留低风险路径（初始化阶段、测试代码、已证明不可能失败的调用）中的 `expect()`，并补充注释说明安全性。
- 建立 clippy lint 规则防止未来引入高风险 `expect()`。

非目标：

- 消除所有 `expect()` / `unwrap()`（零 expect 不是目标，合理使用是允许的）。
- 重构错误类型体系（仅替换 panic 点，不改变错误传播架构）。

## 风险分类

| 风险等级 | 特征 | 处置策略 |
|----------|------|----------|
| 高 | 位于 `async fn` 请求/响应路径、状态机 transition、文件/网络 I/O | 必须替换为 `?` 或 `match`，返回 `Result`/`Option` |
| 中 | 位于后台任务、周期性 sweep、非关键日志路径 | 替换为 `unwrap_or_else` + 日志 + 降级处理 |
| 低 | 位于 `main()` 初始化、`#[cfg(test)]`、编译期保证不失败（如 `Regex::new` 常量模式） | 保留，补充 `// SAFETY:` 注释 |

## 实施方案

### 第一步：审计

- 使用 `cargo clippy` 自定义 lint 或 `grep` 统计所有 `expect()` / `unwrap()` 调用点。
- 按文件、函数、调用上下文标注风险等级。
- 输出审计报告（Markdown 表格），列出每个调用点的位置、上下文、风险等级、建议处置。

### 第二步：分批修复

- **批次 1**：高风险路径（daemon server handlers、gRPC service impl、状态机 transition）。
- **批次 2**：中风险路径（后台 sweep、定时任务）。
- **批次 3**：低风险路径（补充 `// SAFETY:` 注释）。

每批次独立 PR，逐步合入，降低回归风险。

### 第三步：防护

- 在 `.clippy.toml` 或 CI 中启用 `clippy::expect_used` / `clippy::unwrap_used` lint（warn 级别）。
- 对新增的 `expect()` 要求附带 `// SAFETY:` 注释说明为何不会 panic。

## CLI / API 影响

无。本 FR 为内部代码质量改进，不涉及用户可见接口变更。错误处理改进后，部分原本 panic 的场景将返回错误响应而非服务崩溃，属于行为改善。

## 关键设计决策与权衡

### Warn 而非 Deny

选择 `warn` 级别的 clippy lint 而非 `deny`，因为 `expect()` 在某些上下文中是合理的（如测试代码、初始化阶段）。`deny` 会导致大量 `#[allow]` 标注，降低可读性。

### 分批修复 vs 一次性替换

分批修复降低每次变更的回归风险，便于 code review，也允许在修复过程中发现并修正错误传播路径中的遗漏。

## 风险与缓解

风险：替换 `expect()` 后错误被静默吞掉，掩盖真实问题。
缓解：所有替换点必须记录错误日志（`tracing::error!`），确保问题可观测。

风险：大范围修改引入行为回归。
缓解：分批 PR + 现有测试覆盖 + 手动 QA 验证关键路径。

风险：部分 `expect()` 调用实际上不可能失败，替换为错误处理引入不必要的复杂度。
缓解：低风险调用保留 `expect()` 并补充安全注释，不做无谓替换。

## 验收标准

- 审计报告覆盖全部 `expect()` / `unwrap()` 调用点，含风险分类。
- 高风险路径中无残留的未处理 `expect()`（测试代码除外）。
- 所有替换点包含错误日志记录。
- 低风险保留点包含 `// SAFETY:` 或等效注释。
- `cargo test --workspace` 通过。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- CI 中 `clippy::expect_used` lint 以 warn 级别生效。
