# FR-051: Workflow YAML 步骤定义未知字段警告

## 状态

| 项目 | 值 |
|------|-----|
| 编号 | FR-051 |
| 优先级 | P1 |
| 状态 | Open |
| 提出日期 | 2026-03-15 |

## 背景

在 self-bootstrap 测试中，`qa_doc_gen` 步骤的 `capture:` 声明被写在 step 层级（与 `behavior:` 平级），
而 Rust 结构体 `TaskExecutionStep` 只在 `behavior.captures` 中定义了该字段。
serde 在 `#[serde(default)]` 模式下对未知字段静默忽略，导致 capture 完全丢失。

后果：`regression_target_ids` 从未注入 pipeline_vars，qa_testing 的 CEL prehook 在所有 128 个
item 上抛出 `Undeclared reference to 'regression_target_ids'`，task 整体失败。

这类错误极难排查——YAML 语法正确、apply 成功、步骤正常运行，唯独 capture 静默丢失。

## 需求

### 必须 (P0)

1. **Workflow apply 阶段检测未知字段**
   在 `orchestrator apply -f self-bootstrap.yaml` 解析 workflow step 定义时，
   对不属于 `TaskExecutionStep` 或 `WorkflowStepSpec` 的 YAML key 发出警告。
   例如：
   ```
   warning: step 'qa_doc_gen' contains unknown field 'capture' (did you mean 'behavior.captures'?)
   ```

2. **常见拼写纠正提示**
   对以下高频误用给出明确的 "did you mean" 提示：
   - `capture` → `behavior.captures`
   - `on_failure` (step-level) → `behavior.on_failure`
   - `post_actions` (step-level) → `behavior.post_actions`

### 应该 (P1)

3. **CEL prehook 引用的 capture 变量不存在时发出预警**
   在 workflow apply 阶段，静态分析每个步骤的 prehook `when` 表达式中引用的变量名，
   与所有前序步骤的 `behavior.captures[].var` 声明做交叉检查。
   如果 prehook 引用了一个从未被任何步骤 capture 的变量，发出警告：
   ```
   warning: step 'qa_testing' prehook references 'regression_target_ids' but no prior step captures this variable
   ```

### 可选 (P2)

4. **serde `deny_unknown_fields` 限定**
   评估是否可以对 `TaskExecutionStep` 和 `StepBehavior` 启用 `#[serde(deny_unknown_fields)]`。
   需确认不会与现有 YAML 中可能存在的注释/扩展字段冲突。

## 涉及文件

- `crates/orchestrator-config/src/config/execution.rs` — `TaskExecutionStep` 结构体
- `crates/orchestrator-config/src/cli_types.rs` — `WorkflowStepSpec` 结构体
- `core/src/service/resource.rs` 或 `crates/cli/src/commands/resource.rs` — apply 解析入口
- `core/src/prehook/cel.rs` — CEL 变量声明逻辑

## 验证标准

1. 将 `capture:` 写在 step 层级 → apply 时看到 warning
2. 将 `capture:` 改为 `behavior.captures:` → apply 无 warning，capture 正常工作
3. prehook 引用未声明的 capture 变量 → apply 时看到 warning
