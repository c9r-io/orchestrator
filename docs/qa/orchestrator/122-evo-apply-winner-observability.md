# QA 122: evo_apply_winner 可观测性增强

验证 FR-070 四项可观测性需求的正确实现。

## 前置条件

- orchestrator 已编译（`cargo build`）
- `cargo test -p orchestrator-scheduler` 全部通过

## 场景

### S1: item_select 失败时发射 item_select_failed 事件

**步骤**:
1. 运行 `execute_item_select` 单元测试验证 unparseable metric_var 错误逻辑：
   ```bash
   cargo test -p orchestrator-scheduler --lib test_unparseable_metric_var_fails -- --nocapture
   ```
2. 代码审查 — 确认 `item_select_failed` 事件发射逻辑：
   ```bash
   rg -n "item_select_failed" crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs
   ```

**期望**:
- 单元测试通过：`execute_item_select` 在无可解析 metric_var 时返回 "no items have parseable metric_var 'score'" 错误
- 代码审查确认 `segment.rs` 在 `execute_item_select` 失败时发射 `item_select_failed` 事件，payload 包含 `error`, `metric_var`, `item_count`, `item_vars`

### S2: worktree 合并日志包含 diff 统计

**步骤**:
1. 运行 `parse_numstat` 单元测试（已在 S5 验证）确认 diff 统计解析正确
2. 代码审查 — 确认 `evo_apply_winner` 日志和事件包含 diff 统计字段：
   ```bash
   rg -n "evo_apply_winner|files_changed|insertions|deletions|item_isolation_winner_applied" crates/orchestrator-scheduler/src/scheduler/loop_engine/isolation.rs
   ```

**期望**:
- 代码审查确认 `isolation.rs` 的 `evo_apply_winner` 路径：
  - 调用 `parse_numstat` 解析 `git diff --numstat` 输出
  - INFO 日志包含 `winner_branch`, `files_changed`, `insertions`, `deletions`
  - 发射 `item_isolation_winner_applied` 事件，payload 包含 diff 统计字段

### S3: capture 失败时 step_finished 包含 captures_missing

**步骤**:
1. 运行 `apply_captures` 单元测试（已在 S6 验证）确认 missing var 返回值
2. 代码审查 — 确认 `captures_missing` 写入 `step_finished` payload：
   ```bash
   rg -n "captures_missing" crates/orchestrator-scheduler/src/scheduler/item_executor/apply.rs
   ```

**期望**:
- S6 单元测试确认 `apply_captures` 在 JSON path 未找到时返回 missing vec 包含 `"score"`
- 代码审查确认 `apply.rs` 在 `captures_missing` 非空时将其写入 `step_finished` 事件 payload

### S4: item_selected 事件包含评分

**步骤**:
1. 运行 `execute_item_select` 单元测试验证 max strategy 选出最高分 winner：
   ```bash
   cargo test -p orchestrator-scheduler --lib test_select_max_picks_highest_score -- --nocapture
   ```
2. 代码审查 — 确认 `item_selected` 事件 payload 包含评分：
   ```bash
   rg -n "item_selected|selection_succeeded|scores" crates/orchestrator-scheduler/src/scheduler/loop_engine/segment.rs
   ```

**期望**:
- 单元测试通过：两个候选（85.0 和 72.0），winner 为最高分 item，eliminated 包含另一个
- 代码审查确认 `segment.rs` 发射 `item_selected` 事件，payload 包含 `selection_succeeded`, `scores`, `winner`

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

## Checklist

- [ ] S1: item_select 失败时发射 item_select_failed 事件
- [ ] S2: worktree 合并日志包含 diff 统计
- [ ] S3: capture 失败时 step_finished 包含 captures_missing
- [ ] S4: item_selected 事件包含评分
- [ ] S5: parse_numstat 单元测试
- [ ] S6: apply_captures 缺失变量返回值
