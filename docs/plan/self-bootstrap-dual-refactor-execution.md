# self-bootstrap 课题执行计划：双课题压力测试（output_validation 重构 + MessageBus 瘦身）

本文档基于 `self-bootstrap-execution-template.md` 模板，同时提交两个课题给 orchestrator，验证其处理多 target 的自举能力。

参考历史实例：
1. [`docs/plan/self-bootstrap-execution-template.md`](/Volumes/Yotta/ai_native_sdlc/docs/plan/self-bootstrap-execution-template.md)

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`output_validation 解析器统一 + MessageBus 死代码清理`
>
> 背景：
> 1. `core/src/output_validation.rs` 中 `parse_build_errors_from_text()` (L170-212) 和 `parse_test_failures_from_text()` (L215-294) 使用相似的逐行扫描/状态机模式解析 rustc/cargo 输出，但逻辑重复且未抽象。两个函数都接受 `(stderr, stdout)` 参数、合并文本、逐行匹配、维护解析状态，可以统一为 trait-based 解析器。
> 2. `core/src/collab/message.rs` 中 `subscribe()` (L243-246)、`get_latest_output()` (L249-273)、`find_subscribers()` (L275-289) 以及 `MessagePattern` enum (L299-352) 从未在生产代码中被调用——只有 `publish()` 被 `phase_runner.rs` 使用。这些死代码增加了维护负担和认知开销。
>
> 本轮任务目标：
> 1. 在 `output_validation.rs` 中引入统一的文本解析抽象（如 trait 或公共辅助函数），消除 `parse_build_errors_from_text` 与 `parse_test_failures_from_text` 之间的模式重复，保持现有测试全部通过。
> 2. 从 `collab/message.rs` 中移除所有未使用的订阅相关代码（`subscribe`、`get_latest_output`、`find_subscribers`、`MessagePattern`），简化 `MessageBus` 结构体（移除 `subscriptions` 字段），同时保持 `publish()` 和现有测试正常工作。
>
> 约束：
> 1. 优先解决根因，不接受仅做表面绕过。
> 2. 保留已有核心语义、兼容性要求：`validate_phase_output()` 的公共签名和返回行为不变；`MessageBus::publish()` 的行为不变；所有现有测试必须继续通过或被合理更新。
> 3. 最终目标是：output_validation 中解析逻辑无重复模式，MessageBus 中无死代码，`cargo test` 全绿，`cargo clippy` 无新 warning。

### 1.1 预期产出

由 orchestrator 自主产出并落地：

1. 一份实现计划（由 `plan` 步骤生成）。
2. 必要的 QA 文档更新（由 `qa_doc_gen` 判断是否生成/更新）。
3. 与课题目标对应的代码改动（涉及 `output_validation.rs` 和 `collab/message.rs`）。
4. 自举回归验证结果。
5. 若本轮发现问题，由 `ticket_fix` 和后续步骤尝试收口。

### 1.2 非目标

本次不由人工预先定义实现细节；不在计划文档中替 orchestrator 指定具体代码改法。实现路径由 workflow 自主决定，人工只观察其是否偏离目标。

---

## 2. 执行方式

本轮按 `self-bootstrap` 的标准链路执行，不做人肉拆任务：

```text
plan -> qa_doc_gen -> implement -> self_test -> qa_testing -> ticket_fix -> align_tests -> doc_governance -> loop_guard
```

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

```bash
./scripts/orchestrator.sh task create \
  -n "dual-refactor-output-validation-and-messagebus" \
  -w self -W self-bootstrap \
  --no-start \
  -g "课题：output_validation 解析器统一 + MessageBus 死代码清理。目标1：在 output_validation.rs 中引入统一的文本解析抽象（trait 或公共辅助函数），消除 parse_build_errors_from_text 与 parse_test_failures_from_text 之间的模式重复，保持现有测试全部通过。目标2：从 collab/message.rs 中移除所有未使用的订阅相关代码（subscribe、get_latest_output、find_subscribers、MessagePattern），简化 MessageBus 结构体（移除 subscriptions 字段），保持 publish() 和现有测试正常工作。约束：validate_phase_output() 公共签名和返回行为不变；MessageBus::publish() 行为不变；cargo test 全绿；cargo clippy 无新 warning。" \
  -t core/src/output_validation.rs \
  -t core/src/collab/message.rs
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

1. `plan` 是否正确理解**双课题**——两个 target 都被纳入计划
2. `implement` 是否同时处理两个文件，而不是只改了一个就跳过另一个
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
2. `git diff --stat` 是否持续有合理变化——预期至少涉及 `output_validation.rs` 和 `collab/message.rs` 两个文件
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

1. **双课题覆盖**：计划中同时包含 output_validation 重构和 MessageBus 清理
2. output_validation 的根因是两个解析函数的模式重复
3. MessageBus 的根因是 subscribe/pattern-matching 基础设施从未被使用
4. 完成态明确：无重复解析模式 + 无死代码 + 测试全绿

如果 plan 只涉及一个课题而遗漏另一个，应判定为偏题。

### 5.2 Implement 阶段检查点

确认代码改动满足：

1. `output_validation.rs` 中引入了统一抽象，两个解析函数的重复模式被消除
2. `collab/message.rs` 中 `subscribe`、`get_latest_output`、`find_subscribers`、`MessagePattern` 被移除
3. `MessageBus` 结构体的 `subscriptions` 字段被移除
4. `publish()` 行为未被破坏

如果改动只发生在外围文档或配置，且未触及两个目标文件，应判定为不满足目标。

### 5.3 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. `cargo check` 和 `cargo test` 闸门未被绕过
3. 本轮改动未破坏基本自举安全性

### 5.4 Validation 阶段检查点

Cycle 2 中重点观察：

1. `qa_testing` 是否产出回归 ticket
2. `ticket_fix` 是否回收新问题
3. `align_tests` 是否补齐单测——特别关注是否为新的统一解析抽象补充了测试
4. `doc_governance` 是否未引入文档漂移

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. **output_validation.rs**：两个解析函数的重复逐行扫描模式被统一抽象消除，`validate_phase_output()` 签名和行为不变。
3. **collab/message.rs**：`subscribe()`、`get_latest_output()`、`find_subscribers()`、`MessagePattern` 已移除；`MessageBus` 不再包含 `subscriptions` 字段；`publish()` 正常工作。
4. `cargo test` 全绿，`cargo clippy` 无新 warning。
5. `self_test` 仍能作为 builtin 正常执行。
6. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止"仅监控"模式并记录异常：

1. `plan` 明显偏题或只覆盖了一个课题
2. `implement` 长时间无输出、无代码变更
3. `implement` 只改了一个文件而忽略另一个
4. `self_test` 失效或被绕过
5. `qa_testing` 持续产生同类 ticket，进入无效循环

建议记录方式：

```bash
./scripts/orchestrator.sh task info <task_id> -o json
./scripts/orchestrator.sh task logs --tail 200 <task_id>
git diff --stat
```

必要时再由人工接管分析，而不是提前替 orchestrator 设计实现方案。

---

## 8. 压力测试观察点

本轮额外关注以下自举能力维度：

1. **多 target 协调**：orchestrator 是否能在同一个 task 中合理安排两个文件的改动顺序，而不是串行处理完一个再处理另一个时遗漏上下文。
2. **跨文件影响分析**：两个课题相互独立（output_validation 和 collab/message 无依赖关系），orchestrator 是否正确判断为可并行处理。
3. **测试覆盖策略**：移除 MessageBus 死代码后，原有测试是否被合理调整（删除不再需要的测试 vs 保留仍有意义的测试）。
4. **scope 控制**：orchestrator 是否只改了目标范围内的代码，而没有过度重构。

---

## 9. 人工角色边界

本计划中，人工角色明确限定为：

1. 提供目标
2. 启动 workflow
3. 监控状态
4. 在异常时中断并记录

人工不提前替 orchestrator 写实现计划，不预设代码改法，不把任务拆成手工子步骤。这个模板的目的，是复用一种稳定的执行方式来验证：当前 orchestrator 是否已经能围绕一个明确目标，自主完成自举课题。
