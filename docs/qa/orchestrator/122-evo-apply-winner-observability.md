# QA 122: evo_apply_winner 可观测性增强

验证 FR-070 四项可观测性需求的正确实现。

## 前置条件

- orchestrator 已编译（`cargo build`）
- `cargo test -p orchestrator-scheduler` 全部通过

## 场景

### S1: item_select 失败时发射 item_select_failed 事件

**步骤**:
1. 配置 workflow 包含 `item_select` 步骤，strategy 为 `max`，metric_var 为 `score`
2. 运行两个候选 item，其 benchmark 步骤 stdout 不包含可解析的 `score` 变量
3. 检查事件表

**期望**:
- 事件表包含 `item_select_failed` 事件
- payload 包含 `error`（含 "no items have parseable metric_var 'score'"）
- payload 包含 `metric_var: "score"`
- payload 包含 `item_count: 2`
- payload 包含 `item_vars`，列出各候选实际拥有的 pipeline var keys
- pipeline 继续执行（apply_winner 回退逻辑仍运行）

### S2: worktree 合并日志包含 diff 统计

**步骤**:
1. 运行包含 `item_isolation: git_worktree` 的 evolution workflow
2. 候选成功完成并产生代码变更
3. item_select 成功选出 winner
4. 检查 daemon 日志和事件表

**期望**:
- daemon 日志包含 `evo_apply_winner: applied winner worktree` INFO 条目
- 日志包含 `winner_branch`, `files_changed`, `insertions`, `deletions` 字段
- `item_isolation_winner_applied` 事件 payload 包含 `files_changed`, `insertions`, `deletions`
- `files_changed > 0`（确认 diff 统计不全为零）

### S3: capture 失败时 step_finished 包含 captures_missing

**步骤**:
1. 配置步骤 capture `var: score`，json_path 为 `$.total_score`
2. agent 输出不包含 `total_score` 字段的 JSON
3. 检查该步骤的 `step_finished` 事件

**期望**:
- `step_finished` 事件 payload 包含 `captures_missing: ["score"]`
- `score` pipeline var 被设为空字符串（现有行为不变）

### S4: item_selected 事件包含评分

**步骤**:
1. 运行两候选 item，各自 capture `score` 变量成功（如 85.0 和 72.0）
2. item_select 以 `max` strategy 选出 winner
3. 检查 `item_selected` 事件

**期望**:
- payload 包含 `selection_succeeded: true`
- payload 包含 `scores` 对象，key 为 item ID，value 为浮点评分
- `winner` 对应 scores 中最大值的 item

### S5: parse_numstat 单元测试

**步骤**:
1. `cargo test -p orchestrator-scheduler parse_numstat`

**期望**:
- `parse_numstat_basic` — 正确解析多行 numstat
- `parse_numstat_empty` — 空输入返回 (0, 0, 0)
- `parse_numstat_binary_files` — binary 文件行（`-\t-\t<file>`）不影响计数

### S6: apply_captures 缺失变量返回值

**步骤**:
1. `cargo test -p orchestrator-scheduler apply_captures_stdout_json_path_falls_back`

**期望**:
- 返回的 missing vec 包含 `"score"`
