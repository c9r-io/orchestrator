# 01 - 快速开始

5 分钟跑通你的第一个工作流。

## 前置条件

- Rust 工具链（用于从源码构建）
- SQLite3
- Bash shell

## 第一步：构建

```bash
cargo build --workspace --release
```

构建产生三个二进制文件：

| 二进制 | 路径 | 用途 |
|--------|------|------|
| `agent-orchestrator` | `core/target/release/agent-orchestrator` | 单体 CLI（传统，已弃用） |
| `orchestratord` | `target/release/orchestratord` | 守护进程（gRPC 服务端 + 内嵌工作器） |
| `orchestrator` | `target/release/orchestrator` | CLI 客户端（通过 gRPC 连接守护进程） |

C/S 模式（推荐）请直接使用 `orchestratord` + `orchestrator`。传统单体二进制已弃用。

## 第二步：初始化数据库

```bash
orchestrator init
```

这会在 `data/agent_orchestrator.db` 创建 SQLite 表结构。注意：此命令**不会**加载任何配置 —— 配置在下一步完成。

## 第三步：编写清单文件

创建一个 YAML 文件，定义 Workspace、Agent 和 Workflow。以下是一个最小示例：

```yaml
# my-first-workflow.yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  root_path: "."
  qa_targets:
    - docs/qa
  ticket_dir: docs/ticket
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: echo_agent
spec:
  capabilities:
    - qa
  command: >-
    echo '{"confidence":0.95,"quality_score":0.9,
    "artifacts":[{"kind":"analysis","findings":[
    {"title":"all-good","description":"no issues found","severity":"info"}
    ]}]}'
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: simple_qa
spec:
  steps:
    - id: qa
      type: qa
      enabled: true
  loop:
    mode: once
```

## 第四步：应用清单

```bash
orchestrator apply -f my-first-workflow.yaml
```

这会将所有资源（Workspace、Agent、Workflow）加载到数据库中。你可以验证：

```bash
orchestrator get workspaces
orchestrator get agents
orchestrator get workflows
```

## 第五步：创建并运行任务

```bash
orchestrator task create \
  --name "my-first-task" \
  --goal "Verify QA docs pass" \
  --workflow simple_qa
```

这会创建一个任务，绑定到 `default` 工作区和 `simple_qa` 工作流，并立即开始执行。

如果只创建不启动：

```bash
orchestrator task create \
  --name "my-first-task" \
  --goal "Verify QA docs pass" \
  --workflow simple_qa \
  --no-start
```

然后手动启动：

```bash
orchestrator task start <task_id>
```

## 第六步：查看结果

```bash
# 列出所有任务
orchestrator task list

# 任务详情（表格、JSON 或 YAML 格式）
orchestrator task info <task_id>
orchestrator task info <task_id> -o json

# 查看执行日志
orchestrator task logs <task_id>
```

## 刚才发生了什么？

1. `init` 创建了 SQLite 表结构
2. `apply` 将三个资源加载到数据库
3. `task create` 绑定了工作区和工作流，发现 QA 目标文件作为任务项（task items），然后对每个项执行 `qa` 步骤
4. `echo_agent` 被选中（因为它具备 `qa` 能力），其命令针对每个项执行
5. 结果（退出码、stdout、stderr）被记录到数据库中

## 下一步

- [02 - 资源模型](02-resource-model.md) —— 了解四种资源类型
- [03 - 工作流配置](03-workflow-configuration.md) —— 设计多步骤工作流
