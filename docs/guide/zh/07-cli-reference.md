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

## 命令别名

| 命令 | 别名 |
|------|------|
| `apply` | `ap` |
| `get` | `g` |
| `describe` | `desc` |
| `delete` | `rm` |
| `task` | `t` |
| `task list` | `task ls` |
| `task create` | `task new` |
| `task info` | `task get` |
| `task logs` | `task log` |
| `task delete` | `task rm` |
| `project` | `proj` |
| `check` | `ck` |
| `debug` | `dbg` |
| `store list` | `store ls` |

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
```

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
```

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

# 项目作用域
orchestrator delete agent/old --force --project my-project
```

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
| `--detach` | 加入后台工作器队列 |

### task list / info

```bash
orchestrator task list
orchestrator task list -o json
orchestrator task list --project my-project    # 按项目筛选

orchestrator task info <task_id>
orchestrator task info <task_id> -o yaml
```

### task start / pause / resume

```bash
orchestrator task start <task_id>
orchestrator task start <task_id> --detach

orchestrator task pause <task_id>
orchestrator task resume <task_id>
```

### task logs / watch / trace

```bash
# 查看执行日志
orchestrator task logs <task_id>

# 实时监控（自动刷新状态面板）
orchestrator task watch <task_id>

# 执行追踪与异常检测
orchestrator task trace <task_id>
```

### task retry

重试失败的任务项。

```bash
orchestrator task retry <task_id> --item <item_id> --force
```

### task delete

```bash
orchestrator task delete <task_id> --force
```

## 清单

```bash
# 验证清单文件
orchestrator manifest validate -f manifest.yaml
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
orchestrator store delete <store_name> <key>
orchestrator store list <store_name>
orchestrator store prune <store_name>

# 项目作用域存储
orchestrator store get <store_name> <key> --project my-project
orchestrator store put <store_name> <key> <value> --project my-project
```

## 调试与系统

```bash
orchestrator debug                   # 检查内部状态
orchestrator debug --component config  # 显示活跃配置
orchestrator version                 # 构建版本 + git 哈希
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

### 守护进程管理

```bash
./target/release/orchestratord --foreground --workers 2   # 前台运行（推荐）
nohup ./target/release/orchestratord --foreground &       # 后台运行
kill $(cat data/daemon.pid)                               # 优雅关闭（SIGTERM）
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
orchestrator task create --name X --goal Y [--project <id>] [--workflow Z] [--detach]
orchestrator task list [-o json] [--project <id>] [--status <s>]
orchestrator task info <id> [-o json]
orchestrator task start <id> [--detach]
orchestrator task pause <id>
orchestrator task resume <id> [--detach]
orchestrator task logs <id> [--tail N] [--follow]
orchestrator task watch <id>
orchestrator task trace <id> [--verbose]
orchestrator task retry <item_id> [--detach] [--force]
orchestrator task delete <id> --force

# 项目清理
orchestrator delete project/<id> --force

# 存储（--project 用于项目作用域）
orchestrator store put <store> <key> <value> [--project <id>]
orchestrator store get <store> <key> [--project <id>]
orchestrator store list <store> [-o json] [--project <id>]
orchestrator store delete <store> <key> [--project <id>]
orchestrator store prune <store> [--project <id>]

# 系统
orchestrator version
orchestrator debug [--component config]
orchestrator check [-o json] [--workflow <w>]
orchestrator init [<root>]
orchestrator manifest validate -f <file>
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
