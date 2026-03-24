# 为什么选择 Agent Orchestrator?

Agent Orchestrator 专为 **AI 原生软件开发生命周期自动化**而构建。与通用工作流引擎不同，它将 AI Agent 视为开发流程中的一等公民。

## 对比

| 特性 | Agent Orchestrator | Airflow | Prefect | n8n | Dagger |
|------|-------------------|---------|---------|-----|--------|
| **核心定位** | AI 原生 SDLC 自动化 | 数据管道调度 | 数据工作流编排 | 低代码自动化 | CI/CD 管道 |
| **Agent 编排** | 内置：能力匹配、健康评分、轮换 | 不适用 | 不适用 | 不适用 | 不适用 |
| **控制流** | CEL 表达式（Run/Skip/Branch/DynamicAdd/Transform） | Python DAGs | Python 装饰器 | 可视化流程 | Go/Python SDK |
| **安全模型** | mTLS + RBAC + 沙箱（Seatbelt/命名空间）+ 输出脱敏 | 连接级认证 | API 密钥 | 基本认证 | 容器隔离 |
| **部署方式** | 单二进制 + 内嵌 SQLite | 调度器 + Worker + 元数据库 | 服务端 + Worker + 数据库 | 服务端 + 数据库 | 容器引擎 |
| **配置方式** | 声明式 YAML 清单 | Python 代码 | Python 代码 | JSON（可视化编辑器） | Go/Python 代码 |
| **为 AI Agent 设计** | 是 — 启动、监控、评分、轮换 Agent | 否 | 否 | 否 | 否 |

## 核心差异化优势

### AI Agent 作为一等公民

步骤声明 `required_capability`，Agent 声明 `capabilities`，编排器自动进行健康感知的匹配和轮换。

```yaml
kind: Agent
metadata:
  name: tester
spec:
  capabilities: [qa_testing]
  command: claude -p "{prompt}" --verbose
```

### CEL 驱动的动态控制流

通过 Common Expression Language 实现运行时决策 — 无需修改代码。

```yaml
prehook:
  expression: |
    pipeline.step_outputs["scan"].exit_code == 0
      ? "run"
      : "skip"
```

### 声明式，非命令式

所有配置都是 YAML 清单，通过 `orchestrator apply -f` 应用。无需 Python、Go 或 SDK 绑定。

### 单二进制部署

`orchestratord` 是一个 Rust 单二进制文件，内嵌 SQLite。无需外部数据库、消息队列或容器运行时。

### 内置安全

- **mTLS**：守护进程通信的双向 TLS 认证
- **RBAC**：gRPC 端点的角色访问控制
- **沙箱**：macOS Seatbelt 配置文件或 Linux 命名空间隔离
- **脱敏**：日志中自动过滤密钥/令牌/密码
