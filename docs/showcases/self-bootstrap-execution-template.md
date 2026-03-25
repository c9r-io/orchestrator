# self-bootstrap 课题执行计划模板

本文档是通用模板，用于把某个课题直接交给 orchestrator 的 `self-bootstrap` workflow 执行。使用方式是复制本文件，替换占位符，把课题目标传给 orchestrator；人工只负责启动、监控、记录，并在异常时介入。

建议参考历史实例：

1. [`docs/showcases/resource-rs-refactor-execution.md`](resource-rs-refactor-execution.md)（如该文件仍保留）
2. [`docs/showcases/self-bootstrap-builtin-self-test-workaround-execution.md`](self-bootstrap-builtin-self-test-workaround-execution.md)

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`<课题标题>`
>
> 背景：
> `<简要描述当前问题、技术债、缺陷或待优化点>`
>
> 本轮任务目标：
> `<说明希望 orchestrator 完成的结果>`
>
> 约束：
> 1. 优先解决根因，不接受仅做表面绕过。
> 2. 保留已有核心语义、兼容性要求、关键事件或状态行为：`<需要保留的行为>`
> 3. 最终目标是：`<明确的完成态>`

### 1.1 预期产出

由 orchestrator 自主产出并落地：

1. 一份实现计划（由 `plan` 步骤生成）。
2. 必要的 QA 文档更新（由 `qa_doc_gen` 判断是否生成/更新）。
3. 与课题目标对应的代码或配置改动。
4. 自举回归验证结果。
5. 若本轮发现问题，由 `ticket_fix` 和后续步骤尝试收口。

### 1.2 非目标

本次不由人工预先定义实现细节；不在计划文档中替 orchestrator 指定具体代码改法。实现路径由 workflow 自主决定，人工只观察其是否偏离目标。

---

## 2. 执行方式

本轮按 `self-bootstrap` 的标准链路执行，不做人肉拆任务：

```text
plan -> qa_doc_gen -> implement -> self_test -> self_restart -> qa_testing -> ticket_fix -> align_tests -> doc_governance -> loop_guard
```

> **`self_restart` step**: After `self_test` passes, `self_restart` rebuilds the release binary
> (`cargo build --release -p orchestratord`), verifies it (`--help`), snapshots `.stable`, sets the task to
> `restart_pending`, and signals a restart via `RestartRequestedError`. The daemon drains workers
> (30s timeout), then uses `exec()` to replace itself with the new binary in-place (preserving PID).
> The new process auto-resumes the `restart_pending` task and continues into Cycle 2.
> This step has `repeatable: false`, so it only runs in Cycle 1.
> Build failure is non-fatal (`on_failure: continue`) — the loop continues with the old binary.

人工职责只有两类：

1. 启动和提供课题目标。
2. 监控执行状态、判断是否卡住、记录结果。

---

## 3. 启动步骤

### 3.1 构建并启动 daemon

C/S 架构下，CLI（`orchestrator`）通过 Unix Domain Socket 连接 daemon（`orchestratord`）。

```bash
cd "$ORCHESTRATOR_ROOT"   # your orchestrator project directory

cargo build --release -p orchestratord -p orchestrator-cli

# 启动 daemon（如未运行）
# --foreground 保持前台输出日志；--workers 指定并行 worker 数
nohup ./target/release/orchestratord --foreground --workers 2 > /tmp/orchestratord.log 2>&1 &

# 验证 daemon 运行
ps aux | grep orchestratord | grep -v grep
# 验证队列能被 daemon worker 消费
orchestrator task list -o json
```

> ⚠️ CLI 二进制路径：C/S 模式的 CLI 在 `target/release/orchestrator`（crates/cli），
> 不是旧的单体二进制 `core/target/release/agent-orchestrator`。
> 如有 symlink 指向旧路径需更新。

### 3.2 初始化数据库并加载资源

```bash
orchestrator delete project/self-bootstrap --force
orchestrator init
orchestrator apply -f your-secrets.yaml           --project self-bootstrap
# apply additional secret manifests as needed      --project self-bootstrap
# 如需使用 Claude 原生 API，注释上行即可（claude-* 的模型配置将生效）
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-bootstrap
# ⚠️  必须使用 --project，否则真实 AI agent 会注册到全局空间
orchestrator apply -f docs/workflow/self-bootstrap.yaml --project self-bootstrap
```


### 3.3 验证资源已加载

验证资源已加载（可加 --project 限定项目范围）：

```bash
orchestrator get workspaces --project self-bootstrap -o json
orchestrator get agents --project self-bootstrap -o json
```

### 3.4 创建任务（把目标交给 orchestrator）

C/S 模式下 `task create` 会直接 enqueue 到 daemon worker，
任务创建即自动开始执行，不需要单独 `task start`。

在创建任务前，先确认目标文件范围：

- **选项 A：指定文件**（推荐）——只处理与本次课题直接相关的文件，执行速度快、聚焦度高。
- **选项 B：全量扫描**——省略 `-t`，系统自动扫描 `qa_targets` 配置的文件夹（默认 `docs/qa/`）下所有 `.md` 文件。适用于全面回归或文档治理场景，但 item 数量可能较多，执行时间相应增加。

#### 选项 A：指定目标文件

```bash
orchestrator task create \
  -n "<任务名>" \
  -w self -W self-bootstrap \
  --project self-bootstrap \
  -g "<将上方任务目标压缩成单行，直接作为 goal 传入>" \
  -t <目标文件1> \
  -t <目标文件2>
```

#### 选项 B：全量扫描

```bash
orchestrator task create \
  -n "<任务名>" \
  -w self -W self-bootstrap \
  --project self-bootstrap \
  -g "<将上方任务目标压缩成单行，直接作为 goal 传入>"
```

记录返回的 `<task_id>`。任务会立即被 worker 认领并开始执行。
如需等待完成，请使用 `orchestrator task watch <task_id>` 或轮询 `task info`。

---

## 4. 监控方法

本轮不人工改代码，只持续观察 orchestrator 是否按目标推进。

### 4.1 状态监控

```bash
orchestrator task list
orchestrator task info <task_id>
orchestrator task trace <task_id>    # 带异常检测的执行时间线
orchestrator task watch <task_id>    # 实时刷新状态面板
```

重点观察：

1. 当前 cycle
2. 当前步骤
3. task status 是否前进
4. `task trace` 中的步骤顺序是否符合预期
5. 是否出现 `failed`、`blocked`、长时间无进展

### 4.2 日志监控

```bash
orchestrator task logs --tail 100 <task_id>
orchestrator task logs --tail 200 <task_id>
```

重点观察：

1. `plan` 是否正确理解课题目标
2. `implement` 是否在解决根因，而不是做表面绕过
3. `self_test` 是否仍能发挥自举安全闸门作用
4. `qa_testing` / `ticket_fix` 是否发现并回收回归问题
5. 日志中各步骤的输出是否能定位卡住或偏题发生在哪一段

### 4.3 进程 / daemon 监控

```bash
# daemon 进程
ps aux | grep orchestratord | grep -v grep

# 队列/任务状态
orchestrator task list -o json

# agent 子进程（claude -p）
ps aux | grep "claude -p" | grep -v grep

# 代码变更
git diff --stat
```

重点观察：

1. agent 进程是否仍在推进，而不是僵死
2. `git diff --stat` 是否持续有合理变化
3. 若长时间零输出、零 diff、进程常驻不前，则记录为疑似卡住

### 4.4 补充诊断命令

当需要更细粒度观察时，人工可以补充使用：

```bash
orchestrator task trace <task_id> --json
orchestrator task watch <task_id>
orchestrator event list --task <task_id> --limit 20
```

适用场景：

1. 需要确认最近实际执行了哪些步骤、哪些步骤被 skip
2. 需要判断卡在调度层、agent 层还是事件落库层
3. 需要快速查看最近事件，确认 `step_started`、`step_finished`、guard 决策是否符合预期

---

## 5. 关键检查点

在监控过程中，人工只按下列检查点判断是否继续等待或需要中断。

### 5.1 Plan 阶段检查点

确认 orchestrator 理解的问题是：

1. 根因是什么
2. 完成态是什么
3. 哪些核心语义必须保留

如果 plan 明显偏题，或把课题降级成表面修补，应判定为偏题。

### 5.2 Implement 阶段检查点

确认代码改动至少满足以下其一：

1. 直接修复根因
2. 补齐缺失的回归保护
3. 消除临时 workaround

如果改动只发生在外围文档或配置，且未触及根因，应判定为不满足目标。

### 5.3 Self-Restart 阶段检查点

确认执行证据表明：

1. `self_restart` 在 Cycle 1 的 `self_test` 之后执行
2. daemon 通过 exec() 热重载新二进制（保持 PID 不变）
3. 新进程成功接管 `restart_pending` 任务并进入 Cycle 2
4. 如果 build 失败，任务正常继续（`on_failure: continue`），不影响后续步骤

监控 self_restart 热重载：
```bash
orchestrator event list --task <task_id> --type self_restart --limit 10
```

### 5.4 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. 编译和测试闸门未被绕过
3. 本轮改动未破坏基本自举安全性

### 5.5 Validation 阶段检查点

Cycle 2 中重点观察：

1. `qa_testing` 是否产出回归 ticket
2. `ticket_fix` 是否回收新问题
3. `align_tests` 是否补齐单测
4. `doc_governance` 是否未引入文档漂移

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. 核心修复不是表面绕过，而是解决了目标中定义的根因。
3. 关键完成态达成：`<在此填写课题的明确完成条件>`
4. `self_test` 仍能作为 builtin 正常执行。
5. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止“仅监控”模式并记录异常：

1. `plan` 明显偏题
2. `implement` 长时间无输出、无代码变更
3. `self_test` 失效或被绕过
4. `qa_testing` 持续产生同类 ticket，进入无效循环

### 7.2 C/S 架构特有异常

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| daemon 未运行 | CLI 报 `failed to connect to daemon at .../orchestrator.sock` | 用 `orchestratord --foreground --workers 2` 启动 |
| CLI 指向旧单体二进制 | `which orchestrator` 指向 `core/target/release/` | 更新 symlink 到 `target/release/orchestrator` |
| 重建后 daemon 仍用旧代码 | 观察到已修复的 bug 复现 | 杀掉旧 daemon 进程再启动新的 |
| task create 后任务立即开始 | task list 显示 `pending` 或很快变成 `running` | C/S 模式下 task lifecycle 为 queue-only，这是正常行为 |

建议记录方式：

```bash
orchestrator task info <task_id>
orchestrator task logs --tail 200 <task_id>
git diff --stat
```

必要时再由人工接管分析，而不是提前替 orchestrator 设计实现方案。

---

## 8. 人工角色边界

本计划中，人工角色明确限定为：

1. 提供目标
2. 启动 workflow
3. 监控状态
4. 在异常时中断并记录

人工不提前替 orchestrator 写实现计划，不预设代码改法，不把任务拆成手工子步骤。这个模板的目的，是复用一种稳定的执行方式来验证：当前 orchestrator 是否已经能围绕一个明确目标，自主完成自举课题。
