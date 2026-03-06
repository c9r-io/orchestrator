# QA 场景隔离：消除 `db reset --force` 避免自举自毁

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`QA 场景隔离 — 消除 db reset --force，改用项目级隔离`
>
> 背景：
> self-bootstrap workflow 在 Cycle 2 的 `qa_testing` 步骤中，QA agents 并行执行多个 QA 文档。
> 部分 QA 文档（04-cli-config-db.md、21-runner-security-observability.md、
> 31-runner-policy-defaults-compatibility.md）包含 `db reset --force` 命令，这会清除整个
> tasks 表，导致正在运行的 self-bootstrap task 丢失、item-scoped 步骤报 FOREIGN KEY
> constraint failed、QA agents 变成孤儿进程。
>
> 本轮任务目标：
> 1. 修改 `docs/qa/orchestrator/21-runner-security-observability.md`，将 `db reset --force`
>    替换为 `qa project reset "${QA_PROJECT}" --keep-config --force` 加项目级隔离，
>    保留测试场景的验证意图不变。
> 2. 修改 `docs/qa/orchestrator/31-runner-policy-defaults-compatibility.md`，同样将所有
>    `db reset --force`（两处：Scenario 1 第 36 行、Scenario 2 第 111 行）替换为项目级隔离。
> 3. 修改 `docs/qa/orchestrator/04-cli-config-db.md` 的 Scenario 4（Database Reset），
>    将实际执行 `db reset --force` 改为只验证 `db reset` 不带 `--force` 时的确认提示行为
>    和退出码，不实际执行全库清除。如果该场景原本就是测试 reset 命令本身的功能，则将验证
>    方式改为检查帮助文档或不带 `--force` 的行为，避免实际 reset。
> 4. 确认 `docs/qa/orchestrator/43-cli-force-gate-audit.md` 不需要改动（它已经只测
>    "不带 --force 的行为"，不实际执行 reset）。
> 5. 在修改的 QA 文档中，遵循 `docs/qa/README.md` 已有的规范：
>    "Do not run `orchestrator db reset --include-config` or any equivalent
>    config-destructive reset as part of routine QA scenario execution."
>    将此规范扩展为同样禁止 `db reset --force`，更新 README 中的规范说明。
>
> 约束：
> 1. 每个 QA 文档的**验证意图**必须保留——只改隔离方式，不删减测试覆盖。
> 2. 替代方案使用已有的 `qa project reset` + `--keep-config --force` 模式。
> 3. 最终目标是：`docs/qa/` 下所有 QA 文档中不再出现 `db reset --force`，
>    自举运行期间的 QA 测试不会破坏运行时数据库状态。

### 1.1 预期产出

由 orchestrator 自主产出并落地：

1. 一份实现计划（由 `plan` 步骤生成）。
2. 必要的 QA 文档更新（由 `qa_doc_gen` 判断是否生成/更新）。
3. 与课题目标对应的 QA 文档改动。
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

**本课题的特殊意义**：本课题直接修复上次实验中 `qa_testing` 自毁的 blocker。如果修复成功，
Cycle 2 的 `qa_testing` 将不再执行 `db reset --force`，整个 2-cycle 流程应该能完整走完。
这本身就是对修复效果的端到端验证。

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
./scripts/orchestrator.sh apply -f docs/workflow/claude-secret.yaml
./scripts/orchestrator.sh apply -f docs/workflow/minimax-secret.yaml
# 如需使用 Claude 原生 API，注释上行即可
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

本课题主要改动 QA 文档，建议使用 **选项 A：指定目标文件**，仅关联受影响的 QA 文档。

```bash
./scripts/orchestrator.sh task create \
  -n "qa-db-reset-isolation" \
  -w self -W self-bootstrap \
  --no-start \
  -g "QA 场景隔离：消除 db reset --force 避免自举自毁。修改 docs/qa/orchestrator/04-cli-config-db.md、21-runner-security-observability.md、31-runner-policy-defaults-compatibility.md，将 db reset --force 替换为 qa project reset 加项目级隔离，保留验证意图不变。确认 43-cli-force-gate-audit.md 无需改动。更新 docs/qa/README.md 规范说明，扩展禁止 db reset --force。最终目标：docs/qa/ 下所有 QA 文档中不再出现 db reset --force，自举运行期间的 QA 测试不会破坏运行时数据库状态。" \
  -t docs/qa/orchestrator/04-cli-config-db.md \
  -t docs/qa/orchestrator/21-runner-security-observability.md \
  -t docs/qa/orchestrator/31-runner-policy-defaults-compatibility.md \
  -t docs/qa/orchestrator/43-cli-force-gate-audit.md
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

1. `plan` 是否正确理解课题目标（修改 QA 文档隔离方式，不删减验证覆盖）
2. `implement` 是否在修改 QA 文档，将 `db reset --force` 替换为项目级隔离
3. `self_test` 是否仍能发挥自举安全闸门作用
4. `qa_testing` 是否不再出现 FOREIGN KEY constraint failed
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

1. 根因是：QA 文档中的 `db reset --force` 清除了运行时 tasks 表
2. 完成态是：所有 QA 文档改用项目级隔离，不再出现 `db reset --force`
3. 必须保留：每个 QA 场景的验证意图（测试覆盖不减少）

如果 plan 明显偏题，或把课题降级成表面修补（如仅注释掉 reset 命令），应判定为偏题。

### 5.2 Implement 阶段检查点

确认改动至少满足：

1. `21-runner-security-observability.md` 中 `db reset --force` 被替换为 `qa project reset`
2. `31-runner-policy-defaults-compatibility.md` 中两处 `db reset --force` 被替换
3. `04-cli-config-db.md` 的 Scenario 4 改为非破坏性验证方式
4. `docs/qa/README.md` 规范更新

如果改动只发生在无关文件，或删减了测试场景而非替换隔离方式，应判定为不满足目标。

### 5.3 Self-Restart 阶段检查点

确认执行证据表明：

1. `self_restart` 在 Cycle 1 的 `self_test` 之后执行
2. 进程以 exit code 75 退出，orchestrator.sh 自动重启
3. 新进程成功接管 `restart_pending` 任务并进入 Cycle 2

附加验证（上次课题的改动现在应该生效）：
```bash
sqlite3 data/agent_orchestrator.db \
  "SELECT payload_json FROM events WHERE task_id = '<task_id>' AND event_type = 'self_restart_ready' ORDER BY id DESC LIMIT 1;" \
  | python3 -m json.tool
```

预期输出应包含 `old_binary_sha256`、`new_binary_sha256`、`binary_changed` 字段（因为现在运行的 binary 包含了上次课题的改动）。

### 5.4 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. 编译和测试闸门未被绕过
3. 本轮改动未破坏基本自举安全性

### 5.5 Validation 阶段检查点（本课题核心验证点）

Cycle 2 中重点观察：

1. **关键**：`qa_testing` 步骤是否完整执行完毕，而非中途自毁
2. **关键**：tasks 表中本 task 记录是否始终存在（不再消失）
3. `qa_testing` 是否产出回归 ticket
4. `ticket_fix` 是否回收新问题
5. `align_tests` 是否补齐单测
6. `doc_governance` 是否未引入文档漂移

验证 task 记录完整性：
```bash
sqlite3 data/agent_orchestrator.db "SELECT id, name, status FROM tasks WHERE id = '<task_id>';"
# 应返回记录，且 status 不为空
```

验证无 FK 错误：
```bash
sqlite3 data/agent_orchestrator.db "SELECT count(*) FROM events WHERE task_id = '<task_id>' AND payload_json LIKE '%FOREIGN KEY%';"
# 应返回 0
```

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. 核心修复不是表面绕过，而是在 QA 文档中正确替换了隔离方式。
3. 关键完成态达成：
   - `docs/qa/` 下所有 QA 文档不再包含 `db reset --force`
   - Cycle 2 的 `qa_testing` 完整执行完毕，无 FOREIGN KEY 错误
   - QA 场景的验证意图保留（测试覆盖不减少）
   - `docs/qa/README.md` 规范已更新
4. `self_test` 仍能作为 builtin 正常执行。
5. self-restart 流程正常完成（exit 75 → 重启 → 接管 → Cycle 2）。
6. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止"仅监控"模式并记录异常：

1. `plan` 明显偏题
2. `implement` 长时间无输出、无代码变更
3. `self_test` 失效或被绕过
4. `qa_testing` 再次出现 FOREIGN KEY constraint failed（说明修复未生效）
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

## 附录：本课题对 2-cycle 流程完整性验证的意义

上次实验（课题 B：self_restart 事件增强）中，Cycle 1 全部通过但 Cycle 2 在 `qa_testing` 阶段自毁。本课题直接修复该 blocker：

- **如果修复成功**：Cycle 2 的 `qa_testing` → `ticket_fix` → `align_tests` → `doc_governance` → `loop_guard` 全链路首次完整执行
- **如果修复失败**：说明替代方案不充分，需要进一步调查 QA 隔离机制
- **附加验证**：上次课题（课题 B）的 `self_restart_ready` 事件增强可以在本次 restart 中得到验证，因为当前 binary 已包含 old/new SHA256 改动
