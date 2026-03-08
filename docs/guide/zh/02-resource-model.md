# 02 - 资源模型

编排器管理四种核心资源类型，以及可扩展的自定义资源定义（CRD）。所有资源遵循 Kubernetes 风格的清单格式。

## 清单结构

每个资源使用相同的信封格式：

```yaml
apiVersion: orchestrator.dev/v2
kind: <ResourceKind>
metadata:
  name: <unique-name>
  description: "可选描述"          # 可选
  labels:                          # 可选
    key: value
spec:
  # 特定于 kind 的字段
```

多个资源可以定义在同一个 YAML 文件中，使用 `---` 分隔。

## 1. Workspace（工作区）

Workspace 定义任务执行的文件系统上下文。

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: my-project
spec:
  root_path: "."                    # 项目根目录
  qa_targets:                       # 扫描 QA 文件的目录（.md 文件成为任务项）
    - docs/qa
  ticket_dir: docs/ticket           # 失败工单的写入目录
  self_referential: false           # true = 编排器修改自身代码（参见第 06 章）
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `root_path` | 是 | 项目根目录；相对路径从此处解析 |
| `qa_targets` | 是 | 包含 QA 文档的目录（`.md` 文件成为任务项） |
| `ticket_dir` | 是 | 失败工单目录 |
| `self_referential` | 否 | 为 `true` 时启用生存机制（默认：`false`） |

## 2. Agent（代理）

Agent 是具有声明能力和 shell 命令模板的执行单元。

```yaml
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: coder
  description: "代码生成代理"
spec:
  capabilities:          # 此代理提供的能力列表
    - implement
    - ticket_fix
    - align_tests
  command: >-            # shell 命令模板；{prompt} 在运行时注入
    claude --print -p '{prompt}'
  metadata:              # 可选元数据，用于选择评分
    cost: 100
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `capabilities` | 是 | 此代理能做什么（与步骤的 `required_capability` 匹配） |
| `command` | 是 | shell 命令模板。支持 `{prompt}` 占位符（由 StepTemplate 填充） |
| `metadata.cost` | 否 | 用于代理选择策略的成本感知路由 |

### 代理选择

当一个步骤需要某种能力（例如 `required_capability: implement`）时，编排器会选择声明了该能力的代理。如果多个代理匹配，选择会考虑：

- 能力匹配（必须）
- 成本元数据（越低越优先）
- 项目级代理（通过 `--project` 应用）覆盖全局代理

## 3. StepTemplate（步骤模板）

StepTemplate 将提示词内容与代理定义解耦。工作流步骤通过名称引用模板；运行时模板的 `prompt` 被注入到代理 `{prompt}` 占位符中。

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: plan
spec:
  description: "架构指导的实施规划"
  prompt: >-
    你正在 {source_tree} 项目中工作。
    为以下目标创建详细的实施计划：{goal}。
    当前差异：{diff}
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `description` | 否 | 人类可读的描述 |
| `prompt` | 是 | 包含管道变量占位符的提示词模板 |

### 管道变量

模板可以使用 `{variable_name}` 语法引用管道变量：

| 变量 | 说明 |
|------|------|
| `{goal}` | 任务目标字符串 |
| `{source_tree}` | 工作区根路径 |
| `{workspace_root}` | 工作区绝对路径 |
| `{diff}` | 工作区中当前的 git diff |
| `{rel_path}` | 当前项的相对路径（item 作用域步骤） |
| `{qa_file_path}` | 当前项的 QA 文件路径 |
| `{plan_output_path}` | plan 步骤输出文件的路径 |
| `{ticket_paths}` | 当前项的活动工单路径 |
| `{ticket_dir}` | 工单目录路径 |
| `{task_id}` | 当前任务 ID |
| `{task_item_id}` | 当前任务项 ID |
| `{cycle}` | 当前循环轮次 |
| `{workspace}` | 工作区 ID |
| `{project}` | 项目 ID |
| `{workflow}` | 工作流 ID |
| `{prev_stdout}` | 上一步骤的原始 stdout |
| `{prev_stderr}` | 上一步骤的原始 stderr |
| `{<step_id>_output}` | 指定 ID 步骤的输出 |
| `{prompt}` | 已解析的提示词（用于 Agent 命令模板） |

**磁盘溢出**：超过 4096 字节的值会自动保存到文件，变量变为 `{<key>_path}` 指向文件路径。

## 4. Workflow（工作流）

Workflow 定义流程：步骤的有序列表、循环策略和可选的终结规则。

```yaml
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: qa_fix_retest
spec:
  steps:
    - id: qa
      type: qa
      enabled: true
    - id: ticket_scan
      type: ticket_scan
      enabled: true
    - id: fix
      type: fix
      enabled: true
    - id: retest
      type: retest
      enabled: true
  loop:
    mode: once
```

工作流配置详见[第 03 章](03-workflow-configuration.md)。

## 资源生命周期

### 应用（创建/更新）

```bash
# 从文件
orchestrator apply -f manifest.yaml

# 从标准输入
cat manifest.yaml | orchestrator apply -f -

# 试运行（仅验证不写入）
orchestrator apply -f manifest.yaml --dry-run
```

### 查询

```bash
# 列出资源
orchestrator get workspaces
orchestrator get agents
orchestrator get workflows

# 详情视图
orchestrator describe workspace default
orchestrator workspace info default

# 输出格式
orchestrator get agents -o json
orchestrator get agents -o yaml

# 标签选择器
orchestrator get workspaces -l env=dev
```

### 导出

```bash
# 导出所有配置为 YAML
orchestrator manifest export

# 交互式编辑
orchestrator edit workspace default
```

## 多文档清单

单个 YAML 文件可以定义一个工作流所需的所有资源。这是推荐的模式：

```yaml
# everything-in-one.yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: "."
  qa_targets: [docs/qa]
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: mock_agent
spec:
  capabilities: [qa, fix, loop_guard]
  command: "echo '{\"confidence\":0.9,\"quality_score\":0.9,\"artifacts\":[]}'"
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: my_workflow
spec:
  steps:
    - id: qa
      type: qa
      enabled: true
    - id: fix
      type: fix
      enabled: true
  loop:
    mode: once
```

然后一次性应用：

```bash
orchestrator apply -f everything-in-one.yaml
```

## 下一步

- [03 - 工作流配置](03-workflow-configuration.md) —— 步骤定义、作用域、循环
- [04 - CEL 预钩子](04-cel-prehooks.md) —— 动态步骤门控
