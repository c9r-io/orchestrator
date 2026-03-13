# FR-034: QA Testing 自引用安全防护

## 状态

Open

## 优先级

P1 — 直接导致 self-bootstrap 任务中断，需人工恢复 daemon

## 背景

### 问题发现

2026-03-13 执行 `follow-logs-callback-execution.md` 测试计划时，Cycle 2 的 `qa_testing` 步骤运行 `53-client-server-architecture.md` 中的 "Daemon Startup and Shutdown" 场景，该场景执行了：

```bash
kill $(cat data/daemon.pid)
```

这直接杀死了正在运行 self-bootstrap 任务的 daemon 进程本身。

### 影响

1. daemon 收到 SIGTERM（09:17:55），所有正在运行的 qa_testing item 被中断（exit=-5）
2. 两个并行的 qa_testing item（`53-client-server-architecture` 和 `05-workflow-execution`）同时失败
3. 自动生成了 4 张误报 ticket
4. task 进入 paused 状态，需要人工恢复 daemon 并 resume
5. 恢复后进入 ticket_fix 循环，浪费了大量计算资源

### 根因分析

#### 1. QA 文档中包含破坏性操作

`docs/qa/orchestrator/53-client-server-architecture.md` 的 Scenario 1 要求：
- 启动 daemon → 验证 → 停止 daemon → 验证

这在独立测试时是合理的，但在 self-bootstrap 流程中，daemon 就是执行任务的宿主进程。杀死 daemon 等于自杀。

#### 2. Execution Profile 未隔离危险操作

当前 `sandbox_write` profile 限制了文件写入范围，但没有限制进程操作。qa_testing agent 可以自由执行 `kill`、`pkill` 等影响系统进程的命令。

#### 3. 无自引用检测

orchestrator 没有感知到 "当前正在执行的 agent 进程链依赖 daemon 存活" 这一事实。没有机制阻止 agent 杀死自己的宿主进程。

## 需求

### 方案 A：Runner 级进程保护（推荐）

在 runner 层面拦截对 daemon PID 的 kill 操作：

1. **PID 感知**：runner 启动时将当前 daemon PID 注入环境变量 `ORCHESTRATOR_DAEMON_PID`
2. **命令审查**：sandbox 模式下，在执行 shell 命令前检查是否包含 `kill`/`pkill`/`killall` 等操作指向 daemon PID
3. **拒绝执行**：如果检测到对 daemon PID 的终止操作，返回错误而非执行

```rust
// pseudo: runner command pre-check
if sandbox_enabled && command_targets_pid(cmd, daemon_pid) {
    return Err(anyhow!("blocked: cannot kill daemon process (PID {}) from sandbox", daemon_pid));
}
```

### 方案 B：QA 文档标注 + Agent 感知

1. QA 文档中标注 `<!-- self-referential-hazard: daemon-lifecycle -->` 标记
2. `qa_testing` step template 的 prompt 中注入：`当前任务运行在 daemon 上，禁止执行任何停止/重启 daemon 的操作`
3. agent selection 时，对标记了 self-referential-hazard 的 QA 文档降低优先级或跳过

### 方案 C：Execution Profile 增强

在 `ExecutionProfile` 中增加 `deny_signals` 字段：

```yaml
kind: ExecutionProfile
metadata:
  name: sandbox_write
spec:
  sandbox:
    write_paths: [...]
    deny_signals:
      - target: daemon   # 禁止向 daemon 进程发送信号
      - target: self      # 禁止向自身 agent 进程发送信号
```

## 验收标准

1. 在 self-bootstrap 流程中，qa_testing agent 执行包含 `kill $(cat data/daemon.pid)` 的 QA 文档时，操作被拦截而非执行
2. 拦截后 qa_testing 步骤正常退出（exit=0，报告该场景为 skipped/blocked），不产生误报 ticket
3. 非 self-bootstrap 场景下（独立 QA 任务），daemon lifecycle 测试仍可正常执行
4. 拦截事件被记录到 event 表中，方便审计

## 关联

- 发现于：`follow-logs-callback-execution.md` 测试计划，Cycle 2 qa_testing 步骤
- 相关 ticket：`auto_53-client-server-architecture_260313_091805.md`（误报）
- 相关 QA 文档：`docs/qa/orchestrator/53-client-server-architecture.md`
