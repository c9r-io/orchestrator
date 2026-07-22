# 02 - 资源模型

编排器管理十一种核心资源类型，以及可扩展的自定义资源定义（CRD）。所有资源遵循 Kubernetes 风格的清单格式。

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
  annotations:                     # 可选
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

Agent 是具有声明能力的执行单元，可以使用旧式 shell 命令模板，也可以显式选择供应商 driver。

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
    description: "主代码生成代理"
  selection:             # 可选选择策略覆盖
    strategy: CapabilityAware    # 默认值
  env:                   # 可选环境变量
    - name: LOG_LEVEL
      value: "debug"
    - fromRef: shared-config     # 从 EnvStore 导入所有键
    - name: MY_API_KEY
      refValue:                  # 从 SecretStore 导入单个键
        name: api-keys
        key: OPENAI_API_KEY
  promptDelivery: arg    # 提示词如何传递给代理（默认：arg）
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `capabilities` | 是 | 此代理能做什么（与步骤的 `required_capability` 匹配） |
| `command` | 条件必填 | 未配置 `driver` 时必填；`provider: shell` 也需要；Claude/Codex driver 应省略 |
| `driver` | 否 | 类型化的 provider/transport adapter（`shell`、`claude`、`codex`；当前可执行 transport 为 CLI） |
| `metadata.cost` | 否 | 用于代理选择策略的成本感知路由 |
| `metadata.description` | 否 | 代理的人类可读描述 |
| `selection` | 否 | 代理选择策略覆盖（见下文） |
| `env` | 否 | 环境变量：直接值、`fromRef`（从存储导入全部）、或 `refValue`（从存储导入单个键） |
| `promptDelivery` | 否 | 提示词传递方式：`stdin`、`file`、`env` 或 `arg`（默认：`arg`） |

### Agent Driver

显式 driver 可以避免在 manifest 中写供应商命令行参数，并把工具调用、权限请求、用量和终局结果转成统一事件。Workflow 在 `behavior.driverRequirements` 声明需要的语义；不兼容的候选 Agent 会在 apply 阶段被拒绝。完整字段、安全边界、迁移和示例见双语 [Agent Driver 使用指南](../agent-driver-model.md)。

```yaml
spec:
  capabilities: [implement]
  driver:
    provider: claude
    transport: cli
    options:
      model: sonnet
      permissionMode: ask
      allowedTools: [mcp__orch]
```

### 代理选择

当一个步骤需要某种能力（例如 `required_capability: implement`）时，编排器会选择声明了该能力的代理。如果多个代理匹配，选择会考虑：

- 能力匹配（必须）
- 选择策略评分（每个代理可配置）
- 成本元数据（越低越优先）
- 项目级代理（通过 `--project` 应用）覆盖全局代理

#### 选择策略

| 策略 | 说明 |
|------|------|
| `CostBased` | 静态成本排序 |
| `SuccessRateWeighted` | 按历史成功率加权 |
| `PerformanceFirst` | 延迟优先选择 |
| `Adaptive` | 可配置权重，综合成本、成功率、性能和负载 |
| `LoadBalanced` | 偏好当前负载较低的代理 |
| `CapabilityAware` | 自适应评分 + 健康感知能力追踪 **（默认值）** |

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

## 5. Project（项目）

Project 提供资源隔离域。所有资源命令支持 `--project` 参数限定作用域。

```yaml
apiVersion: orchestrator.dev/v2
kind: Project
metadata:
  name: my-project
spec:
  description: "前端重写项目"
```

## 6. RuntimePolicy（运行时策略）

RuntimePolicy 配置运行器行为、恢复策略和可观测性。

```yaml
apiVersion: orchestrator.dev/v2
kind: RuntimePolicy
metadata:
  name: default
spec:
  runner:
    executor: shell    # shell（默认）| streaming
    # … shell、policy、allowed_shells、env_allowlist、redaction_patterns
  resume: { ... }
  observability: { ... }
```

**`runner.executor`** 只选择旧式全局执行后端。新的供应商感知 Agent 应使用 `spec.driver`：

- `shell`（默认）—— 将每个步骤的命令作为一次性 shell 进程运行，输出按文本/JSON 捕获。
- `streaming` —— 以 `stream-json` 模式驱动 agent CLI，并提供 orchestrator 自持的 MCP 工具，把结构化信号（`tools_called`、`run_cost_usd` 等）暴露给编排 CEL。见 `docs/design_doc/orchestrator/streaming-runner-pivot-overview.md` 与[演示](../showcases/streaming-mark-done-convergence.md)。

## 7. ExecutionProfile（执行 Profile）

ExecutionProfile 定义代理步骤的沙盒/宿主执行边界。默认值：`mode: host`、`fs_mode: inherit`、`network_mode: inherit`。

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: sandbox_write
spec:
  mode: sandbox                    # host | sandbox
  fs_mode: workspace_rw_scoped     # inherit | workspace_rw_scoped
  writable_paths: [src, docs]
  network_mode: deny               # inherit | deny | allowlist
```

## 8. EnvStore（环境变量存储）

EnvStore 存放可复用的环境变量集，代理可通过 `env.fromRef` 引用。

```yaml
apiVersion: orchestrator.dev/v2
kind: EnvStore
metadata:
  name: shared-config
spec:
  data:
    DATABASE_URL: "postgres://localhost/mydb"
    LOG_LEVEL: "debug"
```

## 9. SecretStore（加密存储）

SecretStore 与 EnvStore 结构相同，但用于敏感值。通过 `kind` 字段在资源层面区分。

```yaml
apiVersion: orchestrator.dev/v2
kind: SecretStore
metadata:
  name: api-keys
spec:
  data:
    OPENAI_API_KEY: "sk-..."
```

代理通过 `env` 条目引用存储（参见上文 Agent spec）。

## 10. Trigger（触发器）

Trigger 支持基于 cron 定时或任务生命周期事件（如 task_completed）自动创建任务。遵循 Kubernetes CronJob 心智模型。

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: nightly-qa
spec:
  cron:
    schedule: "0 2 * * *"             # 5 段 cron：分 时 日 月 周
    timezone: "Asia/Shanghai"          # IANA 时区（可选，默认 UTC）
  action:
    workflow: full-qa                  # 触发时运行的工作流
    workspace: main-workspace          # 任务所用的工作区
  concurrencyPolicy: Forbid            # Allow | Forbid | Replace
  suspend: false
  historyLimit:
    successful: 5
    failed: 3
```

| 字段 | 必填 | 说明 |
|------|------|------|
| `cron` | cron/event 二选一 | 定时触发，支持可选时区 |
| `event` | cron/event 二选一 | 事件驱动触发（source + filter） |
| `action.workflow` | 是 | 触发时运行的工作流 |
| `action.workspace` | 是 | 任务关联的工作区 |
| `concurrencyPolicy` | 否 | `Allow`（默认）、`Forbid`（有活跃任务时跳过）、`Replace`（取消活跃任务后创建） |
| `suspend` | 否 | 暂停触发器但不删除（默认：`false`） |
| `historyLimit` | 否 | 每个触发器保留的已完成任务上限（默认：5） |

### 事件触发

事件触发器在匹配的任务生命周期事件发生时触发：

```yaml
spec:
  event:
    source: task_completed             # task_completed | task_failed
    filter:
      workflow: build-pipeline         # 仅匹配来自此工作流的任务
  action:
    workflow: deploy
    workspace: prod
```

### 触发器生命周期

```bash
orchestrator trigger suspend <name>    # 暂停触发器
orchestrator trigger resume <name>     # 恢复触发器
orchestrator trigger fire <name>       # 手动触发（立即创建任务）
orchestrator get triggers              # 列出所有触发器
orchestrator delete trigger/<name>     # 删除触发器
```

## 11. SourceTaskTemplate（来源任务模板）

SourceTaskTemplate 是项目级的来源到任务配方：它描述经过验证的来源信息将如何形成未来任务的目标和动作。它与 StepTemplate 不同：SourceTaskTemplate 面向任务创建，StepTemplate 面向工作流内部某一步的 agent 提示词。

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskTemplate
metadata:
  name: docs-from-slack
spec:
  skill:
    name: docs
    invocation: "$docs"
    args: ["--concise"]
  action:
    workflow: slack-documentation
    workspace: main
    start: true
    initial_vars:
      origin: slack
  goalTemplate: >-
    {skill_invocation}: use {source_message_url} as the source request
  allowedVariables: [skill_invocation, source_message_url]
```

渲染器只接受显式白名单中的精确变量，且只执行一轮替换。Slack 示例 URL 必须是 HTTPS、属于 `slack.com`，并使用 `/archives/` permalink 路径。预览与未来实时路由共用 daemon 渲染器，且不会创建任务或来源记录：

```bash
orchestrator source template preview docs-from-slack \
  --project my-project --provider slack --installation primary \
  --message-url https://example.slack.com/archives/C123/p1234567890000100 \
  -o json
```

当 `SourceTaskBinding` 仍引用模板时，普通删除会被拒绝。管理员可以显式使用 `--force --force-references` 原子删除模板及引用；该操作会进入审计记录。

## 12. SourceTaskBinding（来源任务绑定）

SourceTaskBinding 是项目级的精确匹配策略：它根据经过认证的 Slack reaction 证据选择唯一的 SourceTaskTemplate。Binding 引用同项目的 Slack webhook Trigger；installation identity 和外部 actor 到可信角色的映射都由 Trigger 管理。

```yaml
apiVersion: orchestrator.dev/v2
kind: SourceTaskBinding
metadata:
  name: slack-code-analysis
spec:
  triggerRef: slack-main
  match:
    eventKind: reaction_added
    reaction: agent-analyze
    targetKind: message
    channels: [C01234567]
  templateRef: analyze-from-slack
  allowedActorRoles: [operator, admin]
  suspend: false
```

频道策略必须二选一：非空 `channels` 列表，或显式设置 `allChannels: true`。角色列表不能为空，并且只能从 Trigger `actorRoles` 解析；来源请求不能自行声明角色。两个可能匹配同一事件的 enabled 规则会在 apply/resume 时被拒绝，不使用隐式优先级。

Trigger 的 reaction 路由需要显式开启：

```yaml
event:
  source: webhook
  webhook:
    provider: slack
    installationId: T012345
    actorRoles: {U012345: operator}
    reactionRouting: bindings
    secret: {fromRef: slack-signing}
    outboundCredential: {fromRef: slack-api, key: BOT_TOKEN}
```

`reactionRouting` 默认是 `disabled`。可以先进行无副作用验证：

```bash
orchestrator source binding simulate --project my-project \
  --provider slack --installation T012345 --reaction agent-analyze \
  --channel C01234567 --actor U012345 -o json
orchestrator source binding suspend slack-code-analysis --project my-project
orchestrator source binding resume slack-code-analysis --project my-project
```

模拟不会调用 Slack、渲染模板或创建任务。当 `reactionRouting: bindings` 启用后，经过签名验证的实时 reaction 会在异步路由中通过 Slack `chat.getPermalink` 解析 `channel + message_ts`，使用冻结的模板渲染目标，并创建唯一、确定性的 canonical task。重复投递和 daemon 重启都会收敛到同一个 message/reaction/binding route 与 task。

签名 `secret` 与 `outboundCredential` 是两个独立的同项目 SecretStore 引用。Daemon 只在 Slack API 调用时解析 outbound token；公开 source 摘要、audit、timeline 和 task 输入都不包含 token。只读用户可以查看安全的自动化状态、template 与 binding 信息，Operator 可以显式读取受保护的 Slack 链接：

```bash
orchestrator source list --project my-project -o json
orchestrator source route <source-event-id> -o json
```

将 `reactionRouting` 改回 `disabled` 会停止新的 badge 路由，但保留既有 task 与证据。普通删除仍被引用的 Trigger 或 SourceTaskTemplate 会被拒绝；管理员使用 `--force --force-references` 可以原子清理引用并留下审计证据。

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
orchestrator describe workspace/default

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
