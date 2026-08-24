# 05 - 高级特性

本章涵盖高级工作流原语：自定义资源定义、持久化存储、任务派生、动态项和不变量约束。

## 自定义资源定义（CRD）

CRD 允许你在内置类型（Workspace、Agent、Workflow、StepTemplate、ExecutionProfile、SecretStore、EnvStore、WorkflowStore、Trigger、RuntimePolicy）之外定义新的资源类型。适用于领域特定的配置（提示词库、评估标准等）。

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
orchestrator apply -f crd-manifest.yaml

# 列出实例
orchestrator get promptlibraries
orchestrator get pl                    # 使用短名称

# 详情
orchestrator describe promptlibrary qa-prompts

# 删除
orchestrator delete promptlibrary qa-prompts

# 导出
orchestrator manifest export           # 包含 CRD 资源
```

### CRD 验证

CRD 支持两级验证：
- **JSON Schema**：`schema` 定义结构验证（类型、必填字段、最小/最大值）
- **CEL 规则**：`cel_rules` 定义语义验证（跨字段约束）

## EnvStore 与 SecretStore

EnvStore 和 SecretStore 是可复用的变量集，代理可以引用。它们共享相同的 `data` 结构，但 `kind` 不是命名约定：SecretStore 的 spec 静态加密、在**每一条读取路径**上脱敏（包括 `manifest export` 与 `debug --component config`），并由 `orchestrator secret key` 的轮换与吊销面板服务，EnvStore 三样都没有。正因为导出是脱敏的，它不能被 apply 回去——完整对比与这对备份意味着什么，见 [02 - 资源模型](02-resource-model.md#9-secretstore加密存储)。

```yaml
apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: shared-config
spec:
  data:
    DATABASE_URL: "postgres://localhost/mydb"
    LOG_LEVEL: "debug"
---
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: api-keys
spec:
  data:
    OPENAI_API_KEY: "sk-..."
```

代理通过 `env` 字段引用存储：

```yaml
spec:
  env:
    - fromRef: shared-config              # 从 EnvStore 导入所有键
    - name: MY_API_KEY
      refValue:                           # 从 SecretStore 导入单个键
        name: api-keys
        key: OPENAI_API_KEY
```

## 持久化存储（WP01）

持久化存储通过 `WorkflowStore` CRD 提供跨任务记忆。数据在任务之间持久化，支持从历史运行中学习。

### 定义存储

```yaml
apiVersion: orchestrator.dev/v2
kind: WorkflowStore
metadata:
  name: context
spec:
  provider: local          # "local"（SQLite）、"file"，或某个 StoreBackendProvider 名称
  schema:
    type: object
    properties:
      value:
        type: string
  retention:
    max_entries: 1000
    ttl_days: 30            # 可选：清理早于该天数的条目
```

声明 `WorkflowStore` 是可选的——未声明的 store 名称会以上述默认值自动置备。

### 从步骤读写

步骤直接使用 CLI。`{project_id}` 由任务上下文渲染，因此无需向步骤注入任何值：

```yaml
steps:
  - id: plan
    scope: task
    enabled: true
    command: >-
      RESULT="$(echo '{"confidence":0.95}')" &&
      orchestrator store put context plan_result "$RESULT" --project {project_id}

  - id: implement
    scope: task
    enabled: true
    command: >-
      INHERITED="$(orchestrator store get context plan_result
      --project {project_id} 2>/dev/null || true)" &&
      echo "planning said: $INHERITED"
```

对于 agent step，把同一条命令写进 StepTemplate 的 prompt，让 agent 自己执行。

> **步骤必须能找到你的 daemon。** runner 会把环境变量裁剪到
> `RuntimePolicy.runner.env_allowlist`，其默认值为 `PATH, HOME, USER, LANG, TERM`。当 daemon 运行在
> 同一 `HOME` 下的默认数据目录时这已足够。若不是——自定义了 `ORCHESTRATORD_DATA_DIR`，或使用了显式的
> control-plane 配置——需要把这些变量加入 allowlist，否则步骤内的 CLI 会报
> `daemon socket not found`，而 `|| true` 兜底会把它悄悄变成一个空值。

> 本节此前描述的声明式绑定——`store_inputs`、`store_outputs`、`step_vars` 与 `store_put`
> 后置动作——已被移除。它们通过一张通用 pipeline 变量表传值，而该表已不再是授权面；
> 使用其中任意一项的 manifest 会被 `[legacy_pipeline_variables_removed]` 拒绝。

### CLI 操作

```bash
# 写入值
orchestrator store put context my_key "my_value"

# 读取值
orchestrator store get context my_key

# 列出键
orchestrator store list context

# 删除键
orchestrator store delete context my_key
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
      - type: spawn_task
        goal: "verify-changes"
        workflow: child_workflow
```

一次声明派生一个子任务。复数形式 `spawn_tasks` —— 从管道变量里取出 JSON 数组、再用
JSONPath 表达式做映射 —— 随协调收敛退休，并在 v0.7 窗口被移除。需要按运行期算出的列表
扇出的步骤，请在步骤内部对每一项调用一次 `spawn_task` 协调工具，让这个列表是步骤自己
持有的值，而不是引擎从捕获文本里解析出来的东西。

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

项由 **`generate_items` 协调工具**生成，由步骤的 Agent 直接把项传进去：

```json
{"name": "generate_items", "arguments": {"items": [{"goal": "candidate-a"}, {"goal": "candidate-b"}]}}
```

声明式的 `post_actions: [{type: generate_items, from_var: ...}]` 形式随协调收敛退休，
并在 v0.7 窗口被移除。工具直接接收项本身，因此没有管道变量要填，也没有 JSONPath
表达式可写错。

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
