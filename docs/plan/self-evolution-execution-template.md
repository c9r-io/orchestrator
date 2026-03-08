# self-evolution 课题执行计划模板

本文档是通用模板，用于把某个课题交给 orchestrator 的 `self-evolution` workflow 执行。与 `self-bootstrap` 的线性迭代不同，self-evolution 使用 WP03 动态候选生成 + 竞争选择来探索多条实现路径，由引擎自动选出最优方案。

适用场景：
- 实现路径不唯一，希望通过竞争比较选出最优解
- 课题范围适中（1-5 个文件），适合 2 个候选方案的 A/B 对比
- 有客观可量化的评估指标（编译/测试/clippy/diff 大小）

不适用场景：
- 课题范围极大，单个候选方案就需要多轮迭代才能完成（用 self-bootstrap）
- 实现路径明确唯一，竞争无意义（用 self-bootstrap）
- 需要完整 QA 文档治理和 ticket 回收（用 self-bootstrap，self-evolution 省略了这些步骤）

建议参考历史实例：

1. [`docs/plan/self-evolution-execution.md`](/Volumes/Yotta/ai_native_sdlc/docs/plan/self-evolution-execution.md)（首次实测课题）
2. [`docs/plan/self-bootstrap-execution-template.md`](/Volumes/Yotta/ai_native_sdlc/docs/plan/self-bootstrap-execution-template.md)（对比：线性迭代模板）

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-evolution 的课题：

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

1. 两条竞争方案（由 `evo_plan` 步骤生成并通过 `generate_items` 注入为动态 item）。
2. 每条方案各自实现（`evo_implement`，item-scoped）。
3. 每条方案的自动化评分（`evo_benchmark`，编译/测试/clippy/diff 大小）。
4. 引擎自动选出得分更高的方案（`select_best`，WP03 item_select）。
5. 胜出方案落地并通过最终验证（`evo_apply_winner` + `self_test`）。

### 1.2 非目标

本次不由人工预先定义实现细节；不预设哪条路径应该胜出；不在计划文档中替 orchestrator 指定具体代码改法。实现路径由 workflow 自主探索和竞争选择，人工只观察其是否偏离目标。

### 1.3 课题适配性自检

在使用本模板前，确认课题满足以下条件：

- [ ] 存在至少两种有实质差异的实现路径
- [ ] 改动范围可控（1-5 个文件），单个候选方案可在一次 agent 调用中完成
- [ ] 有客观可量化的比较维度（性能、代码量、正确性等）
- [ ] 现有测试足够充当回归保护，无需额外 QA 文档

---

## 2. 执行方式

本轮按 `self-evolution` 的标准链路执行：

```text
evo_plan ──[generate_items]──> evo_implement (x2) ──> evo_benchmark (x2) ──> select_best ──> evo_apply_winner ──> evo_align_tests ──> self_test ──> loop_guard
```

与 self-bootstrap 的关键差异：

| 维度 | self-bootstrap | self-evolution |
|------|---------------|----------------|
| 循环策略 | Fixed 2 cycles | Fixed 1 cycle |
| 实现路径 | 单一线性 | 2 候选竞争 |
| 选择机制 | 无 | WP03 item_select (max score) |
| 成本控制 | 多步骤多 agent | max_parallel=1, 无 QA/doc 步骤 |
| 安全保障 | self_test + self_restart | self_test + invariant (compilation_gate) |

> **成本说明**：self-evolution 通过单 cycle + 串行候选执行来控制成本。虽然有 2 个候选方案，
> 但 `max_parallel: 1` 确保不会同时运行多个 agent。总 agent 调用数约为 6 次
> （plan x1 + implement x2 + benchmark x2 + apply_winner x1），加上 builtin 步骤。
> 相比 self-bootstrap 的 2 cycle x 多步骤，成本相当或略低。

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
# 如需使用 Claude 原生 API，注释上行即可（claude-* 的模型配置将生效）
./scripts/orchestrator.sh apply -f docs/workflow/self-evolution.yaml
```

### 3.2 验证资源已加载

```bash
./scripts/orchestrator.sh get workspace
./scripts/orchestrator.sh get workflow
./scripts/orchestrator.sh get agent
```

预期至少可见：

1. workspace `self`
2. workflow `self-evolution`
3. agents `evo_architect`、`evo_coder`、`evo_reviewer`

### 3.3 创建任务（把目标交给 orchestrator）

self-evolution 不需要指定 `-t` 目标文件——动态 item 由 `evo_plan` 的 `generate_items` 在运行时生成，不依赖静态 QA 文件扫描。

```bash
./scripts/orchestrator.sh task create \
  -n "<任务名>" \
  -w self -W self-evolution \
  --no-start \
  -g "<将上方任务目标压缩成单行，直接作为 goal 传入>"
```

记录返回的 `<task_id>`，然后启动：

```bash
./scripts/orchestrator.sh task start <task_id>
```

---

## 4. 监控方法

### 4.1 状态监控

```bash
./scripts/orchestrator.sh task list
./scripts/orchestrator.sh task info <task_id> -o json
./scripts/orchestrator.sh task trace <task_id>
```

重点观察：

1. 当前步骤（特别注意 item-scoped 步骤的 fan-out 状态）
2. task status 是否前进
3. 是否出现 `failed`、`blocked`、长时间无进展

### 4.2 进化过程关键事件

self-evolution 相比 self-bootstrap 有以下特有的观察点：

1. **`items_generated` 事件**：确认 `evo_plan` 成功生成了候选 item
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT payload_json FROM events WHERE task_id='<task_id>' AND event_type='items_generated';"
   ```

2. **动态 item 状态**：确认候选都被执行
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT id, label, source, status FROM task_items WHERE task_id='<task_id>';"
   ```

3. **选择结果**：确认 item_select 选出了胜者
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT value_json FROM workflow_store_entries WHERE store_name='evolution' AND key='winner_latest';"
   ```

### 4.3 日志监控

```bash
./scripts/orchestrator.sh task logs --tail 100 <task_id>
./scripts/orchestrator.sh task logs --tail 200 <task_id>
```

重点观察：

1. `evo_plan` 是否生成了有实质差异的方案（而非形式上的变体）
2. `evo_implement` 各 item 是否各自独立实现
3. `evo_benchmark` 评分是否基于客观指标、是否有区分度
4. `select_best` 是否选出了得分更高的方案
5. `evo_apply_winner` 是否干净地应用了胜出方案

### 4.4 进程监控

```bash
ps aux | grep -E "opencode|orchestratord" | grep -v grep
git diff --stat
```

重点观察：

1. agent 进程是否仍在推进
2. `git diff --stat` 是否持续有合理变化
3. 若长时间零输出、零 diff、进程常驻不前，则记录为疑似卡住

### 4.5 补充诊断命令

```bash
./scripts/orchestrator.sh task trace <task_id> --json
sqlite3 data/agent_orchestrator.db "SELECT event_type, payload_json FROM events WHERE task_id = '<task_id>' ORDER BY id DESC LIMIT 20;"
```

---

## 5. 关键检查点

### 5.1 evo_plan 阶段检查点

确认输出包含：

1. 2 个结构化候选方案（JSON 格式，包含 id/name/description/strategy）
2. 两个方案有实质性差异（不同算法、不同设计、不同取舍）
3. `items_generated` 事件已落库，item 数量正确

如果 `evo_plan` 输出非法 JSON 或候选方案实质相同，应判定为 prompt 分化引导不足。

### 5.2 evo_implement 阶段检查点

确认：

1. 两个 item 各自产生了代码变更
2. 变更范围与各自方案的 strategy 描述一致
3. 没有相互干扰（item-scoped 隔离正常工作）

### 5.3 evo_benchmark 阶段检查点

确认：

1. 两个 item 都有 score capture
2. 评分基于编译/测试/clippy 等客观指标
3. 分数有区分度（不是都给满分或都为零）

### 5.4 select_best 阶段检查点

确认：

1. `evolution.winner_latest` store entry 存在
2. 选出的是得分更高的方案
3. winner 数据包含方案 ID 和分数

### 5.5 evo_apply_winner + self_test 阶段检查点

确认：

1. 胜出方案的代码通过编译
2. 所有测试通过
3. `compilation_gate` invariant 未触发 halt
4. 目标中要求保留的行为仍然正常

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-evolution` pipeline，在 `loop_guard` 正常收口。
2. 确实生成了 2 个不同的候选方案并分别实现。
3. 引擎通过 `item_select` 选出了得分更高的方案。
4. 胜出方案的代码通过 `self_test` 和 `compilation_gate` invariant。
5. 关键完成态达成：`<在此填写课题的明确完成条件>`
6. `evolution.winner_latest` store 中记录了选择结果。
7. 本轮没有引入新的编译或测试回归。

---

## 7. 异常处理

### 7.1 进化特有的异常场景

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| evo_plan 未输出合法 JSON | `items_generated` 事件不存在 | 检查 prompt，可能需要调整 JSON 输出指令 |
| 两个候选方案实质相同 | 查看 item label 和 approach 变量 | prompt 分化引导不足，考虑在 goal 中明确要求差异维度 |
| 两个候选都编译失败 | benchmark score 都为 0 | invariant 会 halt，需人工分析课题是否过于复杂 |
| item_select 无法选出 winner | store entry 不存在 | 检查 score capture 是否正常工作 |
| evo_apply_winner 后测试回归 | self_test 失败 | evo_align_tests 应尝试修复；若仍失败则人工介入 |
| 候选方案超出课题范围 | diff 涉及非预期文件 | plan prompt 的范围约束不足，考虑在 goal 中增加 scope 限制 |

### 7.2 通用异常

若出现以下情况，人工应停止"仅监控"模式并记录异常：

1. `evo_plan` 明显偏题或无法生成结构化候选
2. `evo_implement` 长时间无输出、无代码变更
3. `self_test` 失效或被绕过
4. 进程僵死、零输出

建议记录方式：

```bash
./scripts/orchestrator.sh task info <task_id> -o json
./scripts/orchestrator.sh task logs --tail 200 <task_id>
git diff --stat
```

必要时再由人工接管分析。

---

## 8. 人工角色边界

本计划中，人工角色明确限定为：

1. 提供目标
2. 启动 workflow
3. 监控状态
4. 在异常时中断并记录

人工不提前替 orchestrator 写实现计划，不预设代码改法，不预判哪条路径应该胜出。

本模板的目的是复用一种稳定的执行方式来验证：当前 orchestrator 是否已经能围绕一个明确目标，通过竞争进化机制自主选出更优的实现方案。

---

## 9. 与 self-bootstrap 的选择指南

| 判断维度 | 选 self-evolution | 选 self-bootstrap |
|---------|------------------|-------------------|
| 实现路径 | 多条可行路径，需要比较 | 路径明确或唯一 |
| 课题范围 | 小到中（1-5 文件） | 中到大（不限） |
| 评估方式 | 可客观量化评分 | 需要 QA 场景验证 |
| 迭代需求 | 一次进化足够 | 需要多轮迭代打磨 |
| 文档治理 | 不需要 | 需要 QA/doc 同步更新 |
| 成本敏感度 | 中（2 候选 x 6 agent 调用） | 中（2 cycle x 多步骤） |
| 安全要求 | invariant 编译闸门足够 | 需要 self_restart 自举验证 |
