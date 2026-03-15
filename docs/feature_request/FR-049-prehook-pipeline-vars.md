# FR-049: Prehook CEL 表达式接入 Pipeline Variables

## 概述

**优先级**: P1
**状态**: Proposed
**来源**: 自举工作流回归测试效率问题（ticket: `20260315-qa-regression-targets-not-constraining-qa-testing`）

在自举工作流（self-bootstrap）中，`qa_doc_gen` 步骤产出 `regression_targets` 列表用于缩窄后续 `qa_testing` 的执行范围。当前 `StepPrehookContext` 不包含 pipeline variables，导致 prehook CEL 表达式无法引用 `regression_targets`，所有 127 项 QA 文档全量执行（预期仅 5 项，约 5–10 分钟；实际 10+ 小时）。

---

## 现状分析

### 架构不对称

| CEL 上下文 | 是否含 pipeline vars | 使用场景 |
|------------|---------------------|----------|
| `ConvergenceContext` | ✅ `vars: HashMap<String, String>` | 循环收敛判定 |
| `StepPrehookContext` | ❌ | 步骤是否执行判定 |
| `ItemFinalizeContext` | ❌ | Item 状态终结判定 |

`ConvergenceContext`（`execution.rs:336-348`）已有 `vars` 字段，且 CEL 构建函数（`context.rs:419-438`）已实现自动类型推导注入（int64 → f64 → bool → string）。`StepPrehookContext` 缺少等价能力。

### 当前 prehook 上下文变量

`StepPrehookContext`（`execution.rs:259-333`）提供：
- 循环/任务状态：`cycle`、`max_cycles`、`is_last_cycle`、`task_id`、`task_item_id`
- QA/Ticket 状态：`qa_file_path`、`qa_exit_code`、`active_ticket_count`
- Fix/Build/Test 退出码与计数
- 安全标志：`self_referential_safe`、`last_sandbox_denied`

**不包含**：任何来自步骤捕获的 pipeline variables。

### 受影响工作流

`docs/workflow/self-bootstrap.yaml` 中的 `qa_testing` prehook：

```cel
is_last_cycle
  && qa_file_path.startsWith("docs/qa/")
  && qa_file_path.endsWith(".md")
  && self_referential_safe
```

此表达式无法引用 `regression_targets`，即使 `generate_items` post-action 已将其缓冲。

### 现有 generate_items 机制

`qa_doc_gen` 步骤的 `post_actions` 配置了 `generate_items`（`self-bootstrap.yaml:308-319`）：

```yaml
behavior:
  post_actions:
    - type: generate_items
      from_var: qa_doc_gen_output
      json_path: "$.regression_targets"
      mapping:
        item_id: "$.id"
        label: "$.name"
      replace: true
```

此机制在 task-scoped 段完成后执行（`segment.rs:184-190`），将静态 item 集替换为 `regression_targets` 子集。理论上，后续 `qa_testing` 只需遍历已缩窄的 item 集。

**问题**：当 `generate_items` 提取失败（JSON 解析错误、变量被截断/溢出、键缺失等），系统静默回退到全量 item 集，且 prehook 无法作为兜底过滤器。

---

## 需求描述

### 目标

1. 将 pipeline variables 注入 `StepPrehookContext`，使 prehook CEL 表达式可引用步骤捕获的变量
2. 提供与 `ConvergenceContext` 一致的自动类型推导注入
3. 支持列表类型变量的 `in` / `contains` 操作，使 prehook 可按列表过滤 item

### 非目标

- 不改变 `generate_items` post-action 的执行时序
- 不改变 prehook 与 finalize 的职责边界
- 不增加新的持久化层

---

## 实现方案

### Phase 1：StepPrehookContext 注入 pipeline vars

#### 1.1 扩展 StepPrehookContext

**文件**: `crates/orchestrator-config/src/config/execution.rs`

在 `StepPrehookContext` 末尾增加字段：

```rust
/// User-defined pipeline variables (from step captures).
/// Available in prehook CEL expressions with automatic type inference.
#[serde(default)]
pub vars: std::collections::HashMap<String, String>,
```

#### 1.2 填充 vars

**文件**: `crates/orchestrator-scheduler/src/scheduler/item_executor/accumulator.rs`

在 `to_prehook_context()` 中（line ~128），向 `StepPrehookContext` 构造体添加：

```rust
vars: self.pipeline_vars.vars.clone(),
```

注意：task-scoped 的 pipeline vars（如 `qa_doc_gen_output`）存储在 `TaskRuntimeContext.pipeline_vars` 中。需要将 `task_ctx.pipeline_vars.vars` 合并到 `self.pipeline_vars.vars`（item-scoped 的 accumulator 可能不包含 task-scoped 变量），确保 task-scoped 变量可见。

合并策略：item-scoped vars 覆盖同名 task-scoped vars（更具体的优先）。

```rust
vars: {
    let mut merged = task_ctx.pipeline_vars.vars.clone();
    merged.extend(self.pipeline_vars.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
    merged
},
```

#### 1.3 注入 CEL 上下文

**文件**: `core/src/prehook/context.rs`

在 `build_step_prehook_cel_context()` 末尾，复用 `build_convergence_cel_context` 中的类型推导逻辑，增加 pipeline vars 注入：

```rust
// Inject user-defined pipeline variables with type inference.
for (key, val) in &context.vars {
    if let Ok(i) = val.parse::<i64>() {
        cel_context.add_variable(key.as_str(), i).map_err(|e| anyhow!("{}: {}", err_msg, e))?;
    } else if let Ok(f) = val.parse::<f64>() {
        cel_context.add_variable(key.as_str(), f).map_err(|e| anyhow!("{}: {}", err_msg, e))?;
    } else if let Ok(b) = val.parse::<bool>() {
        cel_context.add_variable(key.as_str(), b).map_err(|e| anyhow!("{}: {}", err_msg, e))?;
    } else {
        cel_context.add_variable(key.as_str(), val.clone()).map_err(|e| anyhow!("{}: {}", err_msg, e))?;
    }
}
```

如果变量名与内建变量冲突（如 `cycle`、`step`），内建变量优先（先注入 vars，再注入内建变量以覆盖）。

#### 1.4 跳过溢出变量

Pipeline vars 可能包含大 JSON 值（被 spill 到磁盘后变量值为截断标记）。注入时跳过已截断的变量：

```rust
const TRUNCATION_MARKER: &str = "[truncated:";
for (key, val) in &context.vars {
    if val.starts_with(TRUNCATION_MARKER) {
        continue; // Skip spilled vars — too large for CEL evaluation
    }
    // ... type inference and injection
}
```

### Phase 2：列表类型 pipeline var 支持

Pipeline vars 本质是 `HashMap<String, String>`。当值为 JSON 数组字符串（如 `["docs/qa/00.md","docs/qa/01.md"]`），需解析为 CEL list 以支持 `in` 操作符。

在 Phase 1 的类型推导链中，增加 JSON 数组检测：

```rust
// Try JSON array → CEL list<string>
if val.starts_with('[') {
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(val) {
        cel_context.add_variable(key.as_str(), arr)
            .map_err(|e| anyhow!("{}: {}", err_msg, e))?;
        continue;
    }
}
```

使得 prehook 可写为：

```cel
is_last_cycle && qa_file_path in regression_target_ids && self_referential_safe
```

### Phase 3：自举工作流适配

#### 3.1 qa_doc_gen 步骤增加捕获声明

在 `self-bootstrap.yaml` 的 `qa_doc_gen` 步骤中增加 capture，将 `regression_targets` 提取为独立的列表变量：

```yaml
capture:
  - var: regression_target_ids
    source: stdout
    json_path: "$.regression_targets[*].id"
```

#### 3.2 更新 qa_testing prehook

```yaml
prehook:
  engine: cel
  when: >-
    is_last_cycle
    && qa_file_path.startsWith("docs/qa/")
    && qa_file_path.endsWith(".md")
    && self_referential_safe
    && (regression_target_ids == null || qa_file_path in regression_target_ids)
  reason: "QA deferred to final cycle; filtered by regression targets when available"
```

`regression_target_ids == null` 兜底：当变量不存在时（首次运行、capture 失败），回退到全量执行。

---

## 涉及文件

| 文件 | 变更 |
|------|------|
| `crates/orchestrator-config/src/config/execution.rs` | `StepPrehookContext` 增加 `vars` 字段 |
| `crates/orchestrator-scheduler/src/scheduler/item_executor/accumulator.rs` | `to_prehook_context()` 填充 `vars`（合并 task + item scoped） |
| `core/src/prehook/context.rs` | `build_step_prehook_cel_context()` 注入 pipeline vars |
| `docs/workflow/self-bootstrap.yaml` | qa_doc_gen capture + qa_testing prehook 更新 |
| `docs/guide/04-cel-prehooks.md` | 文档补充 pipeline vars 可用性说明 |

---

## 测试策略

1. **单元测试**: `StepPrehookContext` 构造后 `vars` 包含合并后的 task + item pipeline vars
2. **CEL 评估测试**: pipeline var 字符串/整数/布尔/JSON 数组值在 prehook 表达式中可正确引用
3. **集成测试**: 完整 apply → capture → prehook 流程验证 `in` 操作符过滤
4. **截断变量测试**: 溢出到磁盘的变量在 CEL 上下文中被跳过，不导致解析错误
5. **变量名冲突测试**: pipeline var 名称与内建变量同名时，内建变量优先

---

## 预期收益

| 指标 | 当前 | 预期 |
|------|------|------|
| 自举回归测试 item 数 | 127（全量） | 5（regression_targets） |
| 回归测试预计时长 | 10+ 小时 | 5–10 分钟 |
| generate_items 失败时的兜底 | 静默全量执行 | prehook 过滤 + 全量回退 |
