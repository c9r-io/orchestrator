# Task Trace 添加 Build Version 元数据

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`Task Trace 添加 Build Version 元数据`
>
> 背景：
> `task trace` 输出（JSON 和终端渲染）目前不包含构建版本信息。当跨版本调试、
> 对比不同 binary 产出的 trace 时，无法判断 trace 来自哪个构建。项目已在
> `build.rs` 中生成 `BUILD_GIT_HASH` 和 `BUILD_TIMESTAMP` 编译时常量，并在
> `Commands::Version` 中使用，但 `TaskTrace` 结构体未包含这些信息。
>
> 本轮任务目标：
> 1. 在 `core/src/scheduler/trace/model.rs` 的 `TaskTrace` 结构体中添加
>    `build_version` 字段（包含 version、git_hash、build_timestamp）。
> 2. 在 `core/src/scheduler/trace/builder.rs` 的 `build_trace_with_meta` 中
>    填充构建版本信息（从编译时环境变量读取）。
> 3. 在 `core/src/scheduler/trace/render.rs` 的终端渲染中，在 header 区域
>    显示构建版本（紧跟在 task status 行之后）。
> 4. 确保 JSON 输出（`task trace --json`）自动包含 build_version 字段
>    （通过 Serialize derive 自动实现）。
> 5. 更新现有测试以适配新字段。
>
> 约束：
> 1. 只改 trace 模块内的文件，不扩散到其他模块。
> 2. 保留 `TaskTrace` 已有字段和语义不变。
> 3. 编译时常量使用 `env!()` 宏，与 `main.rs` 中 `Commands::Version` 保持一致。
> 4. 最终目标是：`task trace` 的输出（终端和 JSON）包含构建版本、git hash、
>    构建时间戳，便于跨版本溯源。

### 1.1 预期产出

由 orchestrator 自主产出并落地：

1. 一份实现计划（由 `plan` 步骤生成）。
2. 必要的 QA 文档更新（由 `qa_doc_gen` 判断是否生成/更新）。
3. 与课题目标对应的代码改动。
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

**本课题的验证意义**：本课题是首次使用 MiniMax 低成本代理 API 执行 self-bootstrap，
同时验证 SecretStore 环境变量注入机制是否正确工作。课题本身改动集中在 trace 模块，
风险较低，适合作为 MiniMax API 的首次端到端验证。

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

```bash
./scripts/orchestrator.sh task create \
  -n "trace-build-version" \
  -w self -W self-bootstrap \
  --no-start \
  -g "Task Trace 添加 Build Version 元数据。在 core/src/scheduler/trace/model.rs 的 TaskTrace 结构体中添加 build_version 字段（version、git_hash、build_timestamp），在 builder.rs 的 build_trace_with_meta 中从编译时环境变量填充，在 render.rs 的终端渲染 header 中显示构建版本，JSON 输出通过 Serialize 自动包含。只改 trace 模块内文件，保留已有字段语义不变，编译时常量使用 env!() 宏与 main.rs Commands::Version 保持一致。"
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

1. `plan` 是否正确理解课题目标（改 trace 模块添加 build version）
2. `implement` 是否只改 trace 模块内文件，不扩散到其他模块
3. `self_test` 是否仍能发挥自举安全闸门作用
4. MiniMax API 是否正常响应（关注是否有 API 错误或超时）
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

1. 根因是：`TaskTrace` 缺少构建版本元数据，无法跨版本溯源
2. 完成态是：trace 输出（终端 + JSON）包含 version、git_hash、build_timestamp
3. 必须保留：已有 `TaskTrace` 字段和语义不变，只在 trace 模块内改动

如果 plan 明显偏题，或把课题扩展到 trace 模块之外，应判定为偏题。

### 5.2 Implement 阶段检查点

确认改动至少满足：

1. `model.rs` 中 `TaskTrace` 有新的 build version 相关字段
2. `builder.rs` 中 `build_trace_with_meta` 填充了构建信息
3. `render.rs` 终端渲染包含版本信息
4. 改动范围限制在 `core/src/scheduler/trace/` 内

如果改动散落在无关模块，应判定为范围失控。

### 5.3 Self-Restart 阶段检查点

确认执行证据表明：

1. `self_restart` 在 Cycle 1 的 `self_test` 之后执行
2. 进程以 exit code 75 退出，orchestrator.sh 自动重启
3. 新进程成功接管 `restart_pending` 任务并进入 Cycle 2

### 5.4 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. 编译和测试闸门未被绕过
3. 本轮改动未破坏基本自举安全性

### 5.5 MiniMax API 验证检查点（本课题附加验证点）

首次使用 MiniMax 代理 API，重点观察：

1. SecretStore 环境变量是否正确注入到 agent 命令中
2. MiniMax API 是否正常响应、延迟是否可接受
3. 模型输出质量是否足以完成 plan/implement/qa_testing 等步骤
4. 如果 API 失败，确认是 API 问题还是 SecretStore 注入问题

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. 核心修复不是表面绕过，而是正确添加了 build version 元数据。
3. 关键完成态达成：
   - `task trace` 终端输出包含构建版本信息
   - `task trace --json` 输出包含 `build_version` 字段
   - 改动限制在 `core/src/scheduler/trace/` 模块内
   - 现有测试通过，新字段有测试覆盖
4. `self_test` 仍能作为 builtin 正常执行。
5. self-restart 流程正常完成（exit 75 → 重启 → 接管 → Cycle 2）。
6. MiniMax API 全程正常工作，SecretStore 注入无误。
7. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止"仅监控"模式并记录异常：

1. `plan` 明显偏题
2. `implement` 长时间无输出、无代码变更
3. `self_test` 失效或被绕过
4. MiniMax API 持续报错或超时
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

## 附录：本课题对 MiniMax API 集成的验证意义

这是首次使用 MiniMax 低成本代理 API（通过 SecretStore 注入）执行完整的 self-bootstrap 流程。
本课题改动范围集中、风险较低，适合作为 API 切换的首次端到端验证：

- **如果成功**：证明 SecretStore 环境变量注入正确工作，MiniMax API 能力足以完成 plan/implement/qa 全链路
- **如果 API 失败**：可以快速定位是 SecretStore 注入问题还是 MiniMax API 能力不足
- **如果质量不足**：记录具体步骤和失败模式，为后续调整模型选择提供依据
