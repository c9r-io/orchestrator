# self-restart 事件增强：记录 old/new binary diff 摘要

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`self_restart 事件增强 — 记录 old/new binary diff 摘要`
>
> 背景：
> 当前 `self_restart_ready` 事件记录了新 binary 的 SHA256 hash 和 build 信息，
> 但缺少旧 binary（即当前正在运行的 binary）的 SHA256 hash 对比。
> `binary_verification` 事件虽然有 expected/actual 对比，但那是 post-restart 阶段的，
> 无法在 restart 前就确认 "确实产生了不同的 binary"。
> 人工查看日志时，需要跨两个事件才能确认 binary 是否真的换了。
>
> 本轮任务目标：
> 1. 在 `execute_self_restart_step` 函数的 Phase 3（snapshot）之前，计算当前运行 binary 的 SHA256 作为 `old_binary_sha256`。
> 2. 在 `self_restart_ready` 事件的 payload 中同时包含 `old_binary_sha256` 和 `new_binary_sha256`，以及 `binary_changed: bool` 标志。
> 3. 在 `verify_post_restart_binary` 中，将 `old_binary_sha256` 也读出并写入 `binary_verification` 事件，形成完整的 old → expected → actual 审计链。
> 4. 为上述逻辑补充单元测试。
>
> 约束：
> 1. 优先解决根因，不接受仅做表面绕过。
> 2. 保留已有核心语义：EXIT_RESTART (75) 退出码、snapshot_binary 流程、restart_pending 状态转换、post-restart verification 逻辑。
> 3. 最终目标是：self_restart_ready 事件同时包含 old/new binary SHA256 和 binary_changed 标志；binary_verification 事件包含完整的 old → expected → actual 审计链。

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
> (`cargo build --release`), verifies it (`--help`), snapshots `.stable`, sets the task to
> `restart_pending`, and exits with code 75. The wrapper script (`orchestrator.sh`) detects
> exit 75 and relaunches the new binary. The new process auto-resumes the `restart_pending`
> task and continues into Cycle 2. This step has `repeatable: false`, so it only runs in Cycle 1.
> Build failure is non-fatal (`on_failure: continue`) — the loop continues with the old binary.

**本课题的特殊意义**：改动直接发生在 self-restart 路径上（`safety.rs`），因此 self-restart 本身就是对该改动的端到端验证。如果 Cycle 1 的 implement 正确修改了事件 payload，那么：
- Cycle 1 的 `self_restart_ready` 事件将包含 `old_binary_sha256` 字段
- Cycle 2 的 `binary_verification` 事件将包含完整的 old → expected → actual 审计链
- 这些事件可通过 `task trace` 或直接查询 events 表来验证

人工职责只有两类：

1. 启动和提供课题目标。
2. 监控执行状态、判断是否卡住、记录结果。

---

## 3. 启动步骤

### 3.1 构建并初始化运行时

```bash
cd /Volumes/Yotta/ai_native_sdlc

cd core && cargo build --release && cd ..

./scripts/orchestrator.sh db reset -f --include-config --include-history
./scripts/orchestrator.sh init -f
./scripts/orchestrator.sh apply -f docs/workflow/self-bootstrap.yaml
```

### 3.2 验证资源已加载

```bash
./scripts/orchestrator.sh get workspace
./scripts/orchestrator.sh get workflow
./scripts/orchestrator.sh get agent
```

预期至少可见：

1. workspace `self`
2. workflow `self-bootstrap`
3. agents `architect`、`coder`、`tester`、`reviewer`

### 3.3 创建任务（把目标交给 orchestrator）

本课题改动集中在 `core/src/scheduler/safety.rs`，不涉及大范围 QA 文档。建议使用 **选项 A：指定目标文件**，仅关联与 self-restart 相关的 QA 文档（如有），或使用 **选项 B：全量扫描** 让系统自动判断。

#### 推荐：选项 B（全量扫描）

由于本课题涉及 self-restart 行为增强，可能影响多个已有 QA 文档中的 self-restart 验证步骤，建议让系统自动扫描 `docs/qa/` 来发现相关文档。

```bash
./scripts/orchestrator.sh task create \
  -n "self-restart-old-new-diff" \
  -w self -W self-bootstrap \
  --no-start \
  -g "self_restart 事件增强：在 execute_self_restart_step 中计算当前运行 binary 的 SHA256 作为 old_binary_sha256，在 self_restart_ready 事件 payload 中同时包含 old_binary_sha256 和 new_binary_sha256 以及 binary_changed bool 标志。在 verify_post_restart_binary 中将 old_binary_sha256 也读出并写入 binary_verification 事件形成完整的 old->expected->actual 审计链。为上述逻辑补充单元测试。保留已有核心语义：EXIT_RESTART 退出码、snapshot_binary 流程、restart_pending 状态转换、post-restart verification 逻辑。"
```

记录返回的 `<task_id>`，然后启动：

```bash
./scripts/orchestrator.sh task start <task_id>
```

---

## 4. 监控方法

本轮不人工改代码，只持续观察 orchestrator 是否按目标推进。

### 4.1 状态监控

```bash
./scripts/orchestrator.sh task list
./scripts/orchestrator.sh task info <task_id> -o json
./scripts/orchestrator.sh task trace <task_id>
```

重点观察：

1. 当前 cycle
2. 当前步骤
3. task status 是否前进
4. `task trace` 中的步骤顺序是否符合预期
5. 是否出现 `failed`、`blocked`、长时间无进展

### 4.2 日志监控

```bash
./scripts/orchestrator.sh task logs --tail 100 <task_id>
./scripts/orchestrator.sh task logs --tail 200 <task_id>
```

重点观察：

1. `plan` 是否正确理解课题目标（增强 self_restart_ready 和 binary_verification 事件）
2. `implement` 是否在修改 `safety.rs` 的事件 payload，而不是做表面绕过
3. `self_test` 是否仍能发挥自举安全闸门作用
4. `qa_testing` / `ticket_fix` 是否发现并回收回归问题
5. 日志中各步骤的输出是否能定位卡住或偏题发生在哪一段

### 4.3 进程监控

```bash
ps aux | grep -E "opencode|agent-orchestrator" | grep -v grep
git diff --stat
```

重点观察：

1. agent 进程是否仍在推进，而不是僵死
2. `git diff --stat` 是否持续有合理变化
3. 若长时间零输出、零 diff、进程常驻不前，则记录为疑似卡住

### 4.4 补充诊断命令

当需要更细粒度观察时，人工可以补充使用：

```bash
./scripts/orchestrator.sh task trace <task_id> --json
./scripts/orchestrator.sh task watch <task_id>
sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id = '<task_id>' ORDER BY id DESC LIMIT 20;"
```

---

## 5. 关键检查点

### 5.1 Plan 阶段检查点

确认 orchestrator 理解的问题是：

1. 根因是：`self_restart_ready` 事件缺少旧 binary 的 SHA256，无法在 restart 前确认 binary 变更
2. 完成态是：事件同时包含 old/new hash 和 `binary_changed` 标志，verification 有完整审计链
3. 必须保留：EXIT_RESTART 退出码、snapshot_binary 流程、restart_pending 状态机、post-restart verification

如果 plan 明显偏题，或把课题降级成表面修补，应判定为偏题。

### 5.2 Implement 阶段检查点

确认代码改动至少满足以下其一：

1. 在 `execute_self_restart_step` 中增加 `old_binary_sha256` 计算
2. `self_restart_ready` 事件 payload 包含 old/new 对比字段
3. `verify_post_restart_binary` 中读出并记录 `old_binary_sha256`

如果改动只发生在外围文档或配置，且未触及 `safety.rs`，应判定为不满足目标。

### 5.3 Self-Restart 阶段检查点

**本课题的核心验证点**：

1. `self_restart` 在 Cycle 1 的 `self_test` 之后执行
2. 进程以 exit code 75 退出，orchestrator.sh 自动重启
3. 新进程成功接管 `restart_pending` 任务并进入 Cycle 2
4. **关键**：查询 `self_restart_ready` 事件，确认 payload 包含 `old_binary_sha256` 字段

验证新事件格式：
```bash
sqlite3 data/agent_orchestrator.db \
  "SELECT payload_json FROM events WHERE task_id = '<task_id>' AND event_type = 'self_restart_ready' ORDER BY id DESC LIMIT 1;" \
  | python3 -m json.tool
```

预期输出应包含：
```json
{
  "exit_code": 75,
  "old_binary_sha256": "<SHA256 of old binary>",
  "new_binary_sha256": "<SHA256 of new binary>",
  "binary_changed": true,
  "binary_path": "...",
  "build_git_hash": "...",
  "build_timestamp": "..."
}
```

### 5.4 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. 编译和测试闸门未被绕过
3. 本轮改动未破坏基本自举安全性

### 5.5 Validation 阶段检查点

Cycle 2 中重点观察：

1. `binary_verification` 事件是否包含完整的 old → expected → actual 审计链
2. `qa_testing` 是否产出回归 ticket
3. `ticket_fix` 是否回收新问题
4. `align_tests` 是否补齐单测
5. `doc_governance` 是否未引入文档漂移

验证 post-restart verification 事件：
```bash
sqlite3 data/agent_orchestrator.db \
  "SELECT payload_json FROM events WHERE task_id = '<task_id>' AND event_type = 'binary_verification' ORDER BY id DESC LIMIT 1;" \
  | python3 -m json.tool
```

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. 核心修复不是表面绕过，而是在 `safety.rs` 中正确计算并记录了 old binary SHA256。
3. 关键完成态达成：
   - `self_restart_ready` 事件包含 `old_binary_sha256`、`new_binary_sha256`、`binary_changed` 字段
   - `binary_verification` 事件包含 `old_binary_sha256` 字段，形成完整审计链
   - 新增的逻辑有对应的单元测试
4. `self_test` 仍能作为 builtin 正常执行。
5. self-restart 流程本身正常完成（exit 75 → 重启 → 接管 → Cycle 2）。
6. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止"仅监控"模式并记录异常：

1. `plan` 明显偏题
2. `implement` 长时间无输出、无代码变更
3. `self_test` 失效或被绕过
4. `self_restart` 因本课题的改动导致 crash（而非正常 exit 75）
5. `qa_testing` 持续产生同类 ticket，进入无效循环

建议记录方式：

```bash
./scripts/orchestrator.sh task info <task_id> -o json
./scripts/orchestrator.sh task logs --tail 200 <task_id>
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

---

## 附录：本课题对 self-restart 验证的特殊价值

本课题的改动目标（safety.rs 中的事件 payload）直接位于 self-restart 代码路径上。这意味着：

- **如果 self-restart 成功**：新 binary 包含了增强后的事件逻辑，事件本身就是验证证据
- **如果 self-restart 失败**：说明改动破坏了 restart 流程，需要在 `on_failure: continue` 路径下继续，并在 Cycle 2 修复
- **如果 binary 未变化**（`binary_changed: false`）：说明 implement 步骤的改动可能未被正确编译进 release binary

这种"改动即验证"的特性使得本课题成为 self-restart 功能的理想首次实验。
