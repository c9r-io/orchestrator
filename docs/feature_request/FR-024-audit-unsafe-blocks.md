# FR-024: 审计 unsafe 块

**优先级**: P2
**状态**: Proposed
**目标**: 强化内存安全保障

## 背景与目标

Rust 的内存安全保证建立在编译器的借用检查之上，而 `unsafe` 块绕过了这些检查。项目中存在约 35 处 `unsafe` 块，每一处都是潜在的内存安全漏洞（use-after-free、data race、undefined behavior）的入口。

在 orchestrator 作为长时间运行的 daemon 场景下，内存安全漏洞可能导致：

- 静默数据损坏（状态机状态错乱）。
- 服务崩溃（segfault，无法被 Rust panic 机制捕获）。
- 安全漏洞（如果 unsafe 代码处理外部输入）。

目标：

- 逐一审计全部 `unsafe` 块，确认每处的必要性和正确性。
- 消除不必要的 `unsafe`（可用 safe Rust 替代的场景）。
- 为必须保留的 `unsafe` 块补充 `// SAFETY:` 注释，记录不变量和正确性论证。
- 建立 CI 门禁防止未审计的 `unsafe` 引入。

非目标：

- 消除所有 `unsafe`（FFI 绑定、性能关键路径中合理的 unsafe 可保留）。
- 替换依赖 crate 中的 `unsafe`（仅审计本项目代码）。
- 形式化验证（人工审计 + 测试覆盖即可）。

## 审计框架

### 分类标准

| 类别 | 定义 | 处置 |
|------|------|------|
| 可消除 | 存在 safe 替代方案且性能差异可忽略 | 替换为 safe 代码 |
| FFI 必需 | `libc` / `nix` / C 库调用，无法避免 unsafe | 保留，补充 SAFETY 注释 |
| 性能必需 | safe 替代方案有可测量的性能开销（需基准测试证明） | 保留，补充 SAFETY 注释 + benchmark 引用 |
| 可疑 | 无法确认正确性，或不变量条件不够严格 | 标记为高优修复，重写或加强测试 |

### 审计清单（每处 unsafe 块）

- [ ] 标注位置（文件:行号）
- [ ] 分类（可消除 / FFI 必需 / 性能必需 / 可疑）
- [ ] 列出前置不变量（调用前必须满足的条件）
- [ ] 列出后置保证（unsafe 块执行后保证成立的条件）
- [ ] 确认是否存在 Miri 可检测的 UB
- [ ] SAFETY 注释是否存在且完整

## 实施方案

### 第一步：全量扫描

- `cargo clippy` + `grep -rn "unsafe"` 定位所有 `unsafe` 块。
- 使用 `cargo-geiger` 生成 unsafe 使用报告。
- 输出审计表格（位置、类别、当前 SAFETY 注释状态）。

### 第二步：分批处置

- **批次 1**：可消除类（预期约 40-50% 的 unsafe 可消除）。
- **批次 2**：可疑类（需重写或加强不变量检查）。
- **批次 3**：FFI/性能必需类（补充 SAFETY 注释）。

### 第三步：Miri 验证

- 对保留的 `unsafe` 块编写针对性测试。
- 在 CI 中运行 `cargo +nightly miri test`（至少覆盖含 unsafe 的模块）。

### 第四步：CI 门禁

- 启用 `clippy::undocumented_unsafe_blocks` lint（deny 级别）。
- 新增 `unsafe` 必须附带 `// SAFETY:` 注释，否则 CI 失败。
- 可选：引入 `#![forbid(unsafe_code)]` 对不需要 unsafe 的 crate。

## CLI / API 影响

无。本 FR 为内部代码安全审计，不涉及用户可见接口变更。

## 关键设计决策与权衡

### 审计优先 vs 盲目消除

先审计分类，再决定处置。避免盲目消除导致 FFI 调用中断或性能回归。

### Miri 作为补充而非替代

Miri 能检测部分 UB（如 use-after-free、alignment violation），但不能证明 unsafe 代码完全正确。人工审计仍是主要手段，Miri 作为自动化补充。

### `forbid(unsafe_code)` 的适用范围

仅对确认不需要 unsafe 的 crate 启用 `forbid`（如纯业务逻辑 crate）。对含 FFI 的 crate 使用 `deny(undocumented_unsafe_blocks)` 即可。

## 风险与缓解

风险：消除 unsafe 后引入功能回归。
缓解：每处替换伴随单元测试验证；分批 PR 降低单次变更范围。

风险：审计遗漏隐蔽的 unsafe 使用（如 `macro_rules!` 内部生成的 unsafe）。
缓解：`cargo-geiger` 能检测宏展开后的 unsafe；Miri 覆盖运行时路径。

风险：Miri 执行时间过长影响 CI。
缓解：Miri 仅运行含 unsafe 的模块测试，不全量运行；或设为 nightly CI job。

## 验收标准

- 审计报告覆盖全部 `unsafe` 块（约 35 处），含分类和处置建议。
- 可消除类 unsafe 全部替换为 safe 代码。
- 保留的 unsafe 块 100% 包含 `// SAFETY:` 注释。
- `clippy::undocumented_unsafe_blocks` lint 以 deny 级别生效。
- 至少一个纯逻辑 crate 启用 `#![forbid(unsafe_code)]`。
- `cargo +nightly miri test` 覆盖含 unsafe 的模块且无 UB 报告。
- `cargo test --workspace` 通过。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
