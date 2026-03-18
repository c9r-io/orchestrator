# FR-060: 减少 QA 场景中的不安全操作

## 背景

当前 139 个 QA 文档中有 114 个标记为 `self_referential_safe: false`，无法在 full-QA
自回归测试中执行。这些文档中的"不安全操作"主要分为以下几类：

| 类别 | 数量 | 典型操作 |
|------|------|---------|
| kill daemon | 14 | `kill $(cat data/daemon.pid)`, SIGTERM/SIGKILL |
| cargo build | 27 | `cargo build --release -p orchestratord` |
| task create | 5 | 创建触发 self_restart 的任务 |
| resource modify | 2 | `orchestrator apply` 修改运行中资源 |
| --force/--unsafe | 2 | 测试破坏性 CLI flag |
| 其他（工作流/CLI 交互） | ~64 | 创建工作流、agent 协作等有副作用的操作 |

**核心论点**：用户在正常使用中不应频繁需要 kill daemon。大量 QA 场景依赖 kill daemon
说明 orchestrator 存在机能缺失（缺少安全的生命周期管理 API），或 QA 文档设计过于暴力。

## 目标

通过小步迭代，逐批分析 unsafe QA 场景，判断每个属于：

1. **Bug** — 实现有误，应修复代码
2. **机能缺失** — 缺少 API/功能，应补充
3. **QA 设计不合理** — 测试方法过于暴力，可用安全方式验证

每次迭代分析 1-2 个 QA 文档，实施修复，然后将其从 unsafe 转为 safe 或部分 safe。

## 迭代计划

### 优先级排序原则

1. **高频操作优先** — kill daemon（14 个）和 cargo build（27 个）影响最大
2. **有明确 API gap 的优先** — 比如缺少 `daemon restart` 命令
3. **QA 文档可直接修正的优先** — 低成本高收益

### 迭代 1: daemon 生命周期 API（kill daemon 类）

**目标文档**: `111-daemon-proper-daemonize.md`, `53-client-server-architecture.md`

**分析方向**:
- `orchestrator daemon stop` 是否已实现？行为是否安全？
- 是否可以在不 kill daemon 的情况下验证关闭行为？
- 是否需要 `orchestrator daemon restart` 命令？

**可能的修复**:
- 提供 `orchestrator daemon stop --graceful` API
- QA 文档改用 CLI 命令而非直接 `kill`

### 迭代 1 结果（2026-03-18）

**分析文档**: `111-daemon-proper-daemonize.md`, `53-client-server-architecture.md`

| 文档 | 总场景 | 可转安全 | 分类 |
|------|--------|---------|------|
| QA-111 | 8 | 0 | 天然不安全 — 所有场景本质上测试 daemon 生命周期（启动/停止/信号），无法在自回归模式下执行 |
| QA-53 | 5 | 4 (S2-S5) | QA 设计问题 — S2-S5 核心逻辑仅为 CLI 命令，daemon start/kill 包装层为多余操作 |

**修复内容**:
- QA-53: 添加 `self_referential_safe_scenarios: [S2, S3, S4, S5]`，删除 S2-S5 中的 daemon 启动/停止步骤
- QA-111: 无变更，已正确标记为 unsafe
- 无代码变更 — `orchestrator daemon stop` 已有自终止防护（拒绝子进程停止父 daemon）

**净收益**: +4 个可安全执行的场景（QA-53 S2-S5）

### 迭代 2: crash resilience 验证方式（kill -9 类）

**目标文档**: `85-daemon-crash-resilience.md`, `86-orphaned-running-items-recovery.md`, `91b-daemon-crash-resilience-shutdown.md`

**分析方向**:
- crash resilience 是否可以通过 unit test 验证而非真的 kill -9？
- 是否可以提供 `orchestrator debug crash-simulate` 测试命令？
- recovery 逻辑是否有独立的集成测试？

**可能的修复**:
- 将 crash 测试改为 unit test + 代码审查验证
- 或提供安全的 crash simulation API

### 迭代 2 结果（2026-03-18）

**分析文档**: `85-daemon-crash-resilience.md`, `86-orphaned-running-items-recovery.md`, `91b-daemon-crash-resilience-shutdown.md`

| 文档 | 总场景 | 可转安全 | 分类 |
|------|--------|---------|------|
| QA-85 | 5 | 5 | QA 设计不合理 — 所有场景的核心逻辑已有 unit test 覆盖（`state_tests.rs` 5 tests, `lifecycle.rs` 3 tests, `runtime.rs` 1 test），kill -9 验证方式可用代码审查 + unit test 替代 |
| QA-86 | 5 | 5 | QA 设计不合理 — 所有 5 个 recovery 场景已有对应 unit test（`recover_orphaned_running_items` 5 tests, `recover_stalled_running_items` 1 test），daemon 生命周期操作为多余包装 |
| QA-91b | 2 | 1 (S2) | S1 已安全（代码审查），S2 `cargo test --workspace --lib` 不影响运行中 daemon，可标记安全 |

**修复内容**:
- QA-85: 全部 5 场景重写为代码审查 + unit test 验证，标记 `self_referential_safe: true`
- QA-86: 全部 5 场景重写为代码审查 + unit test 验证，标记 `self_referential_safe: true`
- QA-91b: 移除 `self_referential_safe_scenarios` 限制，标记 `self_referential_safe: true`（S1+S2 均安全）
- 无代码变更 — 所有 crash recovery 逻辑已有充分 unit test 覆盖
- 10 个相关 unit test 全部通过（7 in core, 3 in daemon）

**净收益**: +11 个可安全执行的场景（QA-85 ×5, QA-86 ×5, QA-91b S2 ×1）
**累计净收益**: 迭代 1 (+4) + 迭代 2 (+11) = +15 个安全场景

### 迭代 3: cargo build 隔离（重编译类）

**目标文档**: `14-config-validation-enhanced.md`, `71-automate-protoc-dependency.md`, `101/102-core-crate-split.md`

**分析方向**:
- 这些 QA 场景是否真的需要重编译？还是只需要验证编译逻辑？
- `cargo test` 是否足以验证（不需要 `cargo build --release`）？
- 是否可以改为 `cargo check` 替代 `cargo build`？

**可能的修复**:
- QA 文档改用 `cargo test --lib` 或 `cargo check`
- 标记为 safe（因为 `cargo test` 不影响 running binary）

### 迭代 4: self_restart 隔离（task create 类）

**目标文档**: `95-prehook-self-referential-safe-filter.md`, `scenario2/3/4`

**分析方向**:
- 是否可以用 `--no-start` 创建任务并只验证资源是否正确？
- 是否需要 task-level isolation（子任务不触发父 daemon restart）？

### 迭代 5+: 逐步处理剩余类别

每次 1-2 个文档，持续推进直到 unsafe 比例从 82% 降到合理水平（目标 < 30%）。

## 成功标准

1. unsafe QA 文档数量从 114 降到 < 40（至少减少 65%）
2. 每个仍为 unsafe 的文档有明确的技术原因（不是"QA 设计不合理"）
3. full-QA 自回归测试可执行的 safe 文档数量从 ~25 增加到 > 80
4. 零 daemon 被 QA agent 意外 kill 的情况

## 约束

1. 每次迭代只改 1-2 个 QA 文档 + 对应的最小代码变更
2. 不破坏现有 safe 文档的通过率
3. 保持 daemon 稳定性（incarnation 不变）
4. 优先修改 QA 文档而非实现新功能（低风险优先）
