---
self_referential_safe: false
---

# Agent Drain 与 Enabled 开关

**Scope**: 验证 FR-017 agent enabled 开关、drain 生命周期状态机、selection.rs 过滤、phase_runner in_flight 计数、drain_timeout_sweep 以及 CLI/gRPC 接口。

## Scenarios

1. 验证 `enabled: false` agent 不参与调度：

   ```bash
   cargo test -p agent-orchestrator scheduler::selection::tests::disabled_agent_excluded_from_selection -- --nocapture
   ```

   Expected:

   - manifest `enabled: false` 的 agent 在 selection 候选集中不出现
   - task 启动时不为该 agent 创建运行态记录
   - 其余 `enabled: true` 的 agent 正常参与调度

2. 验证 cordon/uncordon 状态转换：

   ```bash
   cargo test -p agent-orchestrator scheduler::agent_lifecycle::tests::cordon_prevents_new_item_dispatch -- --nocapture
   cargo test -p agent-orchestrator scheduler::agent_lifecycle::tests::uncordon_restores_active_state -- --nocapture
   ```

   Expected:

   - cordon 后 agent 状态变为 Cordoned，`increment_in_flight` 拒绝新 item
   - cordon 不中断正在执行的 item（in_flight 计数不变）
   - uncordon 后状态恢复 Active，重新参与 selection

3. 验证 drain 流程与 in_flight 驱动的状态转换：

   ```bash
   cargo test -p agent-orchestrator scheduler::phase_runner::tests::drain_transitions_to_drained_when_inflight_zero -- --nocapture
   cargo test -p agent-orchestrator scheduler::phase_runner::tests::draining_agent_completes_inflight_items -- --nocapture
   ```

   Expected:

   - drain 后 agent 状态变为 Draining，不再接受新 item
   - 正在执行的 item 继续运行至完成
   - 最后一个 in_flight item 完成后状态自动转为 Drained
   - `AgentStateChanged` 事件在每次状态转换时发出

4. 验证 drain_timeout_sweep 超时强制排空：

   ```bash
   cargo test -p agent-orchestrator scheduler::drain_timeout_sweep::tests::timeout_forces_drained_state -- --nocapture
   ```

   Expected:

   - 处于 Draining 状态超过 `drain_timeout` 的 agent 被强制标记为 Drained
   - `AgentDrainTimedOut` 事件被发出并记录警告日志
   - 未超时的 Draining agent 不受影响

5. 验证最后一个 Active agent 被 drain 时的告警：

   ```bash
   cargo test -p agent-orchestrator scheduler::agent_lifecycle::tests::last_active_agent_drain_emits_warning -- --nocapture
   ```

   Expected:

   - 最后一个 Active agent 被 drain 时日志中出现可操作警告
   - task 调度循环记录无 Active agent 状态，不崩溃

6. 验证 CLI `agent list / cordon / uncordon / drain` 命令：

   ```bash
   cargo test -p agent-orchestrator cli::agent::tests -- --nocapture
   ```

   Expected:

   - `agent list` 输出包含 name、enabled、lifecycle_state、in_flight_count 列
   - `agent cordon` / `uncordon` / `drain` 正确调用对应 gRPC RPC
   - `--timeout` 参数被正确传递到 `AgentDrainRequest`

7. 验证 gRPC AgentList / AgentCordon / AgentUncordon / AgentDrain RPC：

   ```bash
   cargo test -p orchestratord server::agent::tests -- --nocapture
   ```

   Expected:

   - `AgentList` 返回正确的 agent 状态列表
   - `AgentCordon` / `AgentUncordon` / `AgentDrain` 正确修改运行态状态
   - 无效 task_id 或 agent_name 返回明确错误，不 panic

8. 工作区回归验证：

   ```bash
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all --check
   ```

   Expected:

   - 全工作区测试通过，无现有测试回归
   - clippy 无新增 warning
   - 格式检查通过

## Failure Notes

- 若 selection 过滤失效，检查 `core/src/scheduler/selection.rs` 的 lifecycle_state 判断分支
- 若 in_flight 计数驱动的状态转换失效，检查 `core/src/scheduler/phase_runner/record.rs` 的 `decrement_in_flight` 实现
- 若 drain_timeout_sweep 不触发，检查 `core/src/scheduler/drain_timeout_sweep.rs` 的 sweep 间隔配置
- 若 gRPC RPC 响应异常，检查 `crates/daemon/src/server/agent.rs` 和对应的 proto 定义
- 若 CLI 命令参数解析失败，检查 `crates/cli/src/agent.rs` 的 clap 结构体定义

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | `enabled: false` agent 不参与调度 | ☐ | |
| 2 | cordon/uncordon 状态转换 | ☐ | |
| 3 | drain 流程与 in_flight 驱动的状态转换 | ☐ | |
| 4 | drain_timeout_sweep 超时强制排空 | ☐ | |
| 5 | 最后一个 Active agent drain 告警 | ☐ | |
| 6 | CLI agent list/cordon/uncordon/drain | ☐ | |
| 7 | gRPC AgentList/AgentCordon/AgentUncordon/AgentDrain | ☐ | |
| 8 | 工作区回归验证 | ☐ | |
