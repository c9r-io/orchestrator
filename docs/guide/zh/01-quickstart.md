# 01 - 快速开始

5 分钟启动你的第一个本地 Harness Engineering control plane。

在这个快速开始里，你会启动 daemon、加载 manifest，并让控制面通过声明式 workflow 调度一个基于 shell 的 agent。

## 前置条件

- Rust 工具链（用于从源码构建）
- SQLite3
- Bash shell

## 第一步：构建

```bash
cargo build --workspace --release
```

构建会产出当前支持的运行时二进制：

| 二进制 | 路径 | 用途 |
|--------|------|------|
| `orchestratord` | `target/release/orchestratord` | 守护进程（gRPC 服务端 + 内嵌工作器） |
| `orchestrator` | `target/release/orchestrator` | CLI 客户端（通过 gRPC 连接守护进程） |

唯一支持的运行方式是 `orchestratord` + `orchestrator`。

## 第二步：启动 daemon

```bash
./target/release/orchestratord --foreground --workers 2
```

daemon 负责持有 SQLite、任务队列和 worker 池。保持它在一个终端中运行，再在另一个终端中使用 CLI 客户端。

**创建 SQLite 表结构的正是启动 daemon 这一步**（位置为
`~/.orchestratord/agent_orchestrator.db`，可通过 `ORCHESTRATORD_DATA_DIR` 覆盖）。
daemon 在接受任何一个连接之前就已经跑完全部待执行迁移，因此没有需要手工初始化的东西。

## 第三步：等待 daemon 可以服务

```bash
./target/release/orchestrator daemon status --wait-ready
# orchestratord is ready (migrations=ready (38/38), keyring=ready (active key primary), workers=ready (2/2 started))
```

手动敲命令时这步可省略——等你切换完终端，daemon 早就绪了。写脚本时值得知道：
套接字接受连接的时刻略早于 worker 池注册完成，所以"启动 daemon 后立刻创建任务"
的脚本可能会看着任务没有人领。

> **本指南早期版本在这里放的是 `orchestrator init`**，并声称它创建表结构。
> 事实并非如此：`init` 是一次发往运行中 daemon 的 RPC，因此在 daemon 存在之前
> 根本跑不起来，而运行中的 daemon 早已完成迁移。该命令仍然存在且无害，
> 只是它不是一个安装步骤。

## 第四步：阅读清单文件

quickstart 清单随仓库一起提供：
[fixtures/manifests/bundles/quickstart.yaml](../../../fixtures/manifests/bundles/quickstart.yaml)。
它定义了 Workspace、Agent 和 Workflow：

```yaml
# fixtures/manifests/bundles/quickstart.yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: default
spec:
  work_dir: "."
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
  driver:
    provider: shell
    transport: cli
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

每个 Agent 都要声明 typed driver——这里是 `shell/cli`，它按原样执行
`spec.command`。省略 `spec.driver` 是已废弃的兼容写法，apply 时会触发
`[legacy_agent_command_deprecated]` 警告；参见
[Agent Driver Model](../agent-driver-model.md)。

## 第五步：应用清单

```bash
./target/release/orchestrator apply -f fixtures/manifests/bundles/quickstart.yaml
```

这会将所有资源（Workspace、Agent、Workflow）加载到数据库中。你可以验证：

```bash
./target/release/orchestrator get workspaces
./target/release/orchestrator get agents
./target/release/orchestrator get workflows
```

## 第六步：创建并运行任务

```bash
./target/release/orchestrator task create \
  --goal "My first QA run" \
  --workflow simple_qa
```

这会创建一个任务，绑定到 `default` 工作区和 `simple_qa` 工作流，并立即开始执行。想自己指定任务名可加 `--name "my-first-task"`。

如果只创建不启动：

```bash
./target/release/orchestrator task create \
  --goal "My first QA run" \
  --workflow simple_qa \
  --no-start
```

然后手动启动：

```bash
./target/release/orchestrator task start <task_id>
```

## 第七步：查看结果

```bash
# 列出所有任务
./target/release/orchestrator task list

# 任务详情（表格、JSON 或 YAML 格式）
./target/release/orchestrator task info <task_id>
./target/release/orchestrator task info <task_id> -o json

# 查看执行日志
./target/release/orchestrator task logs <task_id>
```

## 刚才发生了什么？

1. `orchestratord` 启动了控制面、SQLite 运行时和内嵌 worker
2. `init` 创建了 SQLite 表结构
3. `apply` 通过 daemon 将三个资源加载到数据库
4. `task create` 绑定了工作区和工作流，发现 QA 目标文件作为任务项，并将任务排入 daemon worker 队列
5. `echo_agent` 被选中（因为它具备 `qa` 能力），其命令针对每个项执行
6. 结果（退出码、stdout、stderr）被记录到数据库中

## 下一步

- [02 - 资源模型](02-resource-model.md) —— 了解资源类型
- [03 - 工作流配置](03-workflow-configuration.md) —— 设计多步骤工作流
