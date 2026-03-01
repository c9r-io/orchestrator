# self-bootstrap 课题执行计划：运行策略改为安全优先

本文档基于 `docs/plan/self-bootstrap-execution-template.md` 生成，用于把“默认运行策略改为安全优先”这一治理课题直接交给 orchestrator 的 `self-bootstrap` workflow 执行。人工只负责启动、监控、记录，并在异常时介入。

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`运行策略改为安全优先`
>
> 背景：
> 当前 orchestrator 的 `RunnerConfig` 默认仍采用 `Legacy` 模式，只有显式配置时才进入 `Allowlist`。这会让系统在默认安装、默认初始化、默认资源导入的情况下仍以宽松策略执行 agent shell 命令，与“安全优先”的平台定位不一致，也会放大误配置和误用风险。
>
> 本轮任务目标：
> 将默认运行策略调整为 `Allowlist`，把 `Legacy` 明确降级为需要显式声明的兼容模式；同时补齐配置导入/导出、校验、默认资源、CLI/QA 文档与测试，确保现有合法场景仍可通过显式配置继续使用 `Legacy`，但系统默认行为已经转为安全优先。
>
> 约束：
> 1. 优先解决根因，不接受仅做表面绕过。
> 2. 保留已有核心语义、兼容性要求、关键事件或状态行为：`仍然支持显式声明 Legacy；现有 allowlist 字段语义不变；任务执行、日志落库、红线校验、事件行为不被破坏`
> 3. 最终目标是：`默认创建/默认反序列化得到的运行策略为 Allowlist，Legacy 只在用户显式配置 legacy 时生效，相关测试与文档全部更新并通过`

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
  -n "self-bootstrap-security-default-allowlist" \
  -w self -W self-bootstrap \
  --no-start \
  -g "课题名称：运行策略改为安全优先；背景：当前 RunnerConfig 默认仍为 Legacy，默认执行面过宽，与安全优先定位不一致；本轮任务目标：把默认运行策略改为 Allowlist，并让 Legacy 仅在显式声明时生效，同时补齐配置、校验、文档与测试；约束：必须保留显式 Legacy 兼容路径，不能破坏任务执行、日志、事件和已有 allowlist 字段语义；最终目标：默认行为为 Allowlist，Legacy 成为显式降级模式，相关测试与文档更新并通过。" \
  -t core/src
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
./scripts/orchestrator.sh task logs <task_id> --tail 100
./scripts/orchestrator.sh task logs <task_id> --tail 100 --step implement
```

重点观察：

1. `plan` 是否明确识别“默认值不安全”是根因，而不是只加文档提醒
2. `implement` 是否覆盖默认值、配置转换、校验、测试与文档，而不是只改一个常量
3. `self_test` 是否仍能发挥自举安全闸门作用
4. `qa_testing` / `ticket_fix` 是否发现并回收兼容性回归
5. 分步骤日志是否能定位卡住或偏题发生在哪一段

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
./scripts/orchestrator.sh task trace <task_id> -o json
./scripts/orchestrator.sh debug task <task_id>
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

1. 根因是默认安全基线过宽，而不是单个配置文案问题
2. 完成态是默认 `Allowlist` + 显式 `Legacy`
3. 必须保留显式兼容路径和既有运行时语义

如果 plan 明显偏题，或把课题降级成“仅补文档提醒”，应判定为偏题。

### 5.2 Implement 阶段检查点

确认代码改动至少满足以下其一：

1. 修改默认 runner policy 与相关默认资源生成逻辑
2. 补齐 config/resource/CLI round-trip 的兼容测试
3. 更新 QA 与使用文档，明确 Legacy 是显式降级模式

如果改动只发生在文档或只改单一默认值而未处理兼容边界，应判定为不满足目标。

### 5.3 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. 编译和测试闸门未被绕过
3. 本轮改动未破坏基本自举安全性

### 5.4 Validation 阶段检查点

Cycle 2 中重点观察：

1. `qa_testing` 是否覆盖默认初始化和显式 Legacy 两条路径
2. `ticket_fix` 是否修复因默认值变更带来的回归
3. `align_tests` 是否补齐单测
4. `doc_governance` 是否同步文档口径，避免旧文档继续宣称默认 Legacy

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. 核心修复不是表面绕过，而是把默认安全基线从 Legacy 调整为 Allowlist。
3. 关键完成态达成：`默认创建/默认配置反序列化得到的 runner policy 为 Allowlist；只有显式声明 legacy 时才落到 Legacy；相关测试与文档全部更新并通过`
4. `self_test` 仍能作为 builtin 正常执行。
5. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止“仅监控”模式并记录异常：

1. `plan` 把课题错误降级为文档修订
2. `implement` 未覆盖默认值、兼容路径和测试中的至少两类
3. `self_test` 失效或被绕过
4. `qa_testing` 持续暴露默认初始化失败或显式 Legacy 回归，进入无效循环

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

人工不提前替 orchestrator 写实现计划，不预设代码改法，不把任务拆成手工子步骤。本计划的目的，是以稳定的 self-bootstrap 执行方式验证：当前 orchestrator 是否已经能围绕“默认安全基线前移”这一明确目标，自主完成治理课题。
