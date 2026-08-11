# 07 - CLI 参考

Agent Orchestrator CLI 全部命令速查。

## 入口

| 二进制 | 说明 |
|--------|------|
| `orchestratord` | gRPC 守护进程 — 服务端 + 内嵌工作器 |
| `orchestrator` | CLI 客户端 — 通过 Unix 套接字的轻量 gRPC 调用 |

守护进程持有所有状态（引擎、数据库、任务队列）。CLI 是一个轻量级 RPC 客户端。

## 全局选项

| 标志 | 说明 |
|------|------|
| `-v, --verbose` | 启用详细输出 |
| `-h, --help` | 打印帮助 |
| `-V, --version` | 打印版本 |
| `--control-plane-config <path>` | 覆盖控制面板客户端配置（环境变量：`ORCHESTRATOR_CONTROL_PLANE_CONFIG`） |

## 命令别名

| 命令 | 别名 |
|------|------|
| `agent` | `ag` |
| `agent list` | `agent ls` |
| `apply` | `ap` |
| `check` | `ck` |
| `db migrations list` | `db migrations ls` |
| `debug` | `dbg` |
| `delete` | `rm` |
| `describe` | `desc` |
| `event` | `ev` |
| `event list` | `event ls` |
| `get` | `g` |
| `guide` | `gd` |
| `secret key list` | `secret key ls` |
| `store list` | `store ls` |
| `task` | `t` |
| `task create` | `task new` |
| `task delete` | `task rm` |
| `task info` | `task get` |
| `task list` | `task ls` |
| `task logs` | `task log` |
| `trigger` | `tg` |

## 初始化与配置

### init

创建运行时目录和 SQLite 表结构。

```bash
orchestrator init
```

### apply

从 YAML 清单加载资源到数据库。

```bash
# 从文件
orchestrator apply -f manifest.yaml

# 从标准输入
cat manifest.yaml | orchestrator apply -f -

# 试运行（仅验证）
orchestrator apply -f manifest.yaml --dry-run

# 项目级应用
orchestrator apply -f manifest.yaml --project my-project
```

### check

预检验证：交叉引用代理、工作流和模板。

```bash
orchestrator check
orchestrator check --workflow self-bootstrap
orchestrator check --project my-project
orchestrator check -o json
```

| 标志 | 说明 |
|------|------|
| `--workflow <WORKFLOW>` | 检查指定工作流 |
| `-o, --output` | 输出格式：table（默认）、json、yaml |
| `-p, --project` | 项目筛选 |

## 资源查询

### get

列出资源（kubectl 风格）。

```bash
orchestrator get workspaces
orchestrator get agents
orchestrator get workflows

# 输出格式
orchestrator get agents -o json
orchestrator get agents -o yaml

# 项目作用域查询
orchestrator get agents --project my-project

# 标签选择器
orchestrator get agents -l env=dev
```

| 标志 | 说明 |
|------|------|
| `-o, --output` | 输出格式：table（默认）、json、yaml |
| `-l, --selector` | 标签选择器过滤 |
| `-p, --project` | 项目筛选 |

### describe

单个资源的详细视图。

```bash
orchestrator describe workspace/default
orchestrator describe agent/coder

# 项目作用域
orchestrator describe agent/my-agent --project my-project
```

### delete

按 kind/name 删除资源。

```bash
orchestrator delete workspace/my-ws --force
orchestrator delete agent/old-agent --force

# 试运行
orchestrator delete agent/old-agent --dry-run

# 项目作用域
orchestrator delete agent/old --force --project my-project
```

| 标志 | 说明 |
|------|------|
| `-f, --force` | 强制删除，无需确认 |
| `--dry-run` | 显示将被删除的内容 |
| `-p, --project` | 项目筛选 |

## 任务生命周期

### task create

```bash
orchestrator task create \
  --name "my-task" \
  --goal "实现功能 X" \
  --workflow self-bootstrap \
  --project my-project \
  --workspace default \
  --target-file docs/qa/01-test.md    # 可指定多次

# 步骤筛选：仅运行工作流中的指定步骤
orchestrator task create \
  --workflow sdlc --project my-project \
  --step fix \
  --set ticket_paths=docs/ticket/T-0042.md

# 多个步骤（按工作流顺序执行）
orchestrator task create \
  --workflow sdlc --step plan --step implement
```

| 标志 | 说明 |
|------|------|
| `-n, --name` | 任务名称 |
| `-g, --goal` | 任务目标/描述 |
| `-p, --project` | 项目 ID |
| `-w, --workspace` | 工作区 ID |
| `-W, --workflow` | 工作流 ID |
| `-t, --target-file` | 目标文件（可重复） |
| `--no-start` | 创建但不自动启动 |
| `-S, --step` | 仅执行指定的步骤 ID（可重复） |
| `--set` | 注入流水线变量，格式为 `key=value`（可重复） |

### run

同步执行步骤 — 创建任务、跟踪日志，并以状态码退出。

```bash
# 带步骤筛选的同步执行
orchestrator run \
  --workflow sdlc --step fix \
  --set ticket_paths=docs/ticket/T-0042.md

# 后台模式（等同于 task create）
orchestrator run --workflow sdlc --step fix --detach

# 直接组装模式：不经过工作流直接执行 StepTemplate
orchestrator run \
  --template fix-ticket \
  --agent-capability fix \
  --set ticket_paths=docs/ticket/T-0042.md
```

| 标志 | 说明 |
|------|------|
| `-W, --workflow` | 工作流 ID（除非指定 `--template`，否则必填） |
| `-S, --step` | 仅执行指定的步骤 ID（可重复） |
| `--set` | 注入流水线变量，格式为 `key=value`（可重复） |
| `-p, --project` | 项目 ID |
| `-w, --workspace` | 工作区 ID |
| `-t, --target-file` | 目标文件（可重复） |
| `--detach` | 后台运行（打印任务 ID 并返回） |
| `--template` | StepTemplate 名称（直接组装模式） |
| `--agent-capability` | 直接组装模式下的代理能力 |
| `--profile` | 直接组装模式下的 ExecutionProfile 覆盖 |

### task list / info

```bash
orchestrator task list
orchestrator task list -o json
orchestrator task list --project my-project    # 按项目筛选
orchestrator task list --status running        # 按状态筛选
orchestrator task list -v                      # 详细输出

orchestrator task info <task_id>
orchestrator task info <task_id> -o yaml
```

| 标志 (list) | 说明 |
|-------------|------|
| `-s, --status` | 按任务状态筛选 |
| `-p, --project` | 项目筛选 |
| `-o, --output` | 输出格式：table（默认）、json、yaml |
| `-v, --verbose` | 详细输出 |

### task items

列出任务的各个项目及其状态。

```bash
orchestrator task items <task_id>
orchestrator task items <task_id> --status running
orchestrator task items <task_id> -o json
```

| 标志 | 说明 |
|------|------|
| `-s, --status` | 按项目状态筛选 |
| `-o, --output` | 输出格式：table（默认）、json、yaml |

### task recover

恢复孤立的运行中项目（例如崩溃后）。

```bash
orchestrator task recover <task_id>
```

### task start / pause / resume

```bash
orchestrator task start <task_id>
orchestrator task start --latest             # 启动最近的任务

orchestrator task pause <task_id>
orchestrator task resume <task_id>
orchestrator task resume <task_id> --reset-blocked   # 将阻塞项重置为未解决状态
```

| 标志 (start) | 说明 |
|--------------|------|
| `-l, --latest` | 启动最近的任务 |

| 标志 (resume) | 说明 |
|---------------|------|
| `--reset-blocked` | 将阻塞项重置为未解决状态 |

### task logs / watch / trace

```bash
# 查看执行日志
orchestrator task logs <task_id>
orchestrator task logs <task_id> --follow --timestamps
orchestrator task logs <task_id> --tail 50

# 实时监控（自动刷新状态面板）
orchestrator task watch <task_id>
orchestrator task watch <task_id> --interval 5

# 执行追踪与异常检测
orchestrator task trace <task_id>
orchestrator task trace <task_id> --verbose -o json
```

| 标志 (logs) | 说明 |
|-------------|------|
| `-f, --follow` | 实时跟踪日志 |
| `-n, --tail` | 显示行数（默认：100） |
| `--timestamps` | 包含时间戳 |

| 标志 (watch) | 说明 |
|--------------|------|
| `--interval` | 刷新间隔秒数（默认：2） |
| `--timeout <SECONDS>` | N 秒后退出（0 = 无超时，默认：0） |

| 标志 (trace) | 说明 |
|--------------|------|
| `--verbose` | 详细追踪输出 |
| `-o, --output` | 输出格式：table（默认）、json、yaml |

### task timeline

显示任务的语义化过程时间线 — 目标、执行、证据、失败与状态迁移，支持稳定分页。

```bash
orchestrator task timeline <task_id>                       # first timeline page
orchestrator task timeline <task_id> --category failure --follow
orchestrator task timeline <task_id> -o json
```

| 标志 | 说明 |
|------|------|
| `--cursor` | 从分页游标继续 |
| `-l, --limit` | 每页条目数（默认：50） |
| `--category` | 按条目类别筛选 |
| `-f, --follow` | 跟踪新的时间线条目 |
| `-o, --output` | 输出格式：table（默认）、json、yaml |

### task retry

重试失败的任务项。

```bash
orchestrator task retry <task_item_id> [--force]
```

### task delete

```bash
orchestrator task delete <task_id> --force
orchestrator task delete <id1> <id2> <id3> --force   # 多个任务 ID
orchestrator task delete --all --force                # 删除所有任务
orchestrator task delete --all --status completed     # 按状态筛选删除
orchestrator task delete --all --project my-project   # 删除指定项目的所有任务
```

| 标志 | 说明 |
|------|------|
| `-f, --force` | 强制删除，无需确认 |
| `--all` | 删除所有任务 |
| `--status <STATUS>` | 按状态筛选（与 `--all` 配合使用） |
| `--project <PROJECT>` | 按项目筛选（与 `--all` 配合使用） |

## 注意力队列（Attention Queue）

跨任务的人工注意力队列 — 只呈现需要人工决策的工作流状况，按严重度和归属排序。所有队列变更均经过认证、版本校验（`--expected-version`），并接受可安全重试的 `--idempotency-key`。

```bash
orchestrator attention list                                # active inbox
orchestrator attention list --assignee me                  # items assigned to the current actor
orchestrator attention list --state resolved -o json       # audit resolved decisions
orchestrator attention get <id>                            # inspect one item
orchestrator attention claim <id> --expected-version 1
orchestrator attention snooze <id> --expected-version 2 --until 2026-07-13T09:00:00Z
orchestrator attention resolve <id> --expected-version 2 --reason reviewed
orchestrator attention action <id> resume_task --expected-version 1
orchestrator attention follow --after 42                   # stream inbox deltas (NDJSON)
```

| 子命令 | 说明 |
|--------|------|
| `list` | 列出注意力条目，支持可选筛选（`--project`、`--state`、`--kind`、`--severity`、`--assignee`、`--task`、`--limit`）。`--kind` 的完整取值集合见[失败去了哪里](03-workflow-configuration.md#失败去了哪里)中的生成路由表 |
| `get` | 显示脱敏后的状况、乐观版本号、任务上下文和安全的白名单动作 |
| `claim` | 认领一个 open 状态的条目 |
| `snooze` | 将 open 或 claimed 条目推迟到某个 RFC3339 截止时间（`--until`） |
| `resolve` | 携带审计理由关闭条目（`--reason`） |
| `action` | 仅预留并执行条目自身声明的动作，如 `retry_failed_item` 或 `resume_task`（`--input` 提供 JSON 动作输入） |
| `follow` | 从持久化变更序列跟踪单调队列变化（`--after`）；流式输出为 `-o json`（默认，NDJSON）或 `-o yaml` |

## 交接与恢复（Handoff & Resume）

### handoff

生成并检查不可变的任务交接快照，用于在代理或会话之间转移上下文。

```bash
orchestrator handoff generate <task_id>                    # snapshot at the latest event cursor
orchestrator handoff generate <task_id> --cursor 42 -o json  # snapshot at a selected event cursor
orchestrator handoff get <handoff_id>                      # retrieve one snapshot
```

### resume

预览并执行安全的逻辑恢复操作。

```bash
orchestrator resume boundaries <task_id>                   # boundaries + side-effect classifications
orchestrator resume plan <task_id> --boundary <boundary_id> --mode <mode>
orchestrator resume execute <plan_id> --expected-state-version 3 \
  --reason "reviewed preview" --idempotency-key resume-1
```

| 子命令 | 说明 |
|--------|------|
| `resume boundaries` | 列出任务的逻辑边界及其副作用分类 |
| `resume plan` | 在不改变任务或工作区状态的前提下，持久化一个会过期的后果预览（`--attention-item` 可关联注意力条目） |
| `resume execute` | 执行已审阅的计划，带过期状态保护；需要 `--expected-state-version`、`--reason` 和 `--idempotency-key`；提权计划需要 `--elevated-confirmation` |

## 清单

```bash
# 验证清单文件
orchestrator manifest validate -f manifest.yaml
orchestrator manifest validate -f manifest.yaml --project my-project

# 导出所有资源为清单文档
orchestrator manifest export [-o yaml|json]
```

| 标志 (validate) | 说明 |
|-----------------|------|
| `-f, --file` | 清单文件（必填） |
| `-p, --project` | 项目筛选 |

## 密钥管理

```bash
orchestrator secret key status [-o json]
orchestrator secret key list [-o json]
orchestrator secret key rotate [--resume]
orchestrator secret key revoke <key_id> [--force]
orchestrator secret key bootstrap                 # 所有密钥均处于终态时的应急恢复
orchestrator secret key history [-n <limit>] [--key-id <id>] [-o json]
```

## 数据库操作

```bash
orchestrator db status [-o json]
orchestrator db migrations list [-o json]
orchestrator db vacuum                            # 回收磁盘空间（VACUUM）
orchestrator db cleanup                           # 清理已终止任务的旧日志文件
orchestrator db cleanup --older-than 30           # 清理 N 天前的日志（默认 30）
```

## 项目清理

使用 `orchestrator delete project/<id> --force` 进行项目清理。

## 项目管理

项目隔离是原生功能 — 在 `apply`、`get`、`describe`、`delete`、`task create`、`task list` 和 `store` 命令上使用 `--project`。

```bash
# 将资源应用到项目作用域
orchestrator apply -f manifest.yaml --project my-project

# 显式清理 manifest 中未声明的同类资源
orchestrator apply -f manifest.yaml --project my-project --prune

# 查询项目作用域资源
orchestrator get agents --project my-project

# 删除项目及其所有数据（任务、项目、运行、事件、配置）
orchestrator delete project/<project> --force
```

默认 `apply` 是 merge-only 语义：manifest 中缺失的资源会被保留。
只有在你明确希望删除目标项目中、同类但未在本次 manifest 中声明的资源时，才使用 `--prune`。

## 持久化存储

```bash
orchestrator store get <store_name> <key>
orchestrator store put <store_name> <key> <value>
orchestrator store put <store_name> <key> <value> --task-id <id>
orchestrator store delete <store_name> <key>
orchestrator store list <store_name>
orchestrator store list <store_name> --limit 50 --offset 10
orchestrator store prune <store_name>

# 项目作用域存储
orchestrator store get <store_name> <key> --project my-project
orchestrator store put <store_name> <key> <value> --project my-project
```

| 标志 (list) | 说明 |
|-------------|------|
| `-l, --limit` | 结果限制（默认：100） |
| `--offset` | 结果偏移（默认：0） |
| `-o, --output` | 输出格式：table（默认）、json、yaml |
| `-p, --project` | 项目筛选 |

| 标志 (put) | 说明 |
|------------|------|
| `-t, --task-id` | 关联任务 ID |
| `-p, --project` | 项目筛选 |

## 代理生命周期

管理代理调度状态（cordon、drain、uncordon）。

```bash
# 列出代理及其生命周期状态
orchestrator agent list
orchestrator agent list --project my-project -o json

# Cordon：标记代理为不可调度（不再分派新任务）
orchestrator agent cordon <agent_name>
orchestrator agent cordon <agent_name> --project my-project

# Uncordon：将已 cordon 的代理恢复为可调度
orchestrator agent uncordon <agent_name>

# Drain：cordon + 等待进行中的任务完成
orchestrator agent drain <agent_name>
orchestrator agent drain <agent_name> --timeout 60
```

| 子命令 | 说明 |
|--------|------|
| `list` | 列出代理及其生命周期状态 |
| `cordon` | 标记代理为不可调度 |
| `uncordon` | 将已 cordon 的代理恢复为可调度 |
| `drain` | Cordon + 等待进行中的任务完成 |

| 标志 | 说明 |
|------|------|
| `-p, --project` | 项目筛选 |
| `-o, --output`（仅 list） | 输出格式：table（默认）、json、yaml |
| `--timeout`（仅 drain） | 超时秒数；超时后强制 drain |

### 代理会话（Agent Sessions）

观察和控制交互式代理会话。写入控制是一种带围栏（fenced）的租约：写侧变更需要当前的 `--fencing-token`，输入携带可安全重试的 `--idempotency-key`。

```bash
orchestrator agent session list --state detached -o json   # list observable sessions
orchestrator agent session get <session_id>                # lifecycle, process, and lease metadata
orchestrator agent session attach <session_id> --mode writer --client-id terminal-a
orchestrator agent session read <session_id> --offset 0 --chunks-json
orchestrator agent session heartbeat <session_id> --client-id terminal-a --fencing-token 1
orchestrator agent session send-input <session_id> --client-id terminal-a --fencing-token 1 \
  --text hello --idempotency-key input-1
orchestrator agent session detach <session_id> --mode writer --client-id terminal-a --fencing-token 1
orchestrator agent session close <session_id> --reason done --expected-version 2 --idempotency-key close-1
orchestrator agent session resolve --pid 1234 -o json      # diagnostic PID -> sessions (read-only)
```

| 子命令 | 说明 |
|--------|------|
| `list` | 按 `--task`、`--agent`、`--state` 筛选由守护进程权威管理的会话，不暴露传输路径或命令文本 |
| `get` | 显示单个会话的公开生命周期、进程和写入租约元数据 |
| `attach` | 以读者身份附加（`--mode reader`，默认，只读），或显式获取带围栏的写入租约（`--mode writer`，需要 operator 权限和已启用的会话控制策略） |
| `read` | 从客户端自持的 `--offset` 跟踪或读取转录字节；`--chunks-json` 输出带 `next_offset` 的结构化分块，用于可安全重连的流式读取 |
| `heartbeat` | 续期写入租约；只有当前未过期的客户端和 fencing token 才能延长 |
| `send-input` | 使用当前写入方 fencing token 向存活会话发送有界输入 |
| `detach` | 分离读者或写者；写者分离需要与当前 fencing token 完全一致 |
| `close` | 关闭底层会话进程 — 以会话 ID 寻址、版本感知（`--expected-version`）、有审计（`--reason`），绝不凭 PID 单独授权 |
| `resolve` | 将诊断用 PID 解析到会话；只读，绝不产生变更权限 |

## 守护进程生命周期

```bash
orchestrator daemon status                    # 显示守护进程 PID 和状态
orchestrator daemon stop                      # 向守护进程发送 SIGTERM
orchestrator daemon maintenance --enable      # 阻止新任务创建
orchestrator daemon maintenance --disable     # 恢复任务创建
```

## 事件生命周期

```bash
orchestrator event stats                      # 显示事件表统计信息
orchestrator event list --task <task_id>      # 列出某任务的事件
orchestrator event list --task <task_id> --type item --limit 100   # 按事件类型前缀筛选
orchestrator event cleanup                    # 清理旧事件
orchestrator event cleanup --older-than 30    # 清理 N 天前的事件（默认 30）
orchestrator event cleanup --dry-run          # 预览，不实际删除
orchestrator event cleanup --archive          # 删除前归档为 JSONL
```

| 标志 (list) | 说明 |
|-------------|------|
| `--task <TASK>` | 任务标识（必填） |
| `--type <EVENT_TYPE>` | 按事件类型筛选（前缀匹配） |
| `-l, --limit` | 返回的最大事件数（默认：50） |
| `-o, --output` | 输出格式：table（默认）、json、yaml |

## 审计（Audit）

查询规范的控制面板操作审计证据 — 项目作用域的变更记录，用于关联传输层授权、领域变更和事件证据，且不暴露请求体或密钥。

```bash
orchestrator audit list --project demo --status failed     # list failed mutations
orchestrator audit list --project demo --target-type attention_item -o json
orchestrator audit get req-123 --project demo              # one record by request ID
```

| 标志 (list) | 说明 |
|-------------|------|
| `-p, --project` | 项目作用域（必填） |
| `--actor` | 按操作者身份筛选 |
| `--target-type` / `--target-id` | 按变更目标筛选 |
| `--action` | 按动作名筛选 |
| `--status` | 按结果状态筛选 |
| `--from` / `--to` | 时间范围边界 |
| `-l, --limit` | 最大记录数（默认：100） |
| `-o, --output` | 输出格式：table（默认）、json、yaml |

### Apply 动作名

每次非 dry-run 的 `apply` 都会记录一行，无论客户端是否携带审计信封；未携带信封的
客户端以 `reason_code` `legacy_client` 记录。单文档 apply 按其 kind 具名：

```bash
orchestrator audit list --project demo --action resource.secret_store.apply
orchestrator audit list --project demo --target-type workflow
```

`resource.<kind>.apply` 覆盖全部 kind——`resource.workspace.apply`、
`resource.agent.apply`、`resource.workflow.apply`、`resource.project.apply`、
`resource.runtime_policy.apply`、`resource.step_template.apply`、
`resource.execution_profile.apply`、`resource.env_store.apply`、
`resource.secret_store.apply`、`resource.trigger.apply`——另有两个早于该约定、
保持原拼写的名字：`source.template.apply`（SourceTaskTemplate）与
`source.binding.apply`（SourceTaskBinding）。携带 `driver.rawArgs` 的 Agent
清单记为 `agent.driver.raw_args.apply`。

有两种情况记通用的 `resource.apply`、`target_type` 为 `resource_manifest`：
多文档 bundle，以及解析失败的清单。二者都没有单一可解析身份，因此按具名动作
过滤不会返回它们——需要完整序列时请不加 `--action` 列出。bundle 不携带逐文档
kind 清单；如需还原其触及的资源，请按时间戳关联 `resource_versions`。

## 触发器生命周期

```bash
orchestrator trigger suspend <name>           # 挂起触发器
orchestrator trigger resume <name>            # 恢复已挂起的触发器
orchestrator trigger fire <name>              # 手动触发一次
orchestrator trigger fire <name> --payload '{"key":"value"}'   # 携带 JSON payload 触发
```

所有触发器子命令均支持 `--project` 标志用于项目级操作。

## 来源集成（Source Integration）

外部来源事件（如 Slack）及其任务绑定、持久化自动化路由、受治理模板和提供方连接。

### 来源事件（Source events）

```bash
orchestrator source list --state failed                    # list replay candidates
orchestrator source list --project demo --limit 20 -o json
orchestrator source get <source_event_id>                  # one normalized event
orchestrator source ingest --project demo --file event.json  # ingest a normalized fixture
orchestrator source replay <source_event_id>               # requeue one failed generic route
orchestrator source route <source_event_id>                # protected route + Slack deep link
```

| 子命令 | 说明 |
|--------|------|
| `list` | 按 `--project`、`--task`、`--state`、`--limit` 筛选并列出最近的提供方中立来源事件，不暴露原始提供方载荷 |
| `get` | 检查单个归一化事件的路由状态、来源出处和解析出的流程 |
| `ingest` | 持久化插入一个已认证的归一化事件夹具，用于适配器开发和非 Slack 集成测试（需启用运行时来源摄入；`--payload-hash` 可选地固定载荷） |
| `replay` | 仅限管理员的通用来源事件恢复；关联到徽章自动化路由的事件必须改用 `source automation replay` |
| `route` | 检查为某来源事件解析出的受保护自动化路由，含其 Slack 深链接 |

### 来源绑定（Source bindings）

将可信的提供方会话坐标与 orchestrator 任务关联，并控制受治理的 source-to-task 绑定。

```bash
orchestrator source bindings <task_id>                     # bindings correlated with one task
orchestrator source bind --project demo --task <task_id> --provider fixture \
  --installation install-1 --conversation C1 --thread T1 --source-event <event_id>
orchestrator source binding simulate --project demo --installation T1 \
  --reaction agent-analyze --channel C1 --actor U1
orchestrator source binding suspend badge-default --project demo
orchestrator source binding resume badge-default --project demo
```

| 子命令 | 说明 |
|--------|------|
| `bindings` | 列出与单个任务关联的 primary、related 和 notification_target 绑定 |
| `bind` | 创建可信绑定（`--binding-type primary|related|notification_target`，出处通过 `--source-event` 提供） |
| `binding simulate` | 针对调用方提供的证据模拟确定性匹配 — 无副作用，不调用提供方 API |
| `binding suspend` | 立即停止某绑定匹配新事件 |
| `binding resume` | 经与当前活跃绑定的冲突校验后，重新启用已挂起的绑定 |

### 来源自动化（Source automation）

检查和控制持久化徽章自动化路由。运维输出不含 Slack 消息坐标、正文、凭据和永久链接。`replay` 与 `ignore` 是有审计的操作员控制，需要 `--reason`、`--expected-version` 和 `--idempotency-key`。

```bash
orchestrator source automation list --project demo --state needs_attention -o json
orchestrator source automation list --page-size 20 --page-token <token>
orchestrator source automation get <route_id> --attempt-limit 20
orchestrator source automation status --project demo -o json
orchestrator source automation watch --project demo --after 42
orchestrator source automation simulate --project demo --installation T1 \
  --reaction agent-analyze --channel C1 --actor U1 \
  --message-url https://acme.slack.com/archives/C1/p123 --target-id C1:1.23
orchestrator source automation replay <route_id> --expected-version 7 \
  --reason "credential rotated" --idempotency-key replay-1
orchestrator source automation ignore <route_id> --expected-version 8 \
  --reason "obsolete request" --idempotency-key ignore-1
```

| 子命令 | 说明 |
|--------|------|
| `list` | 使用有界 keyset 分页列出安全的路由投影（`--page-size`、`--page-token`；筛选：`--project`、`--state`、`--provider`、`--binding`、`--task`） |
| `get` | 显示单条路由的安全投影和有界的尝试历史（`--attempt-limit`） |
| `status` | 报告积压、最旧时长、活跃租约、重试中路由、Attention 数量和低基数失败族 |
| `watch` | 从持久化变更序列跟踪可重连的路由状态迁移（`--after`）；流式输出为 `-o json`（默认，NDJSON）或 `-o yaml` |
| `simulate` | 用线上同款匹配器和渲染器处理调用方提供的安全证据 — 绝不读取凭据、调用 Slack、预留路由、创建 Attention 或创建任务 |
| `replay` | 从持久化检查点重放一条可操作路由；除非显式指定 `--adopt-current-config`，否则保持固定的配置代际 |
| `ignore` | 有意关闭一条路由而不创建任务，并解决其匹配的 Attention 条目 |

### 来源模板（Source templates）

```bash
orchestrator source template preview badge-default --provider slack \
  --installation T1 --message-url https://acme.slack.com/archives/C1/p123
```

`source template preview` 使用守护进程的活跃配置渲染一个无副作用的样例 — 绝不调用提供方或创建任务。可选证据覆盖：`--event-id`、`--reaction`、`--target-id`。

### 来源连接（Source connections）

管理提供方连接和 OAuth 安装意向。凡触及既有连接的变更均有审计（`--reason`、`--idempotency-key`）和版本校验（`--expected-version`）；会打开 OAuth 的命令接受 `--no-open`，改为打印 URL 而非启动浏览器。

```bash
orchestrator source connection catalog                     # managed/manual provisioning capabilities
orchestrator source connection list -p demo
orchestrator source connection list -p demo --include-disconnected -o json
orchestrator source connection get <connection_id> -p demo
orchestrator source connection watch -p demo --after 42    # stream connection changes (NDJSON)

# Official Slack App OAuth
orchestrator source connection connect -p demo --reason "onboard workspace" --idempotency-key connect-1
orchestrator source connection status <intent_id> -p demo  # poll or resume an OAuth intent
orchestrator source connection cancel <intent_id> -p demo --reason "abandoned flow" --idempotency-key cancel-1
orchestrator source connection reauthorize <connection_id> -p demo --expected-version 2 \
  --reason "scope update" --idempotency-key reauth-1

# Dedicated (workspace-owned) Slack App
orchestrator source connection provision-dedicated -p demo --config-token-stdin \
  --reason "private app" --idempotency-key prov-1
orchestrator source connection dedicated-status <provisioning_id> -p demo
orchestrator source connection dedicated-resume <provisioning_id> -p demo \
  --reason "approve preview" --idempotency-key resume-1
orchestrator source connection dedicated-abandon <provisioning_id> -p demo \
  --reason "wrong workspace" --idempotency-key abandon-1
orchestrator source connection dedicated-upgrade <connection_id> -p demo --expected-version 3 \
  --config-token-stdin --approve --reason "apply manifest fix" --idempotency-key upgrade-1
orchestrator source connection migrate-to-shared <connection_id> -p demo --expected-version 3 \
  --reason "move to official app" --idempotency-key migrate-1
orchestrator source connection dedicated-delete <connection_id> -p demo --expected-version 5 \
  --app-id-confirmation A0123 --reason "decommission" --idempotency-key delete-1

# Connection lifecycle
orchestrator source connection disconnect <connection_id> -p demo --expected-version 2 \
  --reason "offboard workspace" --idempotency-key disc-1
orchestrator source connection transfer <connection_id> -p demo --expected-version 2 \
  --target-daemon-id <daemon_id> --reason "move to prod daemon" --idempotency-key transfer-1
```

| 子命令 | 说明 |
|--------|------|
| `catalog` | 报告守护进程对每个提供方支持哪些托管与手动供给模式 |
| `list` | 列出不暴露凭据的安全连接投影；除非指定 `--include-disconnected`，否则隐藏已断开的连接 |
| `get` | 检查单个连接的安全投影、生命周期状态和版本 |
| `watch` | 跟踪单调的连接变更（`--after`）；流式输出为 `-o json`（默认，NDJSON）或 `-o yaml` |
| `connect` | 启动官方 Slack App OAuth 流程（创建安装意向并打开授权 URL） |
| `status` | 轮询或恢复单个待处理的 OAuth 意向 |
| `cancel` | 取消未完成的 OAuth 意向 |
| `reauthorize` | 为既有连接重新发起 OAuth（例如权限范围变更或凭据被吊销后） |
| `provision-dedicated` | 用标准输入读取的配置令牌（`--config-token-stdin`）验证并供给工作区自有的私有 Slack App；先预览，再 `--approve` |
| `dedicated-status` | 检查专用 App 供给检查点 |
| `dedicated-resume` | 恢复凭据交接，或批准已审阅的专用 App 预览 |
| `dedicated-abandon` | 放弃一个非终态的供给检查点 |
| `dedicated-upgrade` | 审阅并将修正后的清单应用到既有专用 App（先预览，再带 `--approve` 重跑） |
| `migrate-to-shared` | 启动已审阅的专用 App 到官方 App 的迁移 |
| `dedicated-delete` | 永久删除已断开的专用 App；需以 `--app-id-confirmation` 提供 App ID 作为确认 |
| `disconnect` | 断开连接并销毁其托管凭据 |
| `transfer` | 将连接的独占所有权移交给另一个守护进程（`--target-daemon-id`） |

## 调试与系统

```bash
orchestrator debug                   # 检查内部状态
orchestrator debug --component config  # 显示活跃配置
orchestrator version                 # 构建版本 + git 哈希
orchestrator version -o json         # JSON 格式版本输出
orchestrator check                   # 预检验证
orchestrator check -o json           # 结构化检查输出
orchestrator guide                   # 带示例的 CLI 引导参考
orchestrator guide task              # 按命令名筛选
orchestrator guide -c task -f json   # 按类别筛选，JSON 输出
```

### debug sandbox-probe

在不连接守护进程的情况下运行本地沙箱探针 — 用于验证沙箱的资源与网络限制。

```bash
orchestrator debug sandbox-probe write-file --path /tmp/probe.txt
orchestrator debug sandbox-probe open-files --count 256
orchestrator debug sandbox-probe cpu-burn
orchestrator debug sandbox-probe alloc-memory --total-mb 256 --chunk-mb 8
orchestrator debug sandbox-probe spawn-children --count 64 --sleep-secs 60
orchestrator debug sandbox-probe dns-resolve --host example.com --port 443
orchestrator debug sandbox-probe tcp-connect --host 127.0.0.1 --port 8080 --timeout-secs 3
```

## QA 可观测性

```bash
orchestrator qa doctor               # 来自 task_execution_metrics 的可观测性健康指标
orchestrator qa doctor -o json       # 结构化输出
```

## 流程指标（Process Metrics）

Process Console 运维指标。

```bash
orchestrator metrics process -p demo                       # snapshot over the default 24h window
orchestrator metrics process -p demo --window 7d --bucket 1d -o json
orchestrator metrics prune --retention-days 30             # delete optional metrics past retention
orchestrator metrics rebuild -p demo                       # rebuild materialized rollups
```

| 子命令 | 说明 |
|--------|------|
| `process` | 查询单个项目作用域的 Process Console 快照，时间窗口 `--window`（默认：24h），分桶大小 `--bucket` 可配置（默认：1h） |
| `prune` | 删除超过 `--retention-days` 保留阈值的可选指标 |
| `rebuild` | 为单个项目重建保留的物化汇总 |

## 内置工具

供 CRD 插件脚本使用的辅助工具（由触发器/终结插件调用）：`tool webhook-verify-hmac`、`tool payload-extract` 和 `tool secret-rotate`。

```bash
# 验证 HMAC 签名（退出码 0 = 有效，1 = 无效）
orchestrator tool webhook-verify-hmac --secret <secret> --body <body> --signature <sig> [--algo sha256]

# 使用点分路径从 JSON 中提取值（读取标准输入）
echo '{"event":{"type":"push"}}' | orchestrator tool payload-extract --path event.type
orchestrator tool payload-extract --path event.type < payload.json

# 轮换 SecretStore 中的某个密钥（需要守护进程运行）
orchestrator tool secret-rotate <store> <key> --value <new_value> [--project <id>]
```

## 输出格式

所有非流式命令均接受统一的 `-o, --output {table,json,yaml}` 标志：

- 集合类命令（`list` 风格）默认 `table`。
- 单对象读取和变更命令默认 `yaml`。

流式命令（`attention follow`、`source automation watch`、`source connection watch`）接受 `-o {json,yaml}`，默认 `json`，以 NDJSON 形式输出（每行一个 JSON 对象）。

两个有意保留的例外沿用自己的开关：`agent session read --chunks-json`（带重连偏移量的结构化分块输出）和 `guide --format {markdown,json}`。

`--json` 仅在 `version` 和 `task trace` 上作为 `-o json` 的隐藏废弃别名保留一个发布周期 — 请改用 `-o json`。

## 守护进程（C/S 模式）

### orchestratord

运行 gRPC 服务端和内嵌后台工作器的守护进程二进制。

```bash
# 前台启动（推荐用于开发）
./target/release/orchestratord --foreground

# 多工作器
./target/release/orchestratord --foreground --workers 3

# TCP 绑定（远程访问）
./target/release/orchestratord --foreground --bind 0.0.0.0:50051
```

| 标志 | 说明 |
|------|------|
| `--foreground`, `-f` | 前台运行（不后台化） |
| `--bind <addr>` | TCP 绑定地址（默认：Unix 套接字） |
| `--workers <N>` | 后台工作器数量（默认：1） |
| `--insecure-bind <addr>` | 用于开发的不安全 TCP 绑定（feature-gated：`dev-insecure`） |
| `--control-plane-dir <DIR>` | 控制面板证书目录 |
| `--uds-max-role <ROLE>` | 不存在 `uds-policy.yaml` 时 UDS 调用方的最高角色：`read-only`、`operator`、`admin`（默认：operator，环境变量：`ORCHESTRATOR_UDS_MAX_ROLE`） |
| `--event-retention-days <DAYS>` | 事件保留天数（默认：30，0 = 禁用） |
| `--event-cleanup-interval-secs <SECS>` | 清理扫描间隔秒数（默认：3600） |
| `--event-archive-enabled` | 清理前将事件归档为 JSONL |
| `--event-archive-dir <DIR>` | 覆盖事件归档目录 |
| `--log-retention-days <DAYS>` | 自动清理前日志文件的保留天数（默认：30，0 = 禁用） |
| `--task-retention-days <DAYS>` | 自动清理前已终止任务的保留天数（默认：0 = 禁用） |
| `--stall-timeout-mins <MINS>` | 运行中项目被视为停滞的分钟数（默认：30，0 = 禁用） |
| `--webhook-bind <ADDR>` | HTTP webhook 服务绑定地址（默认：`127.0.0.1:19090`，`none` 禁用）。非回环地址需要配置密钥。 |
| `--webhook-secret <SECRET>` | Webhook HMAC-SHA256 签名验证密钥（环境变量：`ORCHESTRATOR_WEBHOOK_SECRET`） |
| `--webhook-allow-unsigned` | 允许非回环地址无签名验证启动 webhook（环境变量：`ORCHESTRATOR_WEBHOOK_ALLOW_UNSIGNED`） |

### control-plane issue-client

为连接守护进程控制面板颁发客户端 TLS 证书材料：

```bash
orchestratord control-plane issue-client \
  --bind <addr> --subject <name> [--role <role>]
```

`--role` 可取 `read-only`、`operator`（默认）或 `admin`。可选的
`--home` 与 `--control-plane-dir` 用于覆盖证书位置。

### webhook-secret

打印从控制面板 CA 证书派生出的 webhook HMAC 密钥。

```bash
orchestratord webhook-secret
orchestratord webhook-secret --control-plane-dir <dir>
```

### 守护进程管理

```bash
./target/release/orchestratord --foreground --workers 2   # 前台运行（推荐）
nohup ./target/release/orchestratord --foreground &       # 后台运行
orchestrator daemon stop                                  # 优雅关闭（SIGTERM）
```

### C/S CLI 命令列表

所有命令通过 Unix 套接字连接守护进程：

```bash
# 资源管理（--project 用于项目作用域）
orchestrator apply -f manifest.yaml [--project <id>] [--dry-run]
orchestrator get <resource> [-o json|yaml] [--project <id>]
orchestrator describe <kind/name> [--project <id>]
orchestrator delete <kind/name> --force [--project <id>]

# 任务生命周期
orchestrator task create --name X --goal Y [--project <id>] [--workflow Z] [--step S] [--set k=v]
orchestrator run --workflow Z [--step S] [--set k=v]          # 同步执行
orchestrator run --template T --agent-capability C [--set k=v] # 直接组装模式
orchestrator task list [-o json] [--project <id>] [--status <s>]
orchestrator task items <id> [--status <s>] [-o json]
orchestrator task info <id> [-o json]
orchestrator task start <id>
orchestrator task pause <id>
orchestrator task resume <id>
orchestrator task logs <id> [--tail N] [--follow]
orchestrator task watch <id>
orchestrator task trace <id> [--verbose]
orchestrator task retry <item_id> [--force]
orchestrator task recover <id>
orchestrator task timeline <id> [--category <c>] [--follow] [-o json]
orchestrator task delete <id> --force

# 注意力队列
orchestrator attention list [--state <s>] [--assignee me] [-o json]
orchestrator attention get <id>
orchestrator attention claim|snooze|resolve|action <id> --expected-version <v>
orchestrator attention follow [--after <seq>]

# 交接与恢复
orchestrator handoff generate <task_id> [--cursor <n>]
orchestrator handoff get <handoff_id>
orchestrator resume boundaries <task_id>
orchestrator resume plan <task_id> --boundary <b> --mode <m>
orchestrator resume execute <plan_id> --expected-state-version <v> --reason <r> --idempotency-key <k>

# 代理生命周期
orchestrator agent list [--project <id>] [-o json|yaml]
orchestrator agent cordon <agent_name> [--project <id>]
orchestrator agent uncordon <agent_name> [--project <id>]
orchestrator agent drain <agent_name> [--project <id>] [--timeout <secs>]

# 代理会话
orchestrator agent session list|get|attach|read|heartbeat|send-input|detach|close|resolve

# 触发器生命周期
orchestrator trigger suspend|resume|fire <name> [--project <id>] [--payload <json>]

# 来源集成
orchestrator source list|get|ingest|replay|route|bind|bindings
orchestrator source binding simulate|suspend|resume
orchestrator source automation list|get|status|watch|simulate|replay|ignore
orchestrator source template preview <name> --provider <p> --installation <i> --message-url <url>
orchestrator source connection list|get|watch|catalog|connect|status|cancel|reauthorize
orchestrator source connection provision-dedicated|dedicated-status|dedicated-resume|dedicated-abandon
orchestrator source connection dedicated-upgrade|migrate-to-shared|dedicated-delete|disconnect|transfer

# 项目清理
orchestrator delete project/<id> --force

# 存储（--project 用于项目作用域）
orchestrator store put <store> <key> <value> [--project <id>]
orchestrator store get <store> <key> [--project <id>]
orchestrator store list <store> [-o json] [--project <id>]
orchestrator store delete <store> <key> [--project <id>]
orchestrator store prune <store> [--project <id>]

# 清单
orchestrator manifest validate -f <file>
orchestrator manifest export [-o yaml|json]

# 密钥管理
orchestrator secret key status|list|rotate|revoke|bootstrap|history

# 数据库
orchestrator db status [-o json]
orchestrator db migrations list [-o json]
orchestrator db vacuum
orchestrator db cleanup [--older-than <days>]

# 事件
orchestrator event stats
orchestrator event list --task <id> [-o json]
orchestrator event cleanup [--older-than <days>] [--dry-run] [--archive]

# 审计证据
orchestrator audit list --project <id> [--actor <a>] [--status <s>]
orchestrator audit get <request_id> --project <id>

# 流程指标
orchestrator metrics process --project <id> [--window <w>] [--bucket <b>]
orchestrator metrics prune [--retention-days <n>]
orchestrator metrics rebuild --project <id>

# 守护进程生命周期
orchestrator daemon status|stop
orchestrator daemon maintenance --enable|--disable

# QA 与工具
orchestrator qa doctor [-o json]
orchestrator tool webhook-verify-hmac|payload-extract|secret-rotate

# 系统
orchestrator version
orchestrator debug [--component config]
orchestrator check [-o json] [--workflow <w>]
orchestrator init [<root>]
```

## 资源元数据

所有资源支持 `metadata.labels`（用于分类和标签选择器查询的键值对）和 `metadata.annotations`（任意键值元数据）。两者均为可选。

```yaml
metadata:
  name: my-resource
  labels:
    env: dev
    team: platform
  annotations:
    note: "created for sprint 12"
```

## 结构化代理输出

代理必须在 stdout 上产生符合以下模式的 JSON：

```json
{
  "confidence": 0.95,
  "quality_score": 0.9,
  "artifacts": [
    {
      "kind": "analysis",
      "findings": [
        {
          "title": "finding-name",
          "description": "详情",
          "severity": "info"
        }
      ]
    }
  ]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `confidence` | `float` | 代理对结果的置信度（0.0–1.0） |
| `quality_score` | `float` | 质量评估（0.0–1.0） |
| `artifacts` | `array` | 结构化输出产物 |
| `artifacts[].kind` | `string` | `analysis`、`code_change` 等 |
| `artifacts[].findings` | `array` | 发现列表，含 title/description/severity |
| `artifacts[].files` | `array` | 修改的文件列表（用于 code_change） |

此输出被解析为 `AgentOutput`，用于预钩子变量注入（`qa_confidence`、`qa_quality_score`）和终结规则评估。
