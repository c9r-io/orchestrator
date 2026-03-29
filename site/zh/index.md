---
layout: home

hero:
  name: Agent Orchestrator
  text: "Harness Engineering Control Plane"
  tagline: "一个本地优先的控制面，把 Claude Code、Codex、OpenCode、Gemini CLI 和各类 shell 工具纳入同一个可治理的工程 harness，用于承载长时间运行的软件交付闭环。"
  actions:
    - theme: brand
      text: 阅读愿景
      link: /zh/guide/vision
    - theme: alt
      text: 快速开始
      link: /zh/guide/quickstart
    - theme: alt
      text: GitHub
      link: https://github.com/c9r-io/orchestrator

features:
  - icon: "\U0001F916"
    title: "原生面向 Harness Engineering"
    details: "它围绕一个核心前提构建：人类定义目标和约束，agent 在 workflow、skills、trigger 和 guardrail 组成的 harness 中执行。"
  - icon: "\U0001F91D"
    title: "统一 Agent 运行时"
    details: "Claude Code、Codex、OpenCode、Gemini CLI 以及其他 shell-based 工具，都可以通过 capability 和 command template 纳入同一个运行时。"
  - icon: "\U0001F4CB"
    title: "声明式控制面"
    details: "Workspace、Agent、Workflow、Trigger、Secret、Policy 都以可版本化的 YAML 资源定义，而不是散落在脚本和临时 prompt 里。"
  - icon: "\u26A1"
    title: "长时间运行工作流闭环"
    details: "plan、implement、test、review、fix 可以作为带状态的闭环持续运行，支持 repeatable steps、DAG、dynamic step pool 和 trigger 驱动任务创建。"
  - icon: "\U0001F512"
    title: "Policy 与恢复护栏"
    details: "mTLS、RBAC、沙箱、secret 生命周期、loop guard、持久任务状态，让 agent 执行具备更强的工程边界与恢复能力。"
  - icon: "\U0001F310"
    title: "默认可观测"
    details: "结构化事件、执行日志、任务状态、机器可读输出，让长时间运行的 agent workflow 可以被审计、追踪和运维。"
---

## 这是什么

Agent Orchestrator 不是单纯的 agent wrapper，而是面向软件工程系统的 harness 层。

OpenAI 最近把这种思路称为 [Harness Engineering](https://openai.com/index/harness-engineering/)：工程师把更多精力放在环境、反馈回路和控制系统上，让 agent 能持续完成可靠工作。这个项目就是把这套方法落到基于 shell 的 coding agent 和本地优先的软件交付流程上。

## 这能带来什么

- 把零散的 agent 用法沉淀为可复用的 manifest、skills 和 workflow 资产
- 跑持续的 plan -> implement -> test -> review -> fix 闭环，而不是一次性调用 agent
- 在底层模型、shell 和 prompt 快速变化时，仍然保留稳定的控制面

## 快速开始

直接运行以下命令，或让你的 AI 编码 Agent 代为执行。你启动的是一个本地控制面，而不只是一次 CLI 调用。

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
