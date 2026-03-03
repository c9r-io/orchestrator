# self-bootstrap 课题执行计划：scheduler/query.rs 模块化重构

本文档基于 `self-bootstrap-execution-template.md` 模板，用于验证 orchestrator 自举能力。

建议参考历史实例：

1. [`docs/plan/self-bootstrap-execution-template.md`](/Volumes/Yotta/ai_native_sdlc/docs/plan/self-bootstrap-execution-template.md)

### Agent 工具链说明

本轮 self-bootstrap workflow 中使用的 agent 命令行工具映射如下：

| 命令 | 实际工具链 | 角色 |
|------|-----------|------|
| `glmcode` | Claude Code + GLM (智谱) | architect（plan）、coder（implement / ticket_fix / align_tests） |
| `minicode` | Claude Code + MiniMax | architect（qa_doc_gen）、tester（qa_testing）、reviewer（doc_governance / review） |

两者都是 Claude Code 的封装，通过 `--output-format stream-json` 输出结构化 JSON 供 orchestrator 解析。

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`scheduler/query.rs 模块化重构`
>
> 背景：
> `scheduler/query.rs` 目前有 1,787 行代码，混合了四类不同职责：任务查询（resolve_task_id, load_task_summary, list_tasks_impl, get_task_details_impl, delete_task_impl）、日志流（stream_task_logs_impl, tail_lines）、Watch 实时监控（StepWatchInfo, load_task_detail_snapshot, render_watch_frame, emit_anomaly_warning, WatchAnomalyCounts）、以及格式化工具函数（colorize_status, format_duration, format_bytes）。这违反了单一职责原则，且与 task_repository 刚完成的模块化重构（从单文件拆分为 command_run.rs, items.rs, queries.rs, state.rs 等子模块）形成风格不一致。
>
> 本轮任务目标：
> 将 `core/src/scheduler/query.rs` 拆分为 `core/src/scheduler/query/` 目录结构，按职责域拆为独立子模块。参考 `core/src/task_repository/` 的拆分模式（mod.rs 做 re-export，各子模块各司其职）。拆分后所有现有公共 API（pub fn）签名和语义保持不变，`mod.rs` 统一 re-export，对外调用者无感知。
>
> 约束：
> 1. 优先解决根因，不接受仅做表面绕过。
> 2. 保留已有核心语义、兼容性要求、关键事件或状态行为：所有 `pub fn` 和 `pub(crate) fn` 的函数签名、参数、返回值保持不变；现有的 retry 机制（retry_query）和瞬态错误判定（is_transient_query_error）逻辑不变；scheduler 模块的 `mod.rs` 中对 query 的引用路径在拆分后仍能正常编译。
> 3. 最终目标是：`query.rs` 被替换为 `query/` 目录，内含 `mod.rs`（re-export）、`task_queries.rs`（任务增删查）、`log_stream.rs`（日志流和 tail）、`watch.rs`（Watch 实时监控渲染）、`format.rs`（格式化工具函数），所有 890+ 现有单元测试通过，`cargo clippy` 无新警告。

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
  -n "scheduler-query-modularize" \
  -w self -W self-bootstrap \
  --no-start \
  -g "将 core/src/scheduler/query.rs（1787行）拆分为 core/src/scheduler/query/ 目录结构。按职责域拆为子模块：task_queries.rs（任务增删查：resolve_task_id, load_task_summary, list_tasks_impl, get_task_details_impl, delete_task_impl）、log_stream.rs（日志流：stream_task_logs_impl, tail_lines）、watch.rs（Watch 实时监控：StepWatchInfo, load_task_detail_snapshot, render_watch_frame, emit_anomaly_warning, WatchAnomalyCounts）、format.rs（格式化工具：colorize_status, format_duration, format_bytes）。mod.rs 统一 re-export 所有 pub fn，对外调用者无感知。参考 core/src/task_repository/ 的拆分模式。retry_query 和 is_transient_query_error 作为内部共享工具放在 mod.rs 或独立的 retry.rs 中。所有现有测试通过，cargo clippy 无新警告。" \
  -t core/src/scheduler/query.rs
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

1. `plan` 是否正确理解"按职责域拆分 query.rs 为目录结构"
2. `implement` 是否在创建 query/ 目录并拆分文件，而不是仅做表面改动
3. `self_test` 是否仍能发挥自举安全闸门作用（cargo check + cargo test 通过）
4. `qa_testing` / `ticket_fix` 是否发现并回收回归问题
5. 日志中各步骤的输出是否能定位卡住或偏题发生在哪一段

### 4.3 进程监控

```bash
ps aux | grep -E "opencode|agent-orchestrator" | grep -v grep
git diff --stat
```

重点观察：

1. agent 进程是否仍在推进，而不是僵死
2. `git diff --stat` 是否持续有合理变化（应当看到 query.rs 删除、query/ 目录新增）
3. 若长时间零输出、零 diff、进程常驻不前，则记录为疑似卡住

### 4.4 补充诊断命令

当需要更细粒度观察时，人工可以补充使用：

```bash
./scripts/orchestrator.sh task trace <task_id> --json
./scripts/orchestrator.sh task watch <task_id>
sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id = '<task_id>' ORDER BY id DESC LIMIT 20;"
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

1. 根因是 query.rs 职责混杂（查询、日志流、Watch 渲染、格式化工具混在 1787 行的单文件中）
2. 完成态是 query.rs → query/ 目录，含 mod.rs + 4 个子模块，所有 pub API 通过 re-export 保持不变
3. 必须保留：所有 pub fn 签名不变、retry 机制不变、scheduler/mod.rs 编译路径不变

如果 plan 明显偏题，或把课题降级成表面修补（如仅添加注释或移动少量代码），应判定为偏题。

### 5.2 Implement 阶段检查点

确认代码改动至少满足以下全部条件：

1. `core/src/scheduler/query.rs` 被替换为 `core/src/scheduler/query/` 目录
2. 目录内含 `mod.rs` 和至少 3 个子模块文件
3. `mod.rs` 通过 `pub use` 或 `pub mod` re-export 所有原有公共接口
4. 无公共 API 签名变更

如果改动只发生在外围文档或配置，且未触及 query.rs 本身，应判定为不满足目标。

### 5.3 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. `cargo check` 和 `cargo test` 闸门未被绕过
3. 本轮改动未破坏基本自举安全性（编译通过 + 现有测试通过）

### 5.4 Validation 阶段检查点

Cycle 2 中重点观察：

1. `qa_testing` 是否产出回归 ticket
2. `ticket_fix` 是否回收新问题
3. `align_tests` 是否补齐单测
4. `doc_governance` 是否未引入文档漂移

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. 核心修复不是表面绕过，而是真正将 query.rs 拆分为目录结构。
3. 关键完成态达成：
   - `core/src/scheduler/query/` 目录存在且包含 `mod.rs` + 至少 3 个子模块
   - 原 `query.rs` 单文件已不存在
   - 所有 890+ 现有单元测试通过
   - `cargo clippy` 无新警告
   - 对 query 模块的外部调用路径（`scheduler::query::*`）保持不变
4. `self_test` 仍能作为 builtin 正常执行。
5. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止"仅监控"模式并记录异常：

1. `plan` 明显偏题（如理解为"添加新查询功能"而非"结构重构"）
2. `implement` 长时间无输出、无代码变更
3. `self_test` 失效或被绕过
4. `qa_testing` 持续产生同类 ticket，进入无效循环
5. 编译错误反复出现（如循环依赖、re-export 遗漏）

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
