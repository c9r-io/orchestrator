# self-evolution 课题执行计划

本文档是 `self-evolution` workflow 的首次实测课题。与 `self-bootstrap` 不同，self-evolution 使用 WP03 动态候选生成 + 竞争选择来探索多条实现路径，由引擎自动选出最优方案。

---

## 1. 任务目标

将下面这段目标原文直接传递给 orchestrator，作为本轮 self-evolution 的课题：

> 课题名称：`StepTemplate prompt 变量解析增强`
>
> 背景：
> 当前 StepTemplate 的 prompt 字段使用简单的字符串替换（`{var_name}`）来注入运行时变量。
> 这种方式存在以下问题：
> 1. 缺少未定义变量的检测——如果 prompt 引用了一个不存在的变量，替换后会保留原始占位符 `{var_name}`，agent 可能会困惑。
> 2. 没有条件段落——无法根据变量是否存在来包含/排除 prompt 的某个段落（例如"如果有 diff 则显示 diff 段落"）。
> 3. 没有默认值机制——变量不存在时无法回退到合理的默认值。
>
> 本轮任务目标：
> 增强 prompt 模板变量解析，支持以下语法：
> - `{var_name}` — 现有行为，未定义时产生警告日志
> - `{var_name:-default_value}` — 未定义时使用默认值
> - `{?var_name}...{/var_name}` — 条件段落，变量存在且非空时包含
>
> 约束：
> 1. 不引入外部模板引擎依赖（如 Tera、Handlebars），用纯 Rust 实现。
> 2. 保留现有 `{var_name}` 语法的完全向后兼容。
> 3. 最终目标是：所有现有 StepTemplate prompt 不需要任何修改即可正常工作；新语法是可选增强。

### 1.1 预期产出

由 orchestrator 自主产出：

1. 两条竞争方案（由 `evo_plan` 步骤生成并通过 `generate_items` 注入为动态 item）。
2. 每条方案各自实现（`evo_implement`，item-scoped 并行）。
3. 每条方案的自动化评分（`evo_benchmark`，编译/测试/clippy/diff 大小）。
4. 引擎自动选出得分更高的方案（`select_best`，WP03 item_select）。
5. 胜出方案落地并通过最终验证（`evo_apply_winner` + `self_test`）。

### 1.2 非目标

- 不预设哪条路径应该胜出。
- 不由人工指定具体代码实现方式。
- 不要求完整的 QA 文档生成（本轮聚焦于进化机制验证）。

### 1.3 课题选择理由

选择此课题作为 self-evolution 首次实测，基于以下考量：

1. **范围适中**：涉及 1-2 个文件（template resolution 模块），改动量可控，适合 2 个候选方案的对比。
2. **有明确的可比较维度**：正则方案 vs 手写 parser，两种路径在性能、可读性、正确性上有真实差异。
3. **可客观评分**：编译通过、测试通过、clippy 干净、diff 大小——都是可自动化量化的指标。
4. **向后兼容约束**：现有测试天然成为回归保护，不需要额外人工验证。
5. **自举相关**：改进 prompt 模板直接提升 orchestrator 自身的 agent 调用质量。

---

## 2. 执行方式

本轮按 `self-evolution` workflow 执行，pipeline 如下：

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

---

## 3. 启动步骤

### 3.1 构建并启动 daemon

C/S 架构下，CLI（`orchestrator`）通过 Unix Domain Socket 连接 daemon（`orchestratord`）。

```bash
cd /path/to/orchestrator

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
orchestrator delete project/self-evolution --force
orchestrator init
orchestrator apply -f docs/workflow/secrets.yaml --project self-evolution
orchestrator apply -f docs/workflow/secrets.yaml --project self-evolution
# ⚠️  必须使用 --project，否则真实 AI agent 会注册到全局空间
orchestrator apply -f docs/workflow/execution-profiles.yaml --project self-evolution
orchestrator apply -f docs/workflow/self-evolution.yaml --project self-evolution
```


### 3.3 验证资源已加载

project-only 部署下 `orchestrator get` 会因全局 defaults 为空报错，
改用 sqlite 直接验证：

```bash
sqlite3 data/agent_orchestrator.db \
  "SELECT json_group_array(key) FROM (
     SELECT key FROM json_each(
       (SELECT json_extract(config_json, '$.projects.\"self-evolution\".workspaces')
        FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1)
     )
   );"
# 预期: ["self"]

sqlite3 data/agent_orchestrator.db \
  "SELECT json_group_array(key) FROM (
     SELECT key FROM json_each(
       (SELECT json_extract(config_json, '$.projects.\"self-evolution\".workflows')
        FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1)
     )
   );"
# 预期: ["self-evolution"]

sqlite3 data/agent_orchestrator.db \
  "SELECT json_group_array(key) FROM (
     SELECT key FROM json_each(
       (SELECT json_extract(config_json, '$.projects.\"self-evolution\".agents')
        FROM orchestrator_config_versions ORDER BY id DESC LIMIT 1)
     )
   );"
# 预期: ["evo_architect","evo_coder","evo_reviewer"]
```

### 3.4 创建并启动任务

C/S 模式下 `task create` 会直接 enqueue 到 daemon worker，
任务创建即自动开始执行，不需要单独 `task start`。

```bash
orchestrator task create \
  -n "evo-prompt-template-enhance" \
  -w self -W self-evolution \
  --project self-evolution \
  -g "增强 StepTemplate prompt 变量解析：支持 {var:-default} 默认值语法和 {?var}...{/var} 条件段落语法。纯 Rust 实现，不引入外部模板引擎。保留现有 {var} 语法完全向后兼容。未定义变量产生 warn 日志而非静默保留占位符。"
```

记录返回的 `<task_id>`。任务会立即被 worker 认领并开始执行。
如需等待完成，请使用 `orchestrator task watch <task_id>` 或轮询 `task info`。

---

## 4. 监控方法

### 4.1 状态监控

```bash
orchestrator task list
orchestrator task info <task_id>
orchestrator task trace <task_id>    # 带异常检测的执行时间线
orchestrator task watch <task_id>    # 实时刷新状态面板
```

### 4.2 进化过程关键事件

除了常规的步骤监控外，self-evolution 有以下特有的观察点：

1. **`items_generated` 事件**：确认 `evo_plan` 成功生成了 2 个候选 item
   ```bash
   sqlite3 data/agent_orchestrator.db \
     "SELECT payload_json FROM events WHERE task_id='<task_id>' AND event_type='items_generated';"
   ```

2. **动态 item 状态**：确认两个候选都被执行
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
orchestrator task logs --tail 100 <task_id>
orchestrator task logs --tail 200 <task_id>
```

重点观察：

1. `evo_plan` 是否生成了两条有实质差异的方案
2. `evo_implement` 两个 item 是否各自独立实现
3. `evo_benchmark` 评分是否基于客观指标
4. `select_best` 是否选出了得分更高的方案
5. `evo_apply_winner` 是否干净地应用了胜出方案

### 4.4 进程 / daemon 监控

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

---

## 5. 关键检查点

### 5.1 evo_plan 阶段

确认输出包含：

1. 2 个结构化候选方案（JSON 格式）
2. 两个方案有实质性差异（例如：正则 vs 手写 parser）
3. `items_generated` 事件已落库

### 5.2 evo_implement 阶段

确认：

1. 两个 item 各自产生了代码变更
2. 变更范围与各自方案描述一致
3. 没有相互干扰（item-scoped 隔离）

### 5.3 evo_benchmark 阶段

确认：

1. 两个 item 都有 score capture
2. 评分基于编译/测试/clippy 等客观指标
3. 分数有区分度（不是都给满分）

### 5.4 select_best 阶段

确认：

1. `evolution.winner_latest` store entry 存在
2. 选出的是得分更高的方案
3. winner 数据包含方案 ID 和分数

### 5.5 evo_apply_winner + self_test 阶段

确认：

1. 胜出方案的代码通过编译
2. 所有测试通过
3. `compilation_gate` invariant 未触发 halt
4. 现有 StepTemplate prompt 行为不变（向后兼容）

---

## 6. 成功判定

当以下条件同时成立，可判定本轮课题完成：

1. orchestrator 完整跑完 `self-evolution` pipeline，在 `loop_guard` 正常收口。
2. 确实生成了 2 个不同的候选方案并分别实现。
3. 引擎通过 `item_select` 选出了得分更高的方案。
4. 胜出方案的代码通过 `self_test` 和 `compilation_gate` invariant。
5. 现有 `{var_name}` 替换语法向后兼容。
6. `evolution.winner_latest` store 中记录了选择结果。

---

## 7. 异常处理

### 7.1 进化特有的异常场景

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| evo_plan 未输出合法 JSON | `items_generated` 事件不存在 | 检查 prompt，可能需要调整 JSON 输出指令 |
| 两个候选方案实质相同 | 查看 item label 和 approach 变量 | 说明 prompt 分化引导不足 |
| 两个候选都编译失败 | benchmark score 都为 0 | invariant 会 halt，需人工分析 plan 质量 |
| item_select 无法选出 winner | store entry 不存在 | 检查 score capture 是否正常工作 |
| evo_apply_winner 后测试回归 | self_test 失败 | evo_align_tests 应尝试修复；若仍失败则人工介入 |

### 7.2 C/S 架构特有异常

| 异常 | 判断方式 | 处理 |
|------|---------|------|
| daemon 未运行 | CLI 报 `failed to connect to daemon at .../orchestrator.sock` | 用 `orchestratord --foreground --workers 2` 启动 |
| CLI 指向旧单体二进制 | `which orchestrator` 指向 `core/target/release/` | 更新 symlink 到 `target/release/orchestrator` |
| 重建后 daemon 仍用旧代码 | 观察到已修复的 bug 复现 | 杀掉旧 daemon 进程再启动新的 |
| task create 后任务立即开始 | task list 显示 `pending` 或很快变成 `running` | C/S 模式下 task lifecycle 为 queue-only，这是正常行为 |

### 7.3 通用异常

与 self-bootstrap 相同：记录状态、日志、diff，必要时人工接管。

---

## 8. 人工角色边界

与 self-bootstrap 一致：人工只负责启动、监控、判断、记录。

本轮的额外观察重点是**进化机制本身是否工作**：
- 候选生成是否产出有意义的分化
- 竞争评估是否基于客观指标
- 选择结果是否合理
- 整体 pipeline 是否比线性 self-bootstrap 产出更优质的代码

这些观察将用于判断 self-evolution workflow 是否值得在后续课题中替代或补充 self-bootstrap。

---

## 9. 收尾清理

任务完成后需清理 agent 产出的课题代码，以便同一课题可重复测试：

```bash
# 还原 agent 修改的所有文件（保留基础设施 bug fix）
git checkout HEAD -- Cargo.lock core/Cargo.toml \
  core/src/collab/context.rs core/src/collab/mod.rs \
  core/src/selection.rs crates/daemon/src/server.rs

# 删除 agent 创建的新文件
rm -f core/src/collab/template.rs

# 确认工作树干净
git status --short

# 验证编译
cargo check
```

> ⚠️ Agent 可能修改 `context.rs`、`lib.rs`、`Cargo.toml` 等核心文件。
> 每次执行后务必检查 `git diff --stat` 并还原非预期变更。
