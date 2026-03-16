# Agent Disease 错误地将任务否定结论计入 agent 健康惩罚

- **Observed during**: full-qa-execution plan, qa_testing step, all cycles
- **Severity**: critical
- **Symptom**: full-qa workflow 在执行约 27/134 个 QA 文档后，所有 agent 被标记为 diseased（5 小时冷却），剩余 ~80 个 item 全部返回 "No healthy agent found with capability: qa_testing"，任务失败。
- **Expected**: Agent 正确执行了任务并给出结论（exit_code > 0 = 被测代码有问题），agent 本身工作正常，不应被标记为 diseased。
- **Status**: open

## 根因分析

`crates/orchestrator-scheduler/src/scheduler/phase_runner/record.rs:171-178`：

```rust
if !validated.success {                              // ← exit_code != 0
    let errors = increment_consecutive_errors(state, agent_id).await;
    if errors >= 2 {                                 // ← 连续 2 次就 disease
        mark_agent_diseased(state, agent_id).await;  // ← 5 小时冷却
    }
} else {
    reset_consecutive_errors(state, agent_id).await;
}
```

`validated.success` 的定义（`validate.rs:42`）：

```rust
let mut success = final_exit_code == 0;
```

**语义混淆**：`success` 合并了两种完全不同的含义：
1. **Agent 自身故障**：crash、timeout、API 错误、sandbox 拒绝 — agent 无法完成工作
2. **任务结论为否定**：QA 发现 bug、plan 判定不可行、implement 编译失败、ticket_fix 修复失败 — agent 正确完成了工作，只是结论是"不通过"

当前代码将 #2 等同于 #1，导致所有步骤类型都受影响。

## 影响范围（所有步骤类型）

| 步骤类型 | exit_code > 0 的含义 | 是否应计入 disease |
|---------|---------------------|-------------------|
| qa_testing | QA 发现 bug | **否** — agent 工作正常 |
| plan | 方案不可行 | **否** — agent 给出了有效判断 |
| implement | 代码写了但有问题 | **否** — agent 完成了工作 |
| ticket_fix | 修复失败 | **否** — agent 尝试了修复 |
| doc_governance | 文档有漂移 | **否** — agent 检测到了问题 |

在所有步骤类型中，`exit_code > 0` 均表示"agent 正确完成了工作，但给出否定结论"。

## 已有的鲁棒判据

`ValidatedOutput` 已提供足够的信号区分 agent 故障 vs 任务否定结论：

| 信号 | 含义 | 应计入 disease |
|------|------|---------------|
| `final_exit_code < 0` | 进程被信号杀死或 spawn 失败 | **是** |
| `timed_out == true` | 步骤超时 | **是**（通过 exit_code < 0 隐含） |
| `sandbox_denied == true` | sandbox 拒绝执行 | **是** |
| `validation_status == "failed"` | agent 输出格式不合规 | **是** |
| `final_exit_code > 0` | agent 正常退出，结论为否定 | **否** |
| `final_exit_code == 0` | agent 正常退出，结论为通过 | **否**（成功） |

## 建议修复

在 `record.rs` 中用已有信号构建更精确的 agent 故障判据：

```rust
// Agent infrastructure failure — the agent itself could not function.
// Distinct from "task conclusion is negative" (exit_code > 0) where
// the agent completed its work correctly.
let agent_infra_failed = validated.final_exit_code < 0
    || validated.sandbox_denied
    || validated.validation_status == "failed";

if agent_infra_failed {
    let errors = increment_consecutive_errors(state, agent_id).await;
    if errors >= 2 {
        mark_agent_diseased(state, agent_id).await;
    }
} else {
    reset_consecutive_errors(state, agent_id).await;
}
```

**不需要按步骤类型分支判断**，因为判据完全基于 agent 基础设施信号，对所有步骤类型通用。

## 证据

daemon log（`/tmp/orchestratord.log`）：
```
2026-03-16T04:15:34 ERROR task failed worker=3 task_id=e16ed4d5
  error=parallel item execution failed: No healthy agent found with capability: qa_testing (×79)
```

任务 `e16ed4d5` 的 item 状态：qa_passed=20, unresolved=7, skipped=2, 其余全部卡在 "No healthy agent"。
