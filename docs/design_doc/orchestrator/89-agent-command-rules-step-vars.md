# Design Doc 89: Agent Command Rules + Step Vars

## Origin

FR-084 — Agent 条件命令规则 + Session 复用

## Problem

Agent `command` 是单一字符串，无法根据运行时状态选择不同命令。Pipeline variables 全局累积，无法在步骤级别实现变量隔离。

典型场景：Claude Code session 复用需要首步 `--session-id` 创建 session，后续步骤 `--resume` 续接，但 QA 步骤需要独立 session 避免先入为主。

## Design Decisions

### 1. Agent `command_rules` — CEL 条件命令选择

在 `AgentConfig`/`AgentSpec` 上增加 `command_rules: Vec<AgentCommandRule>`，每条规则包含 `when` (CEL) + `command` (模板)。

**评估时机：** agent 选择后、命令渲染前（`run_phase_with_rotation` 中）。

**评估上下文：** 复用 `StepPrehookContext`，pipeline vars 通过 `vars` map 注入 CEL。

**匹配语义：** 按序评估，首个 true 生效；全部不匹配回退默认 `command`。CEL 评估失败（语法错误等）跳过该规则并 warn，不中断执行。

**审计：** 匹配的 rule index 记录在 `command_runs.command_rule_index`（NULL = 默认命令）。

### 2. Step `step_vars` — 步骤级临时变量覆盖

在 `WorkflowStepSpec`/`WorkflowStepConfig`/`TaskExecutionStep` 上增加 `step_vars: Option<HashMap<String, String>>`。

**语义：** 步骤执行前，将 `step_vars` 合并到 pipeline vars 的浅拷贝中。步骤执行后，恢复原始值。

**实现：**
- `apply_step_vars_overlay()` — 创建覆盖副本，保存原始值
- `restore_step_vars_overlay()` — 执行后恢复
- 插入点：`execute_agent_step()` 中 `BuiltinStepContext` 构造前

**关键约束：** `step_vars` 只影响当前步骤的输入视图；`behavior.captures` 仍写入全局 pipeline vars（输出不受影响）。

### 3. 为什么不用 `session_group`

最初考虑在框架层引入 `session_group` 概念，但：
- 增加框架复杂度
- 用户失去控制权
- Session 复用只是 `command_rules` + `step_vars` 的一个应用场景

当前方案零新概念，完全由 workflow YAML 编排。

## Key Files

- `crates/orchestrator-config/src/config/agent.rs` — `AgentCommandRule`, `command_rules` 字段
- `crates/orchestrator-config/src/config/execution.rs` — `step_vars` on `TaskExecutionStep`
- `crates/orchestrator-config/src/config/workflow.rs` — `step_vars` on `WorkflowStepConfig`
- `crates/orchestrator-config/src/cli_types.rs` — `command_rules` on `AgentSpec`, `step_vars` on `WorkflowStepSpec`
- `core/src/selection.rs` — 返回 `command_rules` from agent selection
- `crates/orchestrator-scheduler/src/scheduler/phase_runner/mod.rs` — `resolve_agent_command()`
- `crates/orchestrator-scheduler/src/scheduler/item_executor/dispatch.rs` — `apply_step_vars_overlay()`, `restore_step_vars_overlay()`
- `core/src/prehook/mod.rs` — `validate_agent_command_rules()`
- `core/src/persistence/migration_steps.rs` — m0021 `command_rule_index` column
