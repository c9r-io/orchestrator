# Design Doc 63: Workflow YAML 步骤定义未知字段警告 (FR-051)

## Overview

在 `orchestrator apply` 阶段对 workflow step 定义中的未知 YAML 字段发出警告，并对 CEL prehook 引用未声明 capture 变量的情况进行静态预警。

## Motivation

`TaskExecutionStep` / `WorkflowStepSpec` 使用 `#[serde(default)]` 属性，导致未知字段（如将 `capture:` 写在 step 层级而非 `behavior.captures` 下）被静默忽略。这使得 pipeline variable 丢失且下游 CEL prehook 报出不直观的 "Undeclared reference" 错误，极难排查。

## Design

### 1. 未知字段捕获

在 `WorkflowStepSpec` 上添加 `#[serde(flatten)]` 字段：

```rust
#[serde(flatten, default, skip_serializing)]
pub extra: HashMap<String, serde_yml::Value>,
```

serde 将所有不匹配已知字段的 YAML key 收集到 `extra` 中，而非静默丢弃。

### 2. Apply-time 警告收集

新增 `collect_step_warnings(steps, workflow_id) -> Vec<String>`（`core/src/config_load/validate/workflow_steps.rs`），执行两项检查：

**2a. 未知字段检测 + "did you mean" 提示**

遍历每个 step 的 `extra` keys，对高频误用给出建议：
- `capture` / `captures` → `behavior.captures`
- `on_failure` → `behavior.on_failure`
- `on_success` → `behavior.on_success`
- `post_actions` → `behavior.post_actions`
- `execution` → `behavior.execution`
- `collect_artifacts` → `behavior.collect_artifacts`

**2b. CEL prehook 变量交叉检查**

按步骤顺序累积 `captured_vars: HashSet`（来自 `behavior.captures[].var`），对每个 prehook `when` 表达式做词法扫描提取标识符，过滤掉 28 个内建变量和 CEL 关键字后，未匹配的标识符触发警告。

### 3. Proto 扩展

`ApplyResponse` 新增 `repeated string warnings = 4`，与 `errors` 分开传输。警告不阻止 apply 成功，不影响退出码。

### 4. CLI 显示

CLI 在打印 apply 结果后、错误处理前输出 `Warning: ...` 到 stderr。

## 受影响文件

- `proto/orchestrator.proto` — ApplyResponse 新增 warnings
- `crates/orchestrator-config/src/cli_types.rs` — WorkflowStepSpec 新增 extra
- `core/src/config_load/validate/workflow_steps.rs` — collect_step_warnings()
- `core/src/resource/workflow.rs` — WorkflowResource::collect_warnings()
- `core/src/service/resource.rs` — apply_manifests() 收集警告
- `crates/cli/src/commands/resource.rs` — CLI 显示警告

## 测试

- 6 个单元测试覆盖：未知字段+建议、未知字段无建议、干净步骤、未捕获变量警告、已捕获变量无警告、YAML 解析端到端
