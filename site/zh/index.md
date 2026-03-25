---
layout: home

hero:
  name: Agent Orchestrator
  text: 为 Agent 而生，由 Agent 驱动
  tagline: 一个编排平台，让任何基于 Shell 的 AI Agent 可以通过声明式 YAML 清单协作完成复杂的多步骤软件开发任务。
  actions:
    - theme: brand
      text: 快速开始
      link: /zh/guide/quickstart
    - theme: alt
      text: 示例
      link: /zh/showcases/benchmark-multi-model-execution
    - theme: alt
      text: GitHub
      link: https://github.com/c9r-io/orchestrator

features:
  - icon: "\U0001F916"
    title: Agent 优先设计
    details: CLI 输出机器可解析的 JSON，配置是声明式 YAML，Skills 提供结构化的执行计划供 Agent 自主遵循。任何基于 Shell 的 Agent 都可以参与协作。
  - icon: "\U0001F91D"
    title: 多 Agent 协作
    details: 具备不同能力的 Agent（规划、实现、测试、审查）通过编排器协作。内置能力匹配、健康评分和轮换机制。
  - icon: "\U0001F4CB"
    title: 声明式工作流
    details: 通过 YAML 定义多步骤工作流，支持循环控制、守卫步骤、DAG 执行和动态步骤池。无需编写命令式代码。
  - icon: "\u26A1"
    title: CEL 前置钩子
    details: 通过 CEL 表达式实现动态控制流 — 运行时决策：Run、Skip、Branch、DynamicAdd、Transform。
  - icon: "\U0001F512"
    title: 内置安全
    details: mTLS 认证、RBAC 授权、沙箱执行（macOS Seatbelt / Linux 命名空间）、输出脱敏。
  - icon: "\U0001F310"
    title: 任意 Agent，任意模型
    details: "Claude Code、OpenCode、Codex、Gemini CLI — 任何接受 prompt 并执行 Shell 命令的工具都可以作为编排器 Agent。通过环境变量切换模型。"
---

## 快速开始

直接运行以下命令，或让你的 AI 编码 Agent 代为执行 — CLI 为两者而设计。

```bash
# 安装
curl -fsSL https://raw.githubusercontent.com/c9r-io/orchestrator/main/install.sh | sh

# 或通过 Homebrew
brew install c9r-io/tap/orchestrator

# 或通过 Cargo
cargo install orchestrator-cli orchestratord
```

```bash
# 启动守护进程并运行第一个任务
orchestratord --foreground --workers 2 &
orchestrator init
orchestrator apply -f manifest.yaml
orchestrator task create --goal "My first QA run"
orchestrator task list
```
