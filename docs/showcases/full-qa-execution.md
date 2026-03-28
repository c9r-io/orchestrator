# 全量 QA 回归测试执行计划

> **Agent 协作**：本文档是一个 Agent 可执行的计划。在 AI 编码 Agent（Claude Code、OpenCode、Codex 等）中打开本项目，Agent 读取本计划后，通过 orchestrator CLI 调度其他 Agent 协作完成任务 — 从资源部署、任务执行到结果验证，全程自主完成。

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
qa_testing(item) → ticket_fix(item) → align_tests(task) → doc_governance(task) → self_test → loop_guard
```

单 cycle，无 plan/implement/self_restart。

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
    && (self_referential_safe || size(self_referential_safe_scenarios) > 0)
```

即：`self_referential_safe: true` 的文档**全量执行**，
有 `self_referential_safe_scenarios` 的文档**部分执行**（仅限列出的场景），
两者均不满足的文档被**完全跳过**。

**第二层 — QA 文档 frontmatter 标记**

危险 QA 文档在文件头部声明：
```yaml
---
self_referential_safe: false
---
```

当 workspace 设置 `self_referential: true` 时，系统读取 QA 文档的 frontmatter，
`self_referential_safe: false`（且无 `self_referential_safe_scenarios`）的文档会被 prehook 跳过，不会被 agent 执行。

### 2.2 标记为不安全的文档（48 个）

以下文档包含 kill daemon、重启进程、重编译二进制、创建任务、修改资源、触发 webhook 等危险或干扰操作，
或需要 GUI 环境/外部服务（在纯 CLI 回归中不可执行），已标记为 `self_referential_safe: false`。

其中 **34 个被完全跳过**（无 `self_referential_safe_scenarios`），
**14 个被部分执行**（仅限列出的安全场景）。

#### docs/qa/orchestrator/（43 个）

**完全跳过（29 个）：**

| 文件 | 危险操作 |
|------|---------|
| `01-cli-agent-orchestration.md` | force delete, task create/start, apply resources |
| `02-cli-task-lifecycle.md` | force delete, task create/start, apply resources |
| `15-workflow-multi-target-files.md` | force delete, task create/start, apply resources |
| `19-scheduler-repository-refactor-regression.md` | force delete, task create/start, apply resources |
| `26-self-bootstrap-workflow.md` | `cargo build --release`, force delete, apply resources |
| `28-self-bootstrap-pipeline.md` | force delete, apply resources |
| `41-project-scoped-agent-selection.md` | force delete, task create/start, apply resources |
| `45-cli-unsafe-mode.md` | force delete, `--unsafe` 模式 |
| `51-primitive-composition.md` | `cargo build --release`, task create/start |
| `55-sandbox-write-boundaries.md` | force delete, task create/start, apply resources |
| `56-sandbox-denial-anomaly-trace.md` | force delete, task create/start, apply resources |
| `56-sandbox-resource-network-enforcement.md` | `cargo build --release`, kill daemon |
| `57-sandbox-resource-limits-extended.md` | `cargo build --release`, kill daemon |
| `58-control-plane-security.md` | `cargo build --release`, kill daemon |
| `60-daemon-lifecycle-runtime-metrics.md` | `cargo build --release`, kill daemon, signal ops |
| `65-grpc-control-plane-protection.md` | `cargo build --release`, kill daemon |
| `84-generate-items-regression-narrowing.md` | force delete, task create/start, apply resources |
| `87-self-referential-daemon-pid-guard.md` | kill daemon |
| `96-self-restart-socket-continuity.md` | `cargo build`, `exec()` 自替换 |
| `100-agent-subprocess-daemon-pid-guard.md` | kill daemon |
| `116-gui-architecture-tauri-grpc.md` | 需要 GUI 环境（Tauri） |
| `117-gui-uiux-wish-pool-progress.md` | 需要 GUI 环境（Tauri） |
| `118-gui-realtime-wish-isolation.md` | 需要 GUI 环境（Tauri） |
| `119-gui-cli-rpc-parity.md` | 需要 GUI 环境（Tauri） |
| `120-gui-connection-resilience.md` | 需要 GUI 环境（Tauri） |
| `120b-gui-notification-error-humanization.md` | 需要 GUI 环境（Tauri） |
| `121-gui-polish-visual.md` | 需要 GUI 环境（Tauri） |
| `121b-gui-i18n-ux.md` | 需要 GUI 环境（Tauri） |
| `smoke-orchestrator.md` | `cargo build --release` |

**部分执行（14 个，仅限列出的安全场景）：**

| 文件 | 安全场景 | 危险操作（跳过的场景） |
|------|---------|----------------------|
| `20-structured-output-worker-scheduler.md` | S1, S2, S3 | kill daemon, task create/start |
| `22-performance-io-queue-optimizations.md` | S1, S2, S3 | kill daemon, task create/start |
| `54-step-execution-profiles.md` | S2, S3 | force delete, task create/start |
| `64-secretstore-key-lifecycle.md` | S5 | apply resources |
| `94b-trigger-resource-advanced.md` | S2 | apply resources |
| `99-long-lived-command-guard.md` | S5 | task create/start |
| `107-parallel-dispatch-completeness-guard.md` | S2, S3, S4 | S1: `delete --force`, `task create`, `cargo build`（自引用危险操作） |
| `124-homebrew-tap-distribution.md` | S1, S3, S4, S5, S6, S7, S8 | S2: 需要已发布 release；S9/S10: 端到端安装需要外部服务 |
| `125-documentation-site.md` | S1 | S2-S5: 需要 dev server + 浏览器/GUI |
| `126-task-items-event-list-cli.md` | S2, S3, S4, S5, S6, S7, S8, S9 | S1: task create（创建任务） |
| `128-webhook-trigger-infrastructure.md` | S1, S4, S5, S6, S7, S9 | S2/S3: apply trigger + 触发 webhook；S8: apply trigger 资源 |
| `129-per-trigger-webhook-auth-cel-filter.md` | S1, S2 | S3/S4/S5: apply SecretStore + Trigger 资源 |
| `129b-per-trigger-webhook-auth-cel-filter-advanced.md` | S7, S8 | S6: apply trigger + 触发 webhook |
| `111-daemon-proper-daemonize.md` | — | kill daemon, signal ops, daemon stop |

> 注：`111-daemon-proper-daemonize.md` 标记为 false 且无 scenarios，归入完全跳过。
> 上表为便于对照将其列在此处，实际完全跳过数为 30 个 orchestrator 文档。
> `126`、`128`、`129`、`129b` 为 2026-03-29 全量 QA 回归中发现的安全漏洞补充标记。

#### docs/qa/self-bootstrap/（5 个）

**完全跳过（4 个）：**

| 文件 | 危险操作 |
|------|---------|
| `01-survival-binary-checkpoint-self-test.md` | `cargo build --release`, `exec()` 自替换 |
| `04-cycle2-validation-and-runtime-timestamps.md` | `cargo build --release`, `exec()` 自替换 |
| `07-self-restart-process-continuity.md` | `cargo build --release`, `exec()` 自替换 |
| `smoke-self-bootstrap.md` | smoke 测试（含 daemon 交互） |

**部分执行（1 个）：**

| 文件 | 安全场景 | 危险操作（跳过的场景） |
|------|---------|----------------------|
| `02-survival-enforcement-watchdog.md` | S1, S2, S3 | kill daemon, signal ops, file deletion |

### 2.3 安全 QA 文档（约 116 个）

| 类别 | 数量 | 说明 |
|------|------|------|
| 显式 `self_referential_safe: true` | 86 | 完全执行 |
| 无 frontmatter 标记（默认 safe） | 16 | 完全执行 |
| `false` + 有 `scenarios` | 14 | 部分执行（仅安全场景） |
| **合计可执行** | **116** | |

可执行文档包括：
- 纯单元测试文档（`cargo test --lib`）
- CLI 命令验证（`orchestrator get/apply/check` 等只读操作）
- 数据库查询验证（`orchestrator event list` / `orchestrator db status` 等）
- 配置验证文档
- 文档格式/结构验证

> **注意**：有 `self_referential_safe_scenarios` 的文档会被**部分执行**（仅限列出的场景），
> 在指标统计中，这类文档计为"已执行"。

---

## 3. 执行步骤

### 3.1 构建并确认 daemon 运行

```bash
cd "$ORCHESTRATOR_ROOT"   # your orchestrator project directory

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
orchestrator apply -f your-secrets.yaml           --project self-bootstrap
# apply additional secret manifests as needed      --project self-bootstrap
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
> 预计约 171 个 item，其中约 34 个会被 prehook 完全跳过（`self_referential_safe: false` 且无 scenarios），
> 约 14 个部分执行（仅安全场景），实际全量执行约 123 个。

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
5. prehook 跳过的不安全文档数量是否符合预期

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

# 预期最多 4 个并行（workflow max_parallel: 4；ticket_fix 步骤 max_parallel: 2）
```

### 4.4 中间检查

当 item segment 完成约 50% 时，可检查：

```bash
# 查看已创建的 ticket
ls docs/ticket/

# 查看 ticket 数量
ls docs/ticket/*.md 2>/dev/null | wc -l

# 验证不安全文档被跳过（从 JSON 输出中统计 step_skipped 事件数量）
orchestrator event list --task <task_id> --type step_skipped -o json
```

---

## 5. 关键检查点

### 5.1 安全检查点

- [ ] `full-qa.yaml` workspace 的 `self_referential: true` 已生效
- [ ] 34 个完全不安全的 QA 文档被 prehook 跳过（`step_skipped` 事件）
- [ ] 14 个部分安全文档仅执行了指定场景
- [ ] daemon 进程在整个执行过程中保持稳定（PID 不变）
- [ ] 无 `cargo build --release -p orchestratord` 被执行

### 5.2 QA Testing 阶段

- [ ] 所有安全 QA 文档都被执行（约 116 个）
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

### 5.6 Self Test 阶段

- [ ] `cargo test` 编译通过
- [ ] 单测无回归

---

## 6. 成功判定

当以下条件同时成立，可判定本轮全量 QA 完成：

1. orchestrator 完整跑完 `full-qa` workflow，在 `loop_guard` 正常收口。
2. 安全 QA 场景通过率 ≥ 90%（允许部分环境依赖的场景失败）。
3. 34 个完全不安全文档全部被正确跳过，14 个部分安全文档仅执行了安全场景。
4. 所有 ticket 被 ticket_fix 处理（修复或明确标记无法修复）。
5. `align_tests` 确认单测和编译无回归。
6. `doc_governance` 确认文档无漂移。
7. `self_test` 确认编译和测试通过。

---

## 7. 异常处理

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| 不安全文档未被跳过 | `step_skipped` 数量 < 34（完全跳过）或部分执行文档执行了不安全场景 | 检查 workspace `self_referential` 设置、QA 文档 frontmatter |
| 大量 QA 文档同类失败 | 相同 pattern 的 ticket 超过 10 个 | 可能是系统性问题，暂停排查根因 |
| agent 进程僵死 | `claude -p` 进程无输出超过 10 分钟 | 检查 API 配额和网络 |
| ticket_fix 产生新问题 | 修复后 align_tests 失败 | 检查 ticket_fix 的改动范围 |
| daemon 内存过高 | item 并发导致内存压力 | 降低 max_parallel 到 2 |
| daemon 被意外 kill | PID 变化或连接断开 | 有不安全文档绕过了 prehook；立即中止任务 |

---

## 8. 预计执行时间

- **约 109 个全量执行 + 7 个部分执行** × **每个约 2-5 分钟** = 约 60-290 分钟（4 并行）
- 34 个不安全文档被跳过（< 1 秒）
- ticket_fix 取决于 ticket 数量（max_parallel: 2）
- align_tests + doc_governance + self_test 约 10-20 分钟

总计预估：**1.5 - 6 小时**

---

## 9. 人工角色边界

本计划中，人工角色限定为：

1. 启动 workflow
2. 监控执行进度
3. 验证安全文档被正确跳过
4. 在系统性异常时中断
5. 记录最终结果

不人工干预具体 QA 场景的执行和 ticket 修复。
