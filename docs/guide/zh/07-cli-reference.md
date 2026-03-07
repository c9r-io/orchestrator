# 07 - CLI 参考

Agent Orchestrator CLI 全部命令速查。

**入口**：`./scripts/orchestrator.sh <command>`（推荐）

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
./scripts/orchestrator.sh init
```

### apply

从 YAML 清单加载资源到数据库。

```bash
# 从文件
./scripts/orchestrator.sh apply -f manifest.yaml

# 从标准输入
cat manifest.yaml | ./scripts/orchestrator.sh apply -f -

# 试运行（仅验证）
./scripts/orchestrator.sh apply -f manifest.yaml --dry-run

# 项目级应用
./scripts/orchestrator.sh apply -f manifest.yaml --project my-project
```

### check

预检验证：交叉引用代理、工作流和模板。

```bash
./scripts/orchestrator.sh check
```

## 资源查询

### get

列出资源（kubectl 风格）。

```bash
./scripts/orchestrator.sh get workspaces
./scripts/orchestrator.sh get agents
./scripts/orchestrator.sh get workflows

# 输出格式
./scripts/orchestrator.sh get agents -o json
./scripts/orchestrator.sh get agents -o yaml

# 标签选择器
./scripts/orchestrator.sh get workspaces -l env=dev,team=platform
```

### describe

单个资源的详细视图。

```bash
./scripts/orchestrator.sh describe workspace default
./scripts/orchestrator.sh describe agent coder
./scripts/orchestrator.sh describe workflow self-bootstrap
```

### delete

按 kind/name 删除资源。

```bash
./scripts/orchestrator.sh delete workspace my-ws
./scripts/orchestrator.sh delete agent old-agent
```

## 工作区

```bash
./scripts/orchestrator.sh workspace info default          # 位置参数
./scripts/orchestrator.sh workspace create --help
```

## 代理

```bash
./scripts/orchestrator.sh agent create --help
```

## 工作流

```bash
./scripts/orchestrator.sh workflow create --help
```

## 任务生命周期

### task create

```bash
./scripts/orchestrator.sh task create \
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
./scripts/orchestrator.sh task list
./scripts/orchestrator.sh task list -o json

./scripts/orchestrator.sh task info <task_id>
./scripts/orchestrator.sh task info <task_id> -o yaml
```

### task start / pause / resume

```bash
./scripts/orchestrator.sh task start <task_id>
./scripts/orchestrator.sh task start <task_id> --detach

./scripts/orchestrator.sh task pause <task_id>
./scripts/orchestrator.sh task resume <task_id>
```

### task logs / watch / trace

```bash
# 查看执行日志
./scripts/orchestrator.sh task logs <task_id>

# 实时监控（自动刷新状态面板）
./scripts/orchestrator.sh task watch <task_id>

# 执行追踪与异常检测
./scripts/orchestrator.sh task trace <task_id>
```

### task retry

重试失败的任务项。

```bash
./scripts/orchestrator.sh task retry <task_id> --item <item_id> --force
```

### task edit

向运行中任务的执行计划插入步骤。

```bash
./scripts/orchestrator.sh task edit --help
```

### task delete

```bash
./scripts/orchestrator.sh task delete <task_id>
```

### task worker

处理分离任务的后台工作器。

```bash
./scripts/orchestrator.sh task worker start
./scripts/orchestrator.sh task worker --help
```

### task session

附加任务执行的会话管理。

```bash
./scripts/orchestrator.sh task session list
./scripts/orchestrator.sh task session info <session_id>
./scripts/orchestrator.sh task session close <session_id>
```

## Exec

在任务步骤上下文中执行命令。

```bash
./scripts/orchestrator.sh exec --help

# 交互模式
./scripts/orchestrator.sh exec -it <task_id> <step_id>
```

## 清单与编辑

```bash
# 导出所有配置为 YAML
./scripts/orchestrator.sh manifest export

# 交互式编辑资源（打开 $EDITOR）
./scripts/orchestrator.sh edit workspace default
./scripts/orchestrator.sh edit workflow self-bootstrap
```

## 数据库

```bash
# 重置数据库（破坏性 —— 需要 --force）
./scripts/orchestrator.sh db reset --force
./scripts/orchestrator.sh db reset --force --include-config
```

**警告**：`db reset` 是破坏性操作。使用 `qa project reset` 进行隔离的清理。

## QA 项目管理

```bash
# 重置项目（隔离的 —— 不影响其他项目）
./scripts/orchestrator.sh qa project reset <project> --keep-config --force

# 创建新项目脚手架
./scripts/orchestrator.sh qa project create <project> --force

# QA 诊断 —— 验证并发保护措施
./scripts/orchestrator.sh qa doctor
```

## 持久化存储

```bash
./scripts/orchestrator.sh store get <store_name> <key>
./scripts/orchestrator.sh store put <store_name> <key> <value>
./scripts/orchestrator.sh store delete <store_name> <key>
./scripts/orchestrator.sh store list <store_name>
./scripts/orchestrator.sh store prune <store_name>
```

## 配置生命周期

```bash
# 显示自修复审计日志
./scripts/orchestrator.sh config heal-log

# 回填旧事件中缺失的 step_scope
./scripts/orchestrator.sh config backfill-events --force
```

## 调试与验证

```bash
./scripts/orchestrator.sh debug           # 检查内部状态
./scripts/orchestrator.sh verify          # 运行验证检查
./scripts/orchestrator.sh version         # 构建版本 + git 哈希
```

## Shell 补全

```bash
# 生成补全脚本（bash/zsh/fish）
./scripts/orchestrator.sh completion bash > ~/.bash_completion.d/orchestrator
./scripts/orchestrator.sh completion zsh > ~/.zfunc/_orchestrator
```

## 输出格式

大多数 `get` 和 `info` 命令支持 `-o` 输出格式：

```bash
-o json    # JSON 输出
-o yaml    # YAML 输出
# （默认）  # 表格输出
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
