---
self_referential_safe: true
---

# Agent Drain 与 Enabled 开关

**Scope**: 验证 FR-017 agent enabled 开关、drain 生命周期状态机、selection.rs 过滤、phase_runner in_flight 计数、drain_timeout_sweep 以及 CLI/gRPC 接口。

## Scenarios

1. 验证 `enabled: false` agent 不参与调度：

   ```bash
   cargo test -p agent-orchestrator -- selection::tests::test_diseased_agent_filtered_from_candidates --nocapture
   cargo test -p agent-orchestrator -- selection::tests::test_select_agent_advanced_excludes_agents --nocapture
   ```

   Expected:

   - manifest `enabled: false` 的 agent 在 selection 候选集中不出现
   - task 启动时不为该 agent 创建运行态记录
   - 其余 `enabled: true` 的 agent 正常参与调度

2. 验证 cordon/uncordon 状态转换：

   ```bash
   cargo test -p agent-orchestrator -- agent_lifecycle::tests::cordon_active_agent_succeeds --nocapture
   cargo test -p agent-orchestrator -- agent_lifecycle::tests::cordon_already_cordoned_fails --nocapture
   cargo test -p agent-orchestrator -- agent_lifecycle::tests::uncordon_cordoned_agent_succeeds --nocapture
   ```

   Expected:

   - cordon 后 agent 状态变为 Cordoned，`increment_in_flight` 拒绝新 item
   - cordon 不中断正在执行的 item（in_flight 计数不变）
   - uncordon 后状态恢复 Active，重新参与 selection

3. 验证 drain 流程与 in_flight 驱动的状态转换：

   ```bash
   cargo test -p agent-orchestrator -- agent_lifecycle::tests::drain_with_no_inflight_goes_directly_to_drained --nocapture
   cargo test -p agent-orchestrator -- agent_lifecycle::tests::drain_with_inflight_goes_to_draining --nocapture
   cargo test -p agent-orchestrator -- agent_lifecycle::tests::decrement_inflight_completes_drain --nocapture
   ```

   Expected:

   - drain 后 agent 状态变为 Draining，不再接受新 item
   - 正在执行的 item 继续运行至完成
   - 最后一个 in_flight item 完成后状态自动转为 Drained
   - `AgentStateChanged` 事件在每次状态转换时发出

4. 验证 drain_timeout_sweep 超时强制排空：

   ```bash
   cargo test -p agent-orchestrator -- agent_lifecycle::tests::drain_timeout_sweep_forces_drained --nocapture
   ```

   Expected:

   - 处于 Draining 状态超过 `drain_timeout` 的 agent 被强制标记为 Drained
   - `AgentDrainTimedOut` 事件被发出并记录警告日志
   - 未超时的 Draining agent 不受影响

5. 验证 agent selection 排除不健康 agent：

   **Code review** — 确认 `core/src/selection.rs` 中 `select_agent_advanced` 通过 `is_schedulable()` 抽象过滤 agent：

   ```bash
   rg -n "is_schedulable" core/src/selection.rs
   rg -n "fn is_schedulable" core/src/metrics.rs
   ```

   Expected:

   - `selection.rs` 使用 `lifecycle_map.get(*id).map(|s| s.lifecycle.is_schedulable())` 过滤
   - `is_schedulable()` 仅对 `Active` 状态返回 `true`
   - Cordoned/Draining/Drained 状态的 agent 不参与 selection

6. 验证 CLI agent 子命令解析：

   ```bash
   cargo test -p orchestrator-cli -- cli::tests --nocapture
   ```

   Expected:

   - CLI 子命令定义（agent list/cordon/uncordon/drain）正确解析
   - `--timeout` 参数被正确传递到 drain 请求

7. 验证 gRPC server 模块测试：

   ```bash
   cargo test -p orchestratord -- server::tests --nocapture
   ```

   Expected:

   - gRPC server 测试通过
   - 无效 agent_name 返回明确错误，不 panic

8. 工作区回归验证：

   ```bash
   cargo test --workspace --lib
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo fmt --all --check
   ```

   Expected:

   - 全工作区测试通过，无现有测试回归
   - clippy 无新增 warning
   - 格式检查通过

## Failure Notes

- 若 selection 过滤失效，检查 `core/src/selection.rs` 的 lifecycle_state 判断分支
- 若 in_flight 计数驱动的状态转换失效，检查 `core/src/agent_lifecycle.rs` 的 `decrement_in_flight` 实现
- 若 drain_timeout_sweep 不触发，检查 `core/src/agent_lifecycle.rs` 的 sweep 间隔配置
- 若 gRPC RPC 响应异常，检查 `crates/daemon/src/server/mod.rs` 和对应的 proto 定义
- 若 CLI 命令参数解析失败，检查 `crates/cli/src/cli.rs` 的 clap 结构体定义

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | `enabled: false` agent 不参与调度 | ✅ | Tests: `test_diseased_agent_filtered_from_candidates`, `test_select_agent_advanced_excludes_agents` |
| 2 | cordon/uncordon 状态转换 | ✅ | Tests: `cordon_active_agent_succeeds`, `cordon_already_cordoned_fails`, `uncordon_cordoned_agent_succeeds` |
| 3 | drain 流程与 in_flight 驱动的状态转换 | ✅ | Tests: `drain_with_no_inflight_goes_directly_to_drained`, `drain_with_inflight_goes_to_draining`, `decrement_inflight_completes_drain` |
| 4 | drain_timeout_sweep 超时强制排空 | ✅ | Test: `drain_timeout_sweep_forces_drained` |
| 5 | agent selection 排除不健康 agent | ⚠️ | 过滤逻辑正确（通过`is_schedulable()`抽象），但QA验证命令需更新，见 `docs/ticket/qa-agent-drain-s5-verification-method_260320_203000.md` |
| 6 | CLI agent 子命令解析 | ✅ | 5 CLI tests passed |
| 7 | gRPC server 模块测试 | ✅ | 3 server tests passed |
| 8 | 工作区回归验证 | ✅ | 409 lib tests + clippy + fmt 全部通过 |
