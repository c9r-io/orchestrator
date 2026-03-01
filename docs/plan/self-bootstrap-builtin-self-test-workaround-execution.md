# self-bootstrap builtin self_test 去债执行计划

本文档采用此前 [`docs/plan/resource-rs-refactor-execution.md`](/Volumes/Yotta/ai_native_sdlc/docs/plan/resource-rs-refactor-execution.md) 的执行模式：把课题目标直接交给 orchestrator，由 `self-bootstrap` workflow 自主规划、实现、验证；人工只负责启动、监控、记录，并在异常时介入。

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为下一轮 self-bootstrap 的课题：

> 当前项目存在一项技术负债：在 workflow 调度定义中，builtin 步骤 `self_test` 仍存在被误路由为 capability lookup 的瑕疵，导致使用者需要在 `docs/workflow/self-bootstrap.yaml` 中为 `coder` agent 硬编码一套 `self_test` Shell 指令作为临时 workaround。
>
> 本轮任务目标是：围绕 `docs/workflow/self-bootstrap.yaml` 完成这项技术负债的自举修复与回归验证，让 `self_test` 回归真正的 builtin 执行路径，不再依赖 agent capability/template 兜底；并把本次修复收口到可维护状态：将 builtin effective execution 的判定收敛到共享入口，避免在 `config.rs` 与 `item_executor.rs` 维持双份规则，同时补上真正穿过 `load_task_runtime_context()` 与实际 step dispatch 的回归测试，而不是只用 helper 复制逻辑自证。
>
> 约束：
> 1. 必须以 `self-bootstrap` workflow 为主执行路径。
> 2. 由 orchestrator 自行完成 plan、qa_doc_gen、implement、self_test、qa_testing、ticket_fix、align_tests、doc_governance。
> 3. 优先修复 builtin 路由问题，而不是仅删除 YAML 中的 workaround。
> 4. 保留 `self_test` 现有语义：`cargo check`、`cargo test --lib`、`manifest validate`，以及相关事件、状态、pipeline variables。
> 5. 最终目标是让 `docs/workflow/self-bootstrap.yaml` 不再需要 `coder` agent 上的 `self_test` capability/template workaround。
> 6. 将 effective execution 判定收敛到共享入口，不保留 `config.rs` 与 `item_executor.rs` 的双份判定逻辑。
> 7. 回归测试必须覆盖真实运行路径：至少穿过 `load_task_runtime_context()`，并验证实际 step dispatch，而不是仅验证复制出来的 helper。

### 1.1 预期产出

由 orchestrator 自主产出并落地：

1. 一份实现计划（由 `plan` 步骤生成）。
2. 必要的 QA 文档更新（由 `qa_doc_gen` 步骤决定是否生成/更新）。
3. 修复 builtin `self_test` 路由问题的代码改动。
4. 删除 `docs/workflow/self-bootstrap.yaml` 中仅用于兜底的 `self_test` workaround（如果底层修复已经完成）。
5. 回归验证结果，包括 `self_test` 在无 workaround 前提下可运行的证据。
6. 一轮“质量收尾”改动：收敛 builtin effective execution 判定入口，减少重复逻辑。
7. 至少一组真正覆盖 runtime load + step dispatch 的回归测试证据。

### 1.2 非目标

本次不由人工预先定义实现细节；不在计划文档中替 orchestrator 指定具体改哪几行、如何抽象。实现路径由 workflow 内的 `plan` 和后续步骤决定，人工只观察其是否偏离目标。

---

## 2. 执行方式

本轮按 `self-bootstrap` 的标准链路执行，不做人肉拆任务：

```
plan -> qa_doc_gen -> implement -> self_test -> qa_testing -> ticket_fix -> align_tests -> doc_governance -> loop_guard
```

人工职责只有两类：

1. 启动和提供课题目标。
2. 监控执行状态、判断是否卡住、记录结果。

补充围挡：

1. 某些低价模型已知会出现长静默窗口，`low_output_step` 单独出现时不直接视为故障；若 heartbeat 仍增长、`pid_alive=true`，应继续观察。
2. Cycle 2 预期验证链必须实际进入 `qa_testing -> ticket_fix`（如有 ticket）-> `align_tests` -> `doc_governance`。若因 item 被提前 finalize / skipped 而短路，必须视为流程缺陷并记录 ticket。

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

```bash
./scripts/orchestrator.sh task create \
  -n "self-bootstrap-builtin-self-test-workaround" \
  -w self -W self-bootstrap \
  --no-start \
  -g "当前项目存在一项技术负债：在 workflow 调度定义中，builtin 步骤 self_test 仍存在被误路由为 capability lookup 的瑕疵，导致使用者需要在 docs/workflow/self-bootstrap.yaml 中为 coder agent 硬编码一套 self_test Shell 指令作为临时 workaround。本轮任务目标是：围绕 docs/workflow/self-bootstrap.yaml 完成这项技术负债的自举修复与回归验证，让 self_test 回归真正的 builtin 执行路径，不再依赖 agent capability/template 兜底。约束：1. 必须以 self-bootstrap workflow 为主执行路径。2. 由 orchestrator 自行完成 plan、qa_doc_gen、implement、self_test、qa_testing、ticket_fix、align_tests、doc_governance。3. 优先修复 builtin 路由问题，而不是仅删除 YAML 中的 workaround。4. 保留 self_test 现有语义：cargo check、cargo test --lib、manifest validate，以及相关事件、状态、pipeline variables。5. 最终目标是让 docs/workflow/self-bootstrap.yaml 不再需要 coder agent 上的 self_test capability/template workaround。" \
  -t docs/workflow/self-bootstrap.yaml
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
4. `task trace` 中 `plan -> qa_doc_gen -> implement -> self_test` 的链路是否按预期推进
5. 是否出现 `failed`、`blocked`、长时间无进展

### 4.2 日志监控

```bash
./scripts/orchestrator.sh task logs <task_id> --tail 100
ls data/logs/<task_id>/
tail -n 100 data/logs/<task_id>/implement_<run_id>.stdout
tail -n 100 data/logs/<task_id>/implement_<run_id>.stderr
```

重点观察：

1. `plan` 是否产出明确的修复计划
2. `implement` 是否真的修改 builtin 路由，而不是只做 YAML 删除
3. `self_test` 是否在修复后走 builtin 语义
4. `qa_testing` / `ticket_fix` 是否发现并回收回归问题
5. `qa_testing` / `ticket_fix` 在 Cycle 2 是否真的进入执行，而不是因 item 提前 finalize 被短路
6. 分步骤日志是否能定位卡住或偏题发生在哪个阶段

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

当普通状态和日志不足以判断时，人工可以补充使用：

```bash
./scripts/orchestrator.sh task trace <task_id> -o json
./scripts/orchestrator.sh debug --component scheduler -v
sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id = '<task_id>' ORDER BY id DESC LIMIT 20;"
```

重点用途：

1. 确认 `self_test` 是否被执行、被跳过，还是被错误路由
2. 确认 Cycle 2 是否真实进入 `qa_testing` / `ticket_fix`，还是被 finalize / skip 提前短路
3. 判断卡住发生在调度层、agent 执行层还是事件落库层
4. 快速查看最近事件，确认 `step_started`、`step_finished`、guard 决策是否符合预期

---

## 5. 关键检查点

在监控过程中，人工只按下列检查点判断是否继续等待或需要中断：

### 5.1 Plan 阶段检查点

确认 orchestrator 理解的问题是：

1. 先修 builtin 路由误判
2. 再移除 `docs/workflow/self-bootstrap.yaml` 中的 workaround
3. 需要保留 `self_test` 的完整 builtin 语义

如果 plan 只提“删掉 YAML 里的模板”而未触及底层路由，应判定为偏题。

### 5.2 Implement 阶段检查点

确认代码改动至少覆盖以下方向之一：

1. builtin 判定路径
2. task plan / dynamic insert 时的 `self_test` builtin 识别
3. scheduler 执行时 builtin 优先路径
4. 将 effective execution 判定收敛到共享入口，避免在 `config.rs` 与 `item_executor.rs` 维持重复规则

如果改动只发生在 [`docs/workflow/self-bootstrap.yaml`](/Volumes/Yotta/ai_native_sdlc/docs/workflow/self-bootstrap.yaml) 而核心代码无变化，应判定为不满足目标。

### 5.3 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. 行为包含 `cargo check`、`cargo test --lib`
3. 若实现包含 manifest 校验，则该阶段未被破坏
4. 回归测试不是只验证 helper，而是至少有一条真实运行路径覆盖 `load_task_runtime_context()` 或实际 step dispatch

如果 `self_test` 因删掉 workaround 后直接失效，说明本轮未修成。

### 5.4 Validation 阶段检查点

Cycle 2 中重点观察：

1. `qa_testing` 是否实际进入执行（必须有 `step_started` / `step_finished` 或对应 item 级事件）
2. `ticket_fix` 是否在存在 ticket 时实际进入执行
3. 若 `qa_testing` / `ticket_fix` 被提前 finalize / skipped 短路，直接判定为流程缺陷
4. `align_tests` 是否补齐单测
5. `doc_governance` 是否未引入文档漂移

---

## 6. 成功判定

当以下条件同时成立，可判定这轮自举课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. 核心修复不是仅删 workaround，而是修复了 builtin `self_test` 的底层路由问题。
3. [`docs/workflow/self-bootstrap.yaml`](/Volumes/Yotta/ai_native_sdlc/docs/workflow/self-bootstrap.yaml) 中不再需要 `coder` agent 的 `self_test` capability/template。
4. `self_test` 仍能作为 builtin 正常执行，并保留原有验证语义。
5. builtin effective execution 判定已收敛到共享入口，不再在 `config.rs` 与 `item_executor.rs` 维护双份规则。
6. 回归测试覆盖真实运行路径，而不是仅靠复制逻辑的 helper 自证。
7. Cycle 2 实际跑到 `qa_testing`，且在有 ticket 时实际跑到 `ticket_fix`，不能被 item 提前 finalize / skipped 短路。
8. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止“仅监控”模式并记录异常：

1. `plan` 明显偏题，未聚焦 builtin 路由技术债。
2. `implement` 长时间无输出、无代码变更。
3. `self_test` 因移除 workaround 后失效，且 workflow 无法自行修复。
4. 代码层面仍同时保留 `config.rs` 与 `item_executor.rs` 的双份 effective execution 判定逻辑。
5. 测试仍只验证 helper / 局部逻辑，没有覆盖真实 runtime load 或实际 dispatch。
6. Cycle 2 未实际进入 `qa_testing` / `ticket_fix`，而是被 finalize / skipped 提前短路。
7. `qa_testing` 持续产生同类 ticket，进入无效循环。

建议记录方式：

```bash
./scripts/orchestrator.sh task info <task_id> -o json
./scripts/orchestrator.sh task logs <task_id> --tail 200
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

人工不提前替 orchestrator 写实现计划，不预设代码改法，不把任务拆成手工子步骤。这个课题的重点就是验证：当前 orchestrator 是否已经能围绕一个明确目标，自主完成 builtin `self_test` 技术债的自举修复。
