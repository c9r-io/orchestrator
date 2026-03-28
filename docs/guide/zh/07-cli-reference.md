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
| `apply` | `ap` |
| `get` | `g` |
| `describe` | `desc` |
| `delete` | `rm` |
| `event` | `ev` |
| `task` | `t` |
| `task list` | `task ls` |
| `task create` | `task new` |
| `task info` | `task get` |
| `task logs` | `task log` |
| `task delete` | `task rm` |
| `check` | `ck` |
| `debug` | `dbg` |
| `store list` | `store ls` |
| `agent` | `ag` |
| `agent list` | `agent ls` |
| `trigger` | `tg` |
| `secret key list` | `secret key ls` |
| `db migrations list` | `db migrations ls` |

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
orchestrator task trace <task_id> --verbose --json
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
| `--json` | JSON 格式输出 |

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
orchestrator secret key history [-n <limit>] [--key-id <id>] [-o json]
```

## 数据库操作

```bash
orchestrator db status [-o json]
orchestrator db migrations list [-o json]
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
orchestrator event cleanup                    # 清理旧事件
orchestrator event cleanup --older-than 30    # 清理 N 天前的事件（默认 30）
orchestrator event cleanup --dry-run          # 预览，不实际删除
orchestrator event cleanup --archive          # 删除前归档为 JSONL
```

## 触发器生命周期

```bash
orchestrator trigger suspend <name>           # 挂起触发器
orchestrator trigger resume <name>            # 恢复已挂起的触发器
orchestrator trigger fire <name>              # 手动触发一次
orchestrator trigger fire <name> --payload '{"key":"value"}'   # 携带 JSON payload 触发
```

所有触发器子命令均支持 `--project` 标志用于项目级操作。

## 调试与系统

```bash
orchestrator debug                   # 检查内部状态
orchestrator debug --component config  # 显示活跃配置
orchestrator version                 # 构建版本 + git 哈希
orchestrator version --json          # JSON 格式版本输出
orchestrator check                   # 预检验证
orchestrator check -o json           # 结构化检查输出
```

## 输出格式

大多数 `get` 和 `info` 命令支持 `-o` 输出格式：

```bash
-o json    # JSON 输出
-o yaml    # YAML 输出
# （默认）  # 表格输出
```

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
| `--event-retention-days <DAYS>` | 事件保留天数（默认：30，0 = 禁用） |
| `--event-cleanup-interval-secs <SECS>` | 清理扫描间隔秒数（默认：3600） |
| `--event-archive-enabled` | 清理前将事件归档为 JSONL |
| `--event-archive-dir <DIR>` | 覆盖事件归档目录 |
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
orchestrator task create --name X --goal Y [--project <id>] [--workflow Z]
orchestrator task list [-o json] [--project <id>] [--status <s>]
orchestrator task info <id> [-o json]
orchestrator task start <id>
orchestrator task pause <id>
orchestrator task resume <id>
orchestrator task logs <id> [--tail N] [--follow]
orchestrator task watch <id>
orchestrator task trace <id> [--verbose]
orchestrator task retry <item_id> [--force]
orchestrator task delete <id> --force

# 代理生命周期
orchestrator agent list [--project <id>] [-o json|yaml]
orchestrator agent cordon <agent_name> [--project <id>]
orchestrator agent uncordon <agent_name> [--project <id>]
orchestrator agent drain <agent_name> [--project <id>] [--timeout <secs>]

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
orchestrator secret key status|list|rotate|revoke|history

# 数据库
orchestrator db status [-o json]
orchestrator db migrations list [-o json]

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
