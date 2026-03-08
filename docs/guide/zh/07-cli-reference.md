# 07 - CLI 参考

Agent Orchestrator CLI 全部命令速查。

## 入口

| 模式 | 命令 | 说明 |
|------|------|------|
| 单体 | `orchestrator <command>` | 传统单进程 CLI |
| C/S 守护进程 | `./target/release/orchestratord [flags]` | gRPC 服务端 + 内嵌工作器 |
| C/S 客户端 | `./target/release/orchestrator <command>` | 轻量 gRPC 客户端 |

**单体模式**在单进程中运行一切。**C/S 模式**将守护进程（状态、数据库、工作器）与 CLI 客户端（通过 Unix 套接字的 gRPC 调用）分离。

## 全局选项

| 标志 | 说明 |
|------|------|
| `-v, --verbose` | 启用详细输出 |
| `--log-level <LEVEL>` | 覆盖日志级别：`error`、`warn`、`info`、`debug`、`trace` |
| `--log-format <FORMAT>` | 控制台日志格式：`pretty`、`json` |
| `--unsafe` | 绕过所有 `--force` 门控并将运行器策略覆盖为 Unsafe |
| `-h, --help` | 打印帮助 |
| `-V, --version` | 打印版本 |

## 命令别名

多个命令提供简短别名：

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
| `workspace` | `ws` |
| `manifest` | `m` |
| `edit` | `e` |
| `completion` | `comp` |
| `config` | `cfg` |
| `check` | `ck` |
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

# 标签选择器
orchestrator get workspaces -l env=dev,team=platform
```

### describe

单个资源的详细视图。

```bash
orchestrator describe workspace default
orchestrator describe agent coder
orchestrator describe workflow self-bootstrap
```

### delete

按 kind/name 删除资源。

```bash
orchestrator delete workspace my-ws
orchestrator delete agent old-agent
```

## 工作区

```bash
orchestrator workspace info default          # 位置参数
orchestrator workspace create --help
```

## 代理

```bash
orchestrator agent create --help
```

## 工作流

```bash
orchestrator workflow create --help
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

### task edit

向运行中任务的执行计划插入步骤。

```bash
orchestrator task edit --help
```

### task delete

```bash
orchestrator task delete <task_id>
```

### task worker（单体模式）

处理分离任务的后台工作器（仅限单体模式）。

```bash
orchestrator task worker start
orchestrator task worker start --poll-ms 500 --workers 3
orchestrator task worker stop
orchestrator task worker status
```

> **C/S 模式**：工作器内嵌于守护进程。使用 `orchestratord --workers N` 替代，无需单独的 worker 命令。

### task session

附加任务执行的会话管理。

```bash
orchestrator task session list
orchestrator task session info <session_id>
orchestrator task session close <session_id>
```

## Exec

在任务步骤上下文中执行命令。

```bash
orchestrator exec --help

# 交互模式
orchestrator exec -it <task_id> <step_id>
```

## 清单与编辑

```bash
# 导出所有配置为 YAML
orchestrator manifest export

# 交互式编辑资源（打开 $EDITOR）
orchestrator edit workspace default
orchestrator edit workflow self-bootstrap
```

## 数据库

```bash
# 重置数据库（破坏性 —— 需要 --force）
orchestrator db reset --force
orchestrator db reset --force --include-config
```

**警告**：`db reset` 是破坏性操作。使用 `project reset` 进行隔离的清理。

## 项目管理

```bash
# 重置项目（隔离的 —— 不影响其他项目）
orchestrator project reset <project> --force

# QA 诊断 —— 验证并发保护措施
orchestrator qa doctor
```

## 持久化存储

```bash
orchestrator store get <store_name> <key>
orchestrator store put <store_name> <key> <value>
orchestrator store delete <store_name> <key>
orchestrator store list <store_name>
orchestrator store prune <store_name>
```

## 配置生命周期

```bash
# 显示自修复审计日志
orchestrator config heal-log

# 回填旧事件中缺失的 step_scope
orchestrator config backfill-events --force
```

## 调试与验证

```bash
orchestrator debug           # 检查内部状态
orchestrator verify          # 运行验证检查
orchestrator version         # 构建版本 + git 哈希
```

## Shell 补全

```bash
# 生成补全脚本（bash/zsh/fish）
orchestrator completion bash > ~/.bash_completion.d/orchestrator
orchestrator completion zsh > ~/.zfunc/_orchestrator
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

### 守护进程管理（通过 CLI 客户端）

```bash
./target/release/orchestrator daemon start     # 后台启动守护进程
./target/release/orchestrator daemon status     # 检查是否运行
./target/release/orchestrator daemon stop       # 优雅关闭
./target/release/orchestrator daemon restart    # 停止 + 启动
```

### C/S CLI 命令列表

以下命令通过 Unix 套接字连接守护进程：

```bash
# 资源管理
./target/release/orchestrator apply -f manifest.yaml
./target/release/orchestrator get workspaces -o json
./target/release/orchestrator describe workspace/default -o yaml
./target/release/orchestrator delete workspace/old --force

# 任务生命周期
./target/release/orchestrator task create --name "test" --goal "goal" --detach
./target/release/orchestrator task list -o json
./target/release/orchestrator task info <task_id>
./target/release/orchestrator task start <task_id> --detach
./target/release/orchestrator task pause <task_id>
./target/release/orchestrator task logs <task_id> --tail 50

# 持久化存储
./target/release/orchestrator store put <store> <key> <value>
./target/release/orchestrator store get <store> <key>
./target/release/orchestrator store list <store> -o json

# 系统
./target/release/orchestrator version
./target/release/orchestrator debug --component config
./target/release/orchestrator check -o json
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
