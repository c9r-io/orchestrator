# FR-126: Agent 执行路径迁移 — legacy command 模板 → typed driver（解锁 ShellRunnerExecutor 退役）

## 优先级: P2

## 状态: Proposed

## 背景

FR-125 在划定遗留协调机器退役范围时，显式把 `ShellRunnerExecutor` 的移除**移交独立 FR**，理由是它卡在与协调退役正交的**另一条迁移轴**：Agent 进程执行路径由 legacy `command:` 模板迁移到 provider-neutral typed driver。本 FR 承接该移交。

driver 基础设施已由 DD-127（FR-116）建成并证明：`shell/cli`、`claude/cli`、`codex/cli` 三种 provider/transport 组合，统一走 runner 的 policy/sandbox/rlimit/进程组，phase 从 `setup → spawn → wait → validate → record` 改为 `setup → start → consume → fold → record`。重活工作流已全迁：

| 状态 | 生产工作流 |
|---|---|
| 已全 driver | full-qa、self-bootstrap、plan-execute、qa-loop、command-rules |
| 部分迁移（混用） | promotion（command 3 / driver 4）、self-evolution（command 1 / driver 6） |
| 仅 legacy command | hello-world、scheduled-scan、fr-watch、streaming-mark-done-convergence（各 1，均为 demo/模板/治理工作流） |

关键点：`shell/cli` driver 使**纯 shell/脚本 Agent 也能迁到 driver 席位而不改变执行语义**——这是让 legacy `command:` 执行路径达到零生产消费者、从而移除 `ShellRunnerExecutor` legacy 分支的前提。

本 FR 不重新论证 driver 设计（DD-127 已 closed）；它是最后一公里迁移 + 执行侧 legacy 路径退役。

## 目标

- 将剩余 legacy `command:`-only 生产 Agent 迁移到显式 driver：AI Agent → `claude/cli` 或 `codex/cli`，纯 shell/脚本 Agent → `shell/cli`，均附行为对等证据。
- 令 legacy `command:` 执行路径在生产工作流中达到零消费者。
- 零消费者达成后，移除 legacy 非-driver 执行路径（`ShellRunnerExecutor` legacy 分支），并把 FR-125 ledger 的 `shellRunnerExecutor` 状态从 `frozen` 推进到 `remove`，附兼容窗口与回滚证据。
- 建立执行侧冻结棘轮：新 Agent 必须使用 driver，新增 legacy `command:`-only Agent 被拒绝/告警。

## 非目标

- **不**移除各 driver 共享的 sandbox spawn 函数与 runner policy/profile/rlimit/进程组路径——这些是所有 driver 的公共底座，`ShellRunnerExecutor` legacy 分支退役不等于删除公共 spawn。
- **不**改变纯 shell/脚本 Agent 的执行语义——`shell/cli` driver 保持相同命令、相同沙箱、相同输出契约，仅改走 driver seam。
- **不**重新设计 driver 抽象或新增 provider（DD-127 已定；SDK transport 仍仅作 fail-closed 校验描述符）。
- **不**改动协调坍缩范围——本 FR 只动执行路径轴，captures/pipeline/CEL 协调退役由 FR-125 承担。
- **不**静默丢弃 `command_rules`（CEL 条件命令，FR-084）语义——若其绑定在 legacy command 路径，需在退役前显式保留或迁移（见需求 4）。

## 需求

### 1. 执行路径迁移清单

- 为每个生产工作流建立 legacy `command:`-only Agent 与 driver Agent 的机器可读清单，标注 provider 归属（AI vs 纯 shell/脚本）与迁移目标 driver。
- 清单区分"demo/模板工作流"与"承载真实执行的工作流"，"零 legacy 消费者"结论可脚本化复现。
- 与 FR-125 ledger 的 `shellRunnerExecutor` 条目交叉引用，共享同一退役前置条件口径。

### 2. AI Agent 迁移到 claude/cli / codex/cli

- 把仍用 legacy `command:` 的 AI Agent（promotion、self-evolution 等混用工作流中的残留）迁移到对应 provider driver。
- 迁移后 legacy/driver 双版本终态对等，事件证据完整（driver 归一化事件入 `events`），provider session 隔离与续接语义不变。

### 3. 纯 shell/脚本 Agent 迁移到 shell/cli

- 把 demo/模板/治理工作流（hello-world、scheduled-scan、fr-watch、streaming-mark-done-convergence）中的纯 shell/脚本 Agent 迁移到 `shell/cli` driver，保持命令与输出契约不变。
- 证明迁移前后行为对等；沙箱、rlimit、进程组行为一致。

### 4. ShellRunnerExecutor legacy 分支退役 与 command_rules 处置

- legacy `command:` 执行路径零生产消费者后，按 ledger `remove` 条件（兼容窗口 + legacy fixture 回归 + 回滚证据 + 门禁绿）移除 `ShellRunnerExecutor` legacy 分支，`RunnerExecutorKind::Shell` legacy 消费点清零。
- 退役前显式处置 `command_rules`：若 CEL 条件命令语义绑定 legacy command 路径，需在 `shell/cli` driver 下保留等价能力或明确记录其去留决策，不得随执行路径退役被静默移除。
- 更新 ledger `shellRunnerExecutor` 状态与 `sourceBaseline` 相关执行侧计数，令基线单调下降。

## 验收标准

- [ ] 每个生产工作流的 legacy command / driver Agent 清单可复现，"零 legacy 消费者"可脚本化验证
- [ ] 所有生产 AI Agent 迁移到 `claude/cli` 或 `codex/cli`，legacy/driver 终态对等且事件证据完整
- [ ] 所有生产纯 shell/脚本 Agent 迁移到 `shell/cli`，行为、沙箱与输出契约对等
- [ ] legacy `command:` 执行路径生产消费者归零；执行侧冻结棘轮拒绝新增 legacy command-only Agent
- [ ] `ShellRunnerExecutor` legacy 分支在零消费者 + 兼容窗口 + 回滚证据后移除，或明确记录阻塞项
- [ ] `command_rules` 语义被显式保留或其去留被书面决策，未随退役静默丢失
- [ ] FR-125 ledger `shellRunnerExecutor` 由 `frozen` 推进到 `remove`，执行侧基线单调下降
- [ ] `cargo test --workspace`、strict Clippy、协调 strangler 门禁、边界层覆盖率治理全部通过

## QA 计划

- 迁移清单脚本 fixture：注入一个 legacy command-only Agent 应使"零消费者"断言失败，迁移后恢复。
- AI Agent 对等测试：迁移前后 legacy/driver 双 task 终态与事件证据逐一比对。
- shell/cli 行为对等测试：demo/模板工作流命令输出、退出码、沙箱拒绝行为迁移前后一致。
- ShellRunnerExecutor 移除回归：legacy fixture 证明移除前后行为、移除后 legacy 分支不可达、公共 spawn 路径不受影响。
- 全量回归：复用 `scripts/qa/test-coordination-strangler.sh` 与 driver 回归，证明 7 条工作流对等与生存机制不受执行路径迁移影响。

## 风险与缓解

- **纯 shell Agent 迁移改变语义**：`shell/cli` 保持命令/沙箱/输出契约不变；行为对等测试逐工作流验证，附即回滚。
- **误删公共 spawn 底座**：非目标显式排除；退役仅针对 `RunnerExecutorKind::Shell` legacy 分支，共享 spawn/policy/profile 保留。
- **command_rules 静默丢失**：需求 4 与验收标准显式要求保留或书面决策其去留。
- **AI Agent 迁移引入 provider 差异**：driver 归一化事件 + 终态对等门禁；provider session 隔离语义按 DD-127 保持。
- **迁移半途停滞**：执行侧冻结棘轮阻止新 legacy command 进入；`ShellRunnerExecutor` 保持 `frozen` 直至零消费者，退役可见可回滚。

## 依赖与参考

- `docs/design_doc/orchestrator/127-agent-driver-abstraction.md`
- `docs/design_doc/orchestrator/129-codex-session-resume-conformance.md`
- `docs/design_doc/orchestrator/136-coordination-strangler-completion.md`
- `docs/feature_request/FR-125-legacy-coordination-decommission.md`
- `config/governance/coordination-collapse-ledger.json`（`retirement.shellRunnerExecutor`）
- `crates/orchestrator-config/src/config/agent.rs`（`command` / `driver` / `command_rules`）
- `crates/orchestrator-runner/src/runner/spawn.rs`（`ShellRunnerExecutor` / `RunnerExecutorKind`）
