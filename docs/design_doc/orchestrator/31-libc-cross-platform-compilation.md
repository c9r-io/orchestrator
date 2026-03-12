# libc 跨平台编译修复

**Related FR**: `FR-019`
**Related QA**: `docs/qa/orchestrator/70-libc-cross-platform-compilation.md`

## 背景与目标

项目需在 5 个目标平台上编译：`x86_64-unknown-linux-gnu`、`x86_64-unknown-linux-musl`、`aarch64-unknown-linux-gnu`、`x86_64-apple-darwin`、`aarch64-apple-darwin`。但 CI 仅覆盖 `ubuntu-latest`（linux-gnu），且 `crates/cli/Cargo.toml` 中 `libc` 依赖未加平台守卫，导致在非 Unix 目标上产生不必要的依赖拉取。此外，`libc` 版本分散在 3 个 crate 中独立声明，缺乏统一管理。

目标：

- 修复所有 `libc` 依赖声明，确保 `cargo check --workspace` 在所有 5 个目标平台上通过。
- 统一 `libc` 版本为 workspace 级依赖。
- CI 矩阵覆盖全部 5 个目标平台的交叉编译检查。

非目标：

- 不引入新的 FFI 绑定或 native 依赖。
- 不重构现有平台抽象层。

## 设计方案

### 1. Workspace 级依赖统一

在根 `Cargo.toml` 新增 `[workspace.dependencies]` 声明 `libc = "0.2"`，各 crate 改用 `libc = { workspace = true }`。统一版本管理，避免三处独立声明版本漂移。

### 2. CLI crate 依赖修复

`crates/cli/Cargo.toml` 中 `libc` 原为无条件 `[dependencies]`，移至 `[target.'cfg(unix)'.dependencies]`，与 `core` 和 `daemon` 保持一致。

### 3. 测试平台守卫

`core/src/scheduler/phase_runner/tests.rs` 中 `detect_sandbox_violation_detects_cpu_signal` 测试使用 `libc::SIGXCPU`，需加 `#[cfg(unix)]` 守卫，避免在非 Unix 目标上编译失败。

### 4. CI 交叉编译矩阵

新增 `cross-compile` job，覆盖 5 个目标平台。使用 `cargo check --workspace --target <triple>`（仅类型检查，不链接），降低 CI 耗时和依赖复杂度。

| 目标 | Runner | 额外工具 |
|------|--------|----------|
| x86_64-unknown-linux-gnu | ubuntu-latest | — |
| x86_64-unknown-linux-musl | ubuntu-latest | musl-tools |
| aarch64-unknown-linux-gnu | ubuntu-latest | gcc-aarch64-linux-gnu |
| x86_64-apple-darwin | macos-latest | — |
| aarch64-apple-darwin | macos-latest | — |

### 设计决策

1. **check-only 而非 build**：交叉编译仅验证类型正确性，无需链接可执行文件，节省 CI 资源。
2. **workspace 统一依赖**：版本集中管理优先于各 crate 灵活性，对 libc 这类基础 crate 尤为合理。
3. **最小修复原则**：仅修复编译问题，不借机重构平台抽象层。

## 变更文件

| 文件 | 变更 |
|------|------|
| `Cargo.toml` | 新增 `[workspace.dependencies]` 声明 libc |
| `crates/cli/Cargo.toml` | libc 移至 `cfg(unix)` + workspace 引用 |
| `core/Cargo.toml` | libc 改为 workspace 引用 |
| `crates/daemon/Cargo.toml` | libc 改为 workspace 引用 |
| `core/src/scheduler/phase_runner/tests.rs` | SIGXCPU 测试加 `#[cfg(unix)]` |
| `.github/workflows/ci.yml` | 新增 cross-compile job |
