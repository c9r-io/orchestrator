# 全量 QA 回归测试执行计划

本文档用于对当前代码库进行**单次迭代、全量 QA 回归测试**，不涉及代码改动。
适用场景：大批量重构后、发布前、或定期回归验证。

---

## 1. 任务目标

> 课题名称：`全量 QA 回归测试`
>
> 背景：
> 需要对当前代码库的所有 QA 文档（docs/qa/orchestrator/ + docs/qa/self-bootstrap/）
> 进行全面的场景级回归测试，确认各功能点均正常工作。
>
> 本轮任务目标：
> 遍历全部 QA 文档，逐一执行场景验证，对失败项创建 ticket，
> 由 ticket_fix 尝试修复，最后执行 align_tests 和 doc_governance 收口。
>
> 约束：
> 1. 本轮不做主动代码改动，仅在 ticket_fix 中修复 QA 发现的问题。
> 2. 保留所有现有行为不变。
> 3. 最终目标：所有 QA 场景通过或明确记录未通过原因。

### 1.1 预期产出

1. 全部 QA 场景的执行结果（pass/fail/skipped）。
2. 失败场景对应的 ticket（docs/ticket/）。
3. ticket_fix 对可修复项的自动修复。
4. align_tests 确保单测与代码一致。
5. doc_governance 确保文档无漂移。

### 1.2 执行链路

```text
qa_testing(item) → ticket_fix(item) → align_tests(task) → doc_governance(task) → loop_guard
```

单 cycle，无 plan/implement/self_test/self_restart。

---

## 2. 安全机制

### 2.1 双层安全防护

full-qa workflow 通过**双层标记**确保不会执行危险操作（kill daemon、重启进程、重编译二进制）：

**第一层 — YAML Workflow prehook（CEL 表达式）**

`full-qa.yaml` 的 `qa_testing` 步骤 prehook：
```yaml
prehook:
  engine: cel
  when: >-
    qa_file_path.startsWith("docs/qa/")
    && qa_file_path.endsWith(".md")
    && self_referential_safe
```

**第二层 — QA 文档 frontmatter 标记**

危险 QA 文档在文件头部声明：
```yaml
---
self_referential_safe: false
---
```

当 workspace 设置 `self_referential: true` 时，系统读取 QA 文档的 frontmatter，
`self_referential_safe: false` 的文档会被 prehook 跳过，不会被 agent 执行。

### 2.2 标记为不安全的文档（86 个）

以下文档包含 kill daemon、重启进程、重编译二进制、创建任务、修改资源等危险或干扰操作，
已标记为 `self_referential_safe: false`：

#### docs/qa/orchestrator/（77 个，含写操作类 41 个）

| 文件 | 危险操作 |
|------|---------|
| `00-command-contract.md` | 文档 `kill $(cat data/daemon.pid)` 为 daemon 停止命令 |
| `05-workflow-execution.md` | 涉及 daemon 交互的工作流执行 |
| `07-capability-orchestration.md` | 涉及 daemon 交互的能力编排 |
| `14-config-validation-enhanced.md` | `cargo build --workspace` 重编译 |
| `20-structured-output-worker-scheduler.md` | `kill "${DAEMON_PID}"` 清理步骤 |
| `22-performance-io-queue-optimizations.md` | `kill "${DAEMON_PID}"` 清理步骤 |
| `26-self-bootstrap-workflow.md` | `cargo build --release` via skill prerequisites |
| `36-structured-logging.md` | `cargo build --release -p orchestratord` 重编译 daemon |
| `43-cli-force-gate-audit.md` | `--force` 破坏性操作测试 |
| `45-cli-unsafe-mode.md` | `--unsafe` 模式跳过安全检查 |
| `51-primitive-composition.md` | `cargo build --release` 重编译 |
| `53-client-server-architecture.md` | SIGTERM 关闭 daemon、PID 文件操作 |
| `56-sandbox-resource-network-enforcement.md` | `kill "$(cat data/daemon.pid)"` + 启动新 daemon |
| `57-sandbox-resource-limits-extended.md` | `kill "$(cat data/daemon.pid)"` + 启动新 daemon |
| `58-control-plane-security.md` | `kill "$DAEMON_PID"` ×3, `cargo build --release` |
| `59-dynamic-dag-mainline-execution.md` | `cargo build --release` 重编译 |
| `60-daemon-lifecycle-runtime-metrics.md` | SIGTERM 关闭、daemon PID 操作 |
| `65-grpc-control-plane-protection.md` | `kill "$DAEMON_PID"` ×4, `cargo build --release` |
| `71-automate-protoc-dependency.md` | `cargo build --workspace` 重编译 |
| `85-daemon-crash-resilience.md` | `kill -9` SIGKILL 崩溃模拟 |
| `86-orphaned-running-items-recovery.md` | `kill -9` SIGKILL + `pkill orchestratord` |
| `87-self-referential-daemon-pid-guard.md` | 测试 `kill $(cat data/daemon.pid)` 阻断 |
| `91-daemon-crash-resilience.md` | Worker panic 恢复测试 |
| `91b-daemon-crash-resilience-shutdown.md` | SIGTERM/SIGINT 关闭回归 |
| `92-dynamic-items-cycle-overflow.md` | `cargo build --release` 重编译 |
| `93-inflight-step-completion-race.md` | `cargo build --release` 重编译 |
| `94-trigger-resource-cron-event-driven.md` | `cargo build --release` 重编译 |
| `94b-trigger-resource-advanced.md` | `cargo build --release` 重编译 |
| `96-self-restart-socket-continuity.md` | `exec()` 自替换测试 |
| `100-agent-subprocess-daemon-pid-guard.md` | Agent subprocess kill guard 测试 |
| `101-core-crate-split-config.md` | `cargo build --workspace` 重编译 |
| `102-core-crate-split-scheduler.md` | `cargo build --workspace` 重编译 |
| `111-daemon-proper-daemonize.md` | `orchestrator daemon stop` 发送 SIGTERM 停止 daemon |
| `smoke-orchestrator.md` | `cargo build --release` 重编译 |

#### docs/qa/self-bootstrap/（9 个）

| 文件 | 危险操作 |
|------|---------|
| `01-survival-binary-checkpoint-self-test.md` | 集成测试触发 `exec()` 自替换 |
| `02-survival-enforcement-watchdog.md` | watchdog 脚本、`kill "$WATCHDOG_PID"` |
| `04-cycle2-validation-and-runtime-timestamps.md` | `cargo build --release -p orchestratord` + self_restart 任务 |
| `07-self-restart-process-continuity.md` | `exec()` 自替换、`cargo build --release` |
| `08-build-version-hash.md` | `cargo build --release` 验证 hash |
| `scenario2-binary-rollback.md` | 创建任务触发 self_restart |
| `scenario3-binary-skip-disabled.md` | 创建任务触发 self_restart |
| `10-self-referential-safety-policy-alignment.md` | `orchestrator apply` 修改资源 |
| `scenario4-self-test-pass.md` | `orchestrator task create` 创建任务 |

### 2.3 安全 QA 文档（约 52 个）

未标记的文档默认 `self_referential_safe: true`，包括：
- 纯单元测试文档（`cargo test --lib`）
- CLI 命令验证（`orchestrator get/apply/check` 等只读操作）
- 数据库查询验证（`sqlite3` 查询）
- 配置验证文档
- 文档格式/结构验证

---

## 3. 执行步骤

### 3.1 构建并确认 daemon 运行

```bash
cd /Volumes/Yotta/c9r-io/orchestrator

# 确认 daemon 运行
ps aux | grep orchestratord | grep -v grep

# 如未运行：
# nohup ./target/release/orchestratord --foreground --workers 4 > /tmp/orchestratord.log 2>&1 &
```

### 3.2 加载 full-qa workflow 资源

```bash
# 清理旧项目（如需要重新开始）
# orchestrator delete project/full-qa --force

# 初始化
orchestrator init

# 加载 secrets 和 execution profiles
orchestrator apply -f docs/workflow/claude-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/minimax-secret.yaml --project self-bootstrap
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap

# 加载 self-bootstrap 的 StepTemplates（full-qa 复用这些模板）
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap

# 加载 full-qa workflow
orchestrator apply -f docs/workflow/full-qa.yaml --project self-bootstrap
```

### 3.3 创建任务（全量扫描）

```bash
orchestrator task create \
  -n "full-qa-regression" \
  -w full-qa -W full-qa \
  --project self-bootstrap \
  -g "对 docs/qa/ 下全部 QA 文档执行场景级回归测试，对失败项创建 ticket 并尝试修复，最终确保所有场景通过或明确记录未通过原因"
```

> 不指定 `-t`，系统自动扫描 `qa_targets` 配置的 `docs/qa/` 下所有 `.md` 文件。
> 预计约 138 个 item，其中约 86 个会被 prehook 自动跳过（`self_referential_safe: false`），
> 实际执行约 52 个。

记录返回的 `<task_id>`。

---

## 4. 监控方法

### 4.1 状态监控

```bash
orchestrator task list
orchestrator task info <task_id>
orchestrator task trace <task_id>
orchestrator task watch <task_id>
```

重点观察：

1. item 执行进度（已完成 / 总数）
2. qa_testing 步骤的 pass/fail/skipped 分布
3. ticket_fix 是否正在处理活跃 ticket
4. 是否有 item 长时间卡住
5. prehook 跳过的不安全文档数量是否为 39

### 4.2 日志监控

```bash
orchestrator task logs --tail 200 <task_id>
```

重点观察：

1. 各 QA 文档的执行结果
2. ticket 创建和修复情况
3. prehook 跳过的自引用不安全文档（应看到 `step_skipped` 事件）

### 4.3 进程监控

```bash
# agent 子进程
ps aux | grep "claude -p" | grep -v grep | wc -l

# 预期最多 4 个并行（max_parallel: 4）
```

### 4.4 中间检查

当 item segment 完成约 50% 时，可检查：

```bash
# 查看已创建的 ticket
ls docs/ticket/

# 查看 ticket 数量
ls docs/ticket/*.md 2>/dev/null | wc -l

# 验证不安全文档被跳过
sqlite3 data/agent_orchestrator.db \
  "SELECT COUNT(*) FROM events WHERE task_id = '<task_id>' AND event_type = 'step_skipped' AND json_extract(payload_json, '$.step') = 'qa_testing';"
```

---

## 5. 关键检查点

### 5.1 安全检查点

- [ ] `full-qa.yaml` workspace 的 `self_referential: true` 已生效
- [ ] 86 个不安全 QA 文档被 prehook 跳过（`step_skipped` 事件）
- [ ] daemon 进程在整个执行过程中保持稳定（PID 不变）
- [ ] 无 `cargo build --release -p orchestratord` 被执行

### 5.2 QA Testing 阶段

- [ ] 所有安全 QA 文档都被执行（约 52 个）
- [ ] 每个场景的 pass/fail 有明确结论
- [ ] 失败场景有对应的 ticket 文件

### 5.3 Ticket Fix 阶段

- [ ] 活跃 ticket 被尝试修复
- [ ] 修复后重新验证场景通过
- [ ] 无法修复的 ticket 保留并记录原因

### 5.4 Align Tests 阶段

- [ ] cargo test 全部通过
- [ ] cargo clippy 无警告
- [ ] 编译无警告

### 5.5 Doc Governance 阶段

- [ ] QA 文档无格式漂移
- [ ] README/manifest 一致性

---

## 6. 成功判定

当以下条件同时成立，可判定本轮全量 QA 完成：

1. orchestrator 完整跑完 `full-qa` workflow，在 `loop_guard` 正常收口。
2. 安全 QA 场景通过率 ≥ 90%（允许部分环境依赖的场景失败）。
3. 86 个不安全文档全部被正确跳过。
4. 所有 ticket 被 ticket_fix 处理（修复或明确标记无法修复）。
5. `align_tests` 确认单测和编译无回归。
6. `doc_governance` 确认文档无漂移。

---

## 7. 异常处理

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| 不安全文档未被跳过 | `step_skipped` 数量 < 86 | 检查 workspace `self_referential` 设置、QA 文档 frontmatter |
| 大量 QA 文档同类失败 | 相同 pattern 的 ticket 超过 10 个 | 可能是系统性问题，暂停排查根因 |
| agent 进程僵死 | `claude -p` 进程无输出超过 10 分钟 | 检查 API 配额和网络 |
| ticket_fix 产生新问题 | 修复后 align_tests 失败 | 检查 ticket_fix 的改动范围 |
| daemon 内存过高 | item 并发导致内存压力 | 降低 max_parallel 到 2 |
| daemon 被意外 kill | PID 变化或连接断开 | 有不安全文档绕过了 prehook；立即中止任务 |

---

## 8. 预计执行时间

- **约 52 个安全 QA 文档** × **每个约 2-5 分钟** = 约 50-130 分钟（4 并行）
- 86 个不安全文档被跳过（< 1 秒）
- ticket_fix 取决于 ticket 数量
- align_tests + doc_governance 约 10-20 分钟

总计预估：**1 - 2.5 小时**

---

## 9. 人工角色边界

本计划中，人工角色限定为：

1. 启动 workflow
2. 监控执行进度
3. 验证安全文档被正确跳过
4. 在系统性异常时中断
5. 记录最终结果

不人工干预具体 QA 场景的执行和 ticket 修复。
