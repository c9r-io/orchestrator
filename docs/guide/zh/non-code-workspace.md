# 不依赖代码仓库运行通用 Agent 任务

当工作本质上是一个过程——例如 Slack 分流、库存查询、文档分析或回复草拟——而不是修改代码仓库时，请使用 `task` Workspace。

## 先建立正确的心智模型

`code_repo` Workspace 会发现 QA 文件，并可使用 ticket 或 Git checkpoint。`task` Workspace 只有一项工作：任务目标。因此它只创建一个隐式 item，并在 agent 调用 `mark_done` 或 driver 给出成功终局事件时完成。

| 能力 | `code_repo` | `task` |
|---|---|---|
| `work_dir` | 必填 | 可选 |
| `qa_targets`、`ticket_dir` | 必填 | 禁止 |
| 显式 target file | 支持 | 禁止 |
| Git checkpoint / self-reference | 按配置支持 | 禁止 |
| 执行边界 | 由策略决定 | 必须使用 scoped sandbox |
| item 完成信号 | Workflow finalize / QA 证据 | `mark_done` 或 driver 终局事件 |

## 1. 先限定 daemon 可以共享什么

daemon 管理员需要在启动前创建 `{data_dir}/file-sharing.yaml`。默认位置是 `~/.orchestratord/file-sharing.yaml`。

```yaml
fileSharing:
  globalSkills:
    - path: ~/.orchestrator/skills
  shareableRoots:
    - ~/.orchestrator/skills
    - ~/warehouse-data
```

`shareableRoots` 是权限天花板。Workspace 和 ExecutionProfile 只能缩小权限，不能扩大权限。如果文件不存在，所有宿主路径共享都默认拒绝。修改后需要重启 `orchestratord`。

不要为了方便把整个 HOME 加进去。建议为 Skills 和每一类业务数据分别配置窄范围目录。

## 2. 定义 task Workspace

需要让多个任务共享数据时，配置持久目录：

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: warehouse-ops
spec:
  kind: task
  work_dir: ~/warehouse-data
```

需要隔离的临时任务可以省略 `work_dir`：

```yaml
apiVersion: orchestrator.dev/v2
kind: Workspace
metadata:
  name: private-scratch
spec:
  kind: task
```

daemon 会为每个 task 创建唯一的私有 HOME，并在任务终止后清理。用户自己提供的持久目录永远不会被自动删除。

旧 manifest 的 `root_path` 仍可读取；新配置应统一写 `work_dir`。

## 3. 强制使用 scoped sandbox

task Workspace 中每一个启用的 step 都必须引用沙箱配置：

```yaml
apiVersion: orchestrator.dev/v2
kind: ExecutionProfile
metadata:
  name: task-sandbox
spec:
  mode: sandbox
  fs_mode: workspace_rw_scoped
  network_mode: deny
```

只有确实需要时才增加 `readable_paths` 或 `writable_paths`。每一条路径都必须位于 `shareableRoots` 之下；task Workspace 不允许在这些路径中使用环境变量展开。

在 agent 进程内部：

- `HOME` 被强制指向 `work_dir` 或 daemon 管理的 task HOME。
- XDG 配置、缓存、数据、状态、runtime 目录和 `TMPDIR` 都位于该目录下。
- 全局 Skill 目录只读，并通过冒号分隔的 `ORCHESTRATOR_GLOBAL_SKILLS` 暴露。
- `ORCHESTRATOR_READABLE_PATHS` 为兼容的 agent wrapper 提供完整只读白名单。

## 4. 定义 Agent、Prompt 与 Workflow

下面的确定性示例用 driver 终局事件完成任务：

```yaml
apiVersion: orchestrator.dev/v2
kind: StepTemplate
metadata:
  name: reply-prompt
spec:
  prompt: "{goal}"
---
apiVersion: orchestrator.dev/v2
kind: Agent
metadata:
  name: reply-agent
spec:
  capabilities: [prepare_reply]
  command: your-agent-command
  driver:
    provider: shell
    transport: cli
    shell:
      requirePromptPlaceholder: false
---
apiVersion: orchestrator.dev/v2
kind: Workflow
metadata:
  name: reply-flow
spec:
  steps:
    - id: prepare
      required_capability: prepare_reply
      template: reply-prompt
      execution_profile: task-sandbox
      scope: task
  loop:
    mode: once
```

Claude 或 Codex driver 也可以调用 Orchestrator 托管的 `mark_done` MCP 工具。循环上限、超时、资源限制和取消等安全机制仍然生效。

## 5. 创建和检查任务

```bash
orchestrator apply --project operations -f operations.yaml
orchestrator task create \
  --project operations \
  --workspace warehouse-ops \
  --workflow reply-flow \
  --goal "检查打了 badge 的 Slack 消息，并生成基于库存的回复建议"
```

task Workspace 不要传 `--target-file`。

在 Process Console 中进入“Tasks”并选择任务。概览会显示“Workspace type: Task”，Expert workflow 使用通用的“Task”标签。Timeline、证据、Attention、handoff 和 Session 接管能力与代码任务保持一致。

## Slack Badge 完整示例

仓库内置的 mock bundle 可以完整演示 badge 到非代码任务的路径：

```bash
cargo build -p orchestratord -p orchestrator-cli
scripts/qa/test-non-code-workspace.sh
```

脚本会启动隔离 daemon 和本地假 Slack 服务，并验证签名 reaction、permalink 路由、全局 Skill、库存读取、回复建议、Attention、任务收敛和 HOME 清理。它不会调用真实 AI provider，也不会消耗 API credits。

## 常见错误

| 错误 | 含义 | 处理方式 |
|---|---|---|
| `TASK_WORKSPACE_SANDBOX_REQUIRED` | 某个 step 没有 scoped sandbox | 添加 `mode: sandbox` 且 `fs_mode` 非 `inherit` 的 ExecutionProfile |
| `FILE_SHARING_PATH_OUTSIDE_CEILING` | 宿主路径超出 daemon 权限 | 在 `file-sharing.yaml` 增加窄范围 root，然后重启 daemon |
| `TASK_WORKSPACE_QA_FIELDS_FORBIDDEN` | 从代码 Workspace 复制了 QA/ticket 字段 | 删除 `qa_targets` 与 `ticket_dir` |
| `TASK_WORKSPACE_GIT_CHECKPOINT_FORBIDDEN` | Workflow 使用 Git checkpoint | 把 checkpoint strategy 改为 `none` |
| `TASK_WORKSPACE_TARGET_FILES_FORBIDDEN` | 创建 task 时传了 target file | 把工作内容放入 `goal` 或 initial variables |

## 安全检查清单

- `shareableRoots` 保持最小化。
- 把持久 `work_dir` 视为共享状态；需要隔离时省略它。
- 除非 agent 确实需要联网，否则使用 `network_mode: deny`。
- 凭据通过 SecretStore 传递，不要写入共享文件或 task goal。
- 生产启用前执行[文件共享权限天花板测试](../../security/authorization/02-file-sharing-ceiling.md)和[HOME 隔离测试](../../security/file-security/02-workspace-home-isolation.md)。
