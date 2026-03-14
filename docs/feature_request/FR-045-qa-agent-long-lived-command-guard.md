# FR-045: QA Agent 长生命周期命令防护

- **优先级**: P1
- **状态**: Proposed
- **来源**: echo-command-test-fixture 执行监控 (2026-03-14)

---

## 1. 问题描述

QA agent 在执行 `docs/qa/orchestrator/65-grpc-control-plane-protection.md` Scenario 3（TaskWatch 流限制）时，通过 Bash 工具启动了两个 `orchestrator task watch` 进程并管道到 `tee`。`task watch` 是长生命周期流式命令，不会主动退出，导致：

1. Bash 工具调用永远不返回（等待子进程 EOF）
2. QA agent 完全阻塞，无法继续执行后续场景
3. 整个 pipeline 在 qa_testing 步骤停滞 30+ 分钟
4. 下游步骤（ticket_fix、align_tests、doc_governance、loop_guard）全部无法执行

### 进程证据

```
PID 43615: orchestrator task watch 378b8995... --interval 1   (运行 25+ 分钟)
PID 43619: orchestrator task watch 378b8995... --interval 1
PID 43620: tee second-watch.log
```

日志文件在 91 行处冻结，25 分钟内无任何增长。

## 2. 需求

### 2.1 task watch 超时支持

为 `orchestrator task watch` 添加 `--timeout <seconds>` 参数：

```bash
orchestrator task watch <task_id> --interval 1 --timeout 30
```

到达超时后，watch 正常退出（exit 0），输出最终状态快照。

### 2.2 QA agent Bash 命令超时

在 qa_testing 步骤模板中，为 agent 的 Bash 工具设置默认超时：

- 通过 step template 的 `env` 或 prompt 指导 QA agent 在所有 Bash 调用中使用 `timeout` 命令包装长生命周期命令
- 或在 runner 层对 qa_testing 步骤的 agent 进程设置整体超时

### 2.3 Stall 检测强化

当前 daemon 已有 `low_output` 检测（450s 心跳静默），但仅发出 WARNING 级别 anomaly，不触发干预。建议：

1. `low_output` 持续超过阈值（如 900s）时，自动终止步骤并标记为超时失败
2. 在 `task trace` 中显示 stall 持续时间

## 3. 验收标准

1. `orchestrator task watch --timeout 30` 在 30 秒后正常退出。
2. QA agent 执行 65-grpc-control-plane-protection Scenario 3 时，watch 命令在超时后退出，agent 继续执行后续场景。
3. 当 qa_testing 步骤超过 stall 阈值时，daemon 自动终止该步骤并记录超时 anomaly。

## 4. 影响范围

- `crates/cli/src/commands/watch.rs` — 添加 `--timeout` 参数
- `docs/workflow/self-bootstrap.yaml` — qa_testing 步骤 prompt 增加超时指导
- `core/src/scheduler.rs` 或 runner — stall 自动终止逻辑
- `docs/qa/orchestrator/65-grpc-control-plane-protection.md` — Scenario 3 步骤使用 `--timeout`

## 5. 风险

- 超时值过短可能导致合法的长时间 watch 被误杀
- 自动终止 stalled 步骤可能在网络延迟场景下误判
