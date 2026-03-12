# FR-023: 增加集成测试覆盖

**优先级**: P2
**状态**: Proposed
**目标**: 降低系统级回归风险

## 背景与目标

当前测试体系以单元测试为主，覆盖了模块内部逻辑。但模块间交互（CLI → daemon gRPC → core 调度 → agent 执行）缺少端到端的集成测试覆盖，导致：

- 单模块重构后单元测试通过，但跨模块集成行为回归。
- gRPC 协议变更后 CLI 与 daemon 不兼容问题到部署时才暴露。
- workflow 生命周期（create → start → pause → resume → complete）的完整路径缺少自动化验证。

目标：

- 建立集成测试框架，覆盖 CLI → daemon → core 的关键交互路径。
- 覆盖 workflow 生命周期核心场景（正常流程、异常恢复、超时处理）。
- 集成测试可在 CI 中自动运行，执行时间控制在 5 分钟以内。

非目标：

- 替代现有单元测试（集成测试是补充，不是替代）。
- E2E UI 测试（portal 前端测试由 Playwright 承载）。
- 性能/压力测试。

## 覆盖范围

### 核心场景（必须覆盖）

| 场景 | 涉及模块 | 验证点 |
|------|----------|--------|
| task create → start → complete | CLI, daemon, core | 任务生命周期完整性 |
| task pause → resume | CLI, daemon, core | 暂停/恢复语义正确 |
| agent cordon → drain → uncordon | CLI, daemon, core | agent 生命周期状态机 |
| workflow with failing step | core, agent | 错误传播与 item 状态 |
| workflow with prehook skip | core | 条件跳过语义 |
| multi-cycle loop execution | core | 循环调度与 finalize 规则 |
| gRPC API round-trip | daemon, proto | 协议兼容性 |

### 扩展场景（按优先级逐步补充）

- 并发 task 执行
- drain_timeout 超时强制排空
- dynamic items 生成与执行
- secret store 集成

## 实施方案

### 测试框架选型

使用 Rust 内置的 `#[test]` + `tokio::test` 宏，配合以下基础设施：

- **Test harness**：在 `tests/` 目录下建立共享的 test fixture，包含 daemon 启动/停止、临时数据库创建/清理。
- **gRPC client**：复用 `tonic` 生成的 client stub，直接调用 daemon gRPC 接口。
- **CLI wrapper**：通过 `assert_cmd` crate 调用编译后的 CLI 二进制，验证输出和退出码。
- **Timeout**：所有集成测试设置 30 秒超时，防止挂起。

### 目录结构

```
tests/
├── common/
│   ├── mod.rs          # 共享 fixture（daemon 启动、DB 清理）
│   └── manifests/      # 测试用 workflow manifest YAML
├── lifecycle.rs        # task 生命周期集成测试
├── agent_drain.rs      # agent cordon/drain 集成测试
├── grpc_compat.rs      # gRPC 协议兼容性测试
└── workflow_loop.rs    # 多 cycle loop 执行测试
```

### 实施步骤

1. **搭建 test harness**：daemon in-process 启动、临时 DB、端口随机分配。
2. **实现核心场景测试**：按上表逐个实现。
3. **CI 集成**：在 CI pipeline 中添加 `cargo test --test '*' -- --test-threads=1`（集成测试串行执行避免端口冲突）。
4. **覆盖率追踪**：使用 `cargo-llvm-cov` 追踪集成测试覆盖增量。

## CLI / API 影响

无。本 FR 为测试基础设施建设，不涉及生产代码变更。

## 关键设计决策与权衡

### In-process daemon vs 独立进程

选择 in-process 启动 daemon（作为 `tokio::spawn` 任务），避免进程管理复杂度和端口冲突。代价是测试与 daemon 共享进程空间，panic 可能影响测试框架。

### 串行执行集成测试

集成测试串行执行（`--test-threads=1`），避免多个 daemon 实例的端口/DB 冲突。代价是执行时间较长，但通过限制场景数量控制在 5 分钟以内。

## 风险与缓解

风险：集成测试不稳定（flaky），频繁误报降低信任度。
缓解：所有异步等待使用显式 timeout + retry（最多 1 次），避免依赖 sleep；CI 标记 flaky 测试并隔离修复。

风险：test harness 维护成本高。
缓解：harness 设计为最小化 API，仅提供 daemon 启停和 client 创建，业务逻辑由各测试自行组织。

## 验收标准

- 核心场景表中所有 7 个场景有对应的集成测试。
- 集成测试在 CI 中自动运行且通过。
- 集成测试总执行时间 ≤ 5 分钟。
- test harness 支持并行开发（新增测试无需修改 harness）。
- `cargo test --workspace` 通过（含集成测试）。
- `cargo clippy --workspace --all-targets -- -D warnings` 通过。
