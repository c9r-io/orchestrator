# self-bootstrap 课题执行计划：拆分超大核心模块

本文档基于 `docs/plan/self-bootstrap-execution-template.md` 生成，用于把“拆分超大核心模块”这一治理课题直接交给 orchestrator 的 `self-bootstrap` workflow 执行。人工只负责启动、监控、记录，并在异常时介入。

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-bootstrap 的课题：

> 课题名称：`拆分超大核心模块`
>
> 背景：
> 当前核心代码中存在多个超大文件，尤其是 `config_load.rs`、`config.rs`、`dynamic_orchestration.rs`、`collab.rs`，已经同时承载规范化、兼容、自愈、数据结构、DAG、消息总线、artifact 解析等多类职责。这会持续抬高理解成本、修改风险和后续服务化改造成本。
>
> 本轮任务目标：
> 在不改变既有外部行为和核心语义的前提下，拆分超大核心模块，建立更清晰的职责边界和内部模块布局；优先切开 `config_load.rs`、`config.rs`、`dynamic_orchestration.rs`、`collab.rs`，将高耦合逻辑迁移到子模块，同时保持现有测试通过、CLI/trace/事件行为不回归。
>
> 约束：
> 1. 优先解决根因，不接受仅做表面绕过。
> 2. 保留已有核心语义、兼容性要求、关键事件或状态行为：`保留现有 CLI 契约、配置格式、步骤语义、trace 结构、message bus 对外语义、artifact 解析与测试覆盖`
> 3. 最终目标是：`超大文件被按职责拆分，模块边界更清晰，对外行为与测试结果保持稳定，文档同步说明新的内部结构`

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
  -n "self-bootstrap-core-module-split" \
  -w self -W self-bootstrap \
  --no-start \
  -g "课题名称：拆分超大核心模块；背景：config_load.rs、config.rs、dynamic_orchestration.rs、collab.rs 等文件体量过大且职责混杂，已形成维护和演进成本；本轮任务目标：在不改变外部行为的前提下按职责拆分这些超大模块，优先切开配置规范化/自愈、动态编排、消息总线与 artifact 相关逻辑，并保持测试通过；约束：必须保留 CLI 契约、配置格式、步骤语义、trace、事件和 message bus 对外行为；最终目标：超大文件完成高价值拆分，模块边界更清晰，测试与文档同步更新。" \
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

1. `plan` 是否明确给出“按职责切分”的治理思路，而不是机械搬文件
2. `implement` 是否优先拆高耦合逻辑并保持内部接口清晰，而不是只新增转发层
3. `self_test` 是否仍能发挥自举安全闸门作用
4. `qa_testing` / `ticket_fix` 是否能发现拆分后引入的行为回归
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

1. 根因是职责混杂和模块边界失衡，而不是单纯“文件太长”
2. 完成态是按职责拆分且行为不变
3. 哪些核心接口和外部语义必须保持稳定

如果 plan 明显偏题，或把课题降级成“仅拆文件名不拆职责”，应判定为偏题。

### 5.2 Implement 阶段检查点

确认代码改动至少满足以下其一：

1. 将配置规范化/自愈/校验等职责从超大文件中拆到子模块
2. 将动态编排或消息总线中的高耦合逻辑拆出独立子模块
3. 保持对外 API 稳定并补齐回归测试

如果改动只增加 `mod` 包装层、未实质降低耦合和理解成本，应判定为不满足目标。

### 5.3 Self-Test 阶段检查点

确认执行证据表明：

1. `self_test` 仍然执行
2. 编译和测试闸门未被绕过
3. 本轮改动未破坏基本自举安全性

### 5.4 Validation 阶段检查点

Cycle 2 中重点观察：

1. `qa_testing` 是否覆盖配置读取、动态编排、协作总线、trace 等受影响区域
2. `ticket_fix` 是否回收由模块拆分引入的接口回归
3. `align_tests` 是否补齐新的单测或迁移旧测试
4. `doc_governance` 是否同步说明新的内部模块结构，避免文档仍指向过时单文件结构

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-bootstrap` 流程，或在 `loop_guard` 正常收口。
2. 核心修复不是表面绕过，而是完成有价值的职责拆分并降低核心模块耦合。
3. 关键完成态达成：`至少对 config_load.rs、config.rs、dynamic_orchestration.rs、collab.rs 中的高耦合部分完成职责拆分或建立明确子模块边界；对外行为与测试保持稳定`
4. `self_test` 仍能作为 builtin 正常执行。
5. 本轮没有留下新的未解决 ticket；若有 ticket，必须由同一轮 `ticket_fix` 回收，或明确记录未收口原因。

---

## 7. 异常处理

若出现以下情况，人工应停止“仅监控”模式并记录异常：

1. `plan` 把课题误解为单纯移动文件
2. `implement` 大量改动但没有清晰职责边界改善
3. `self_test` 失效或被绕过
4. `qa_testing` 持续暴露同类回归，进入无效循环

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

人工不提前替 orchestrator 写实现计划，不预设代码改法，不把任务拆成手工子步骤。本计划的目的，是以稳定的 self-bootstrap 执行方式验证：当前 orchestrator 是否已经能围绕“核心模块治理与职责重构”这一明确目标，自主完成治理课题。
