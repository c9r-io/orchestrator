# 05 - 高级特性

本章涵盖高级工作流原语：自定义资源定义、持久化存储、任务派生、动态项和不变量约束。

## 自定义资源定义（CRD）

CRD 允许你在内置的 Workspace/Agent/Workflow/StepTemplate 之外定义新的资源类型。适用于领域特定的配置（提示词库、评估标准等）。

### 定义 CRD

```yaml
apiVersion: orchestrator.dev/v2
kind: CustomResourceDefinition
metadata:
  name: promptlibraries.extensions.orchestrator.dev
spec:
  kind: PromptLibrary
  plural: promptlibraries
  short_names: [pl]
  group: extensions.orchestrator.dev
  versions:
    - name: v1
      served: true
      schema:
        type: object
        required: [prompts]
        properties:
          prompts:
            type: array
            minItems: 1
            items:
              type: object
              required: [name, template]
              properties:
                name:
                  type: string
                template:
                  type: string
                tags:
                  type: array
                  items:
                    type: string
      cel_rules:
        - rule: "size(self.prompts) > 0"
          message: "至少需要一个提示词"
```

### 创建 CRD 实例

注册后，使用 CRD 的 `group/version` 作为 `apiVersion` 创建实例：

```yaml
apiVersion: extensions.orchestrator.dev/v1
kind: PromptLibrary
metadata:
  name: qa-prompts
  labels:
    team: platform
spec:
  prompts:
    - name: code-review
      template: "审查以下代码的 {criteria}..."
      tags: [qa, review]
```

### 管理 CRD

```bash
# 应用 CRD + 实例
./scripts/run-cli.sh apply -f crd-manifest.yaml

# 列出实例
./scripts/run-cli.sh get promptlibraries
./scripts/run-cli.sh get pl                    # 使用短名称

# 详情
./scripts/run-cli.sh describe promptlibrary qa-prompts

# 删除
./scripts/run-cli.sh delete promptlibrary qa-prompts

# 导出
./scripts/run-cli.sh manifest export           # 包含 CRD 资源
```

### CRD 验证

CRD 支持两级验证：
- **JSON Schema**：`schema` 定义结构验证（类型、必填字段、最小/最大值）
- **CEL 规则**：`cel_rules` 定义语义验证（跨字段约束）

## 持久化存储（WP01）

持久化存储通过 `WorkflowStore` CRD 提供跨任务记忆。数据在任务之间持久化，支持从历史运行中学习。

### 定义存储

```yaml
apiVersion: orchestrator.dev/v2
kind: WorkflowStore
metadata:
  name: context
spec:
  backend: local           # "local"（SQLite）或 "command"（shell 命令）
  schema:
    type: object
    properties:
      value:
        type: string
  retention:
    max_entries: 1000
    ttl_seconds: 86400      # 可选：24 小时后自动过期
```

### 从步骤读写

步骤通过 `store_inputs`、`store_outputs` 和 `post_actions` 与存储交互：

```yaml
steps:
  - id: plan
    scope: task
    enabled: true
    command: "echo '{\"confidence\":0.95}'"
    behavior:
      post_actions:
        - type: store_put
          store: context
          key: plan_result
          from_var: plan_output

  - id: implement
    scope: task
    enabled: true
    store_inputs:                # 执行前从存储读取
      - store: context
        key: plan_result
        as_var: inherited_plan
```

### CLI 操作

```bash
# 写入值
./scripts/run-cli.sh store put context my_key "my_value"

# 读取值
./scripts/run-cli.sh store get context my_key

# 列出键
./scripts/run-cli.sh store list context

# 删除键
./scripts/run-cli.sh store delete context my_key
```

## 任务派生（WP02）

步骤可以通过后置动作派生子任务，实现自主的工作分解。

### 派生单个任务

```yaml
- id: plan
  scope: task
  enabled: true
  behavior:
    post_actions:
      - type: spawn_task
        goal: "verify-changes"
        workflow: verify_workflow
```

### 派生多个任务

```yaml
- id: plan
  scope: task
  enabled: true
  behavior:
    post_actions:
      - type: spawn_tasks
        from_var: task_list        # 包含目标 JSON 数组的管道变量
        workflow: child_workflow
```

### 安全限制

任务派生受安全配置保护：

```yaml
safety:
  max_spawned_tasks: 10      # 每个父任务最大子任务数
  max_spawn_depth: 3         # 最大 父→子→孙 深度
  spawn_cooldown_seconds: 5  # 两次派生之间的最小秒数
```

## 动态项 + 选择（WP03）

工作流步骤可以在运行时动态生成任务项，并使用锦标赛式选择来挑选最佳候选者。

### 生成项

```yaml
- id: generate
  scope: task
  enabled: true
  behavior:
    post_actions:
      - type: generate_items
        from_var: candidates       # 包含 JSON 数组的管道变量
```

### 项选择

`item_select` 内置步骤使用可配置策略选择项：

```yaml
- id: select_best
  scope: task
  builtin: item_select
  enabled: true
  item_select_config:
    strategy: weighted              # min | max | threshold | weighted
    metric_key: quality_score       # 要比较的字段
    top_k: 3                        # 选择前 N 项
    threshold: 0.7                  # 最低分数（threshold 策略）
    weights:                        # 字段权重（weighted 策略）
      confidence: 0.4
      quality_score: 0.6
```

| 策略 | 说明 |
|------|------|
| `min` | 选择指标值最低的项 |
| `max` | 选择指标值最高的项 |
| `threshold` | 选择高于/低于阈值的项 |
| `weighted` | 按字段加权组合评分 |

## 不变量约束（WP04）

不变量是不可变的安全断言，工作流本身无法削弱。它们在任务启动时固定，由引擎强制执行。

```yaml
safety:
  invariants:
    - id: main_branch_exists
      description: "main 分支必须始终存在"
      check:
        command: "git branch --list main | wc -l"
        expect: "1"
      on_violation: abort           # abort | warn | rollback
      protected_files:              # 不可修改的文件
        - ".github/workflows/*"
        - "Cargo.lock"
      checkpoint_filter:            # 仅在特定步骤检查
        steps: [implement, self_test]
```

| on_violation | 行为 |
|-------------|------|
| `abort` | 立即停止任务 |
| `warn` | 记录警告但继续 |
| `rollback` | 恢复到上一个检查点 |

## 下一步

- [06 - 自引导](06-self-bootstrap.md) —— 自修改工作流和生存机制
- [04 - CEL 预钩子](04-cel-prehooks.md) —— 动态步骤门控
