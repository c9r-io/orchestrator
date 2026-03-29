# 00 - 愿景

Agent Orchestrator 的目标是成为一个面向 agent-first 软件交付的 **Harness Engineering control plane**。

OpenAI 最近用 [Harness Engineering](https://openai.com/index/harness-engineering/) 这个概念来描述一种新的工程方式：人类减少直接手写代码，把更多精力放在设计环境、定义意图、建立反馈闭环，让 agent 可以稳定地产出可靠结果。这个项目直接采用这套 framing。

## 我们在构建什么

- 一个本地优先的控制面，把 Claude Code、Codex、OpenCode、Gemini CLI 以及其他基于 shell 的工具统一抽象到同一套资源模型之下
- 一个声明式 harness，把 workflow、agent、trigger、secret、policy、observability 都沉淀成仓库内可版本化的资产
- 一个长时间运行的执行内核，可以持续推进 plan -> implement -> test -> review -> fix 这样的工程闭环

## 为什么是 Control Plane？

一个真正可用的工程 harness，不只是“调用一下 agent”。它需要决定：

- 由哪个 agent 执行
- 在哪个 workspace 执行
- 执行 workflow 的哪个 step
- 带着哪些 guardrail 和 secret
- 使用什么重试、暂停、恢复策略
- 如何把结果沉淀为可审计的状态、日志与后续动作

这正是本项目要承担的角色。

## 核心信念

- **人类负责 steering，agent 负责 execution**：人定义目标、验收标准和约束，系统负责协调执行。
- **仓库是系统事实来源**：manifest、文档、skills、QA 资产、policy 应该都在仓库里可发现、可版本化。
- **Guardrail 比一次性 prompt 更重要**：workflow、不变量、trigger、恢复路径，比临时 prompt 更能复利。
- **任何基于 shell 的 agent 都应可迁移**：控制面不能绑死在某一个模型或供应商上。
- **长时软件交付闭环必须可观测**：持久状态、日志、事件和 trace 不是附属品，而是核心能力。

## 这不是什么

- 不是某个单一模型的聊天包装器
- 不只是一个 workflow DSL
- 也不是面向所有企业场景的通用 CI/CD 替代品

它的目标更聚焦，也更有倾向性：成为 agent-native engineering system 的控制面。

## 产品方向

- 把零散的 agent 用法沉淀成可复用的 workflow 资产
- 让团队把 QA、修复、评审、治理、迁移、自举等流程制度化
- 在快速变化的模型与 coding shell 之上，提供稳定的 harness 层

## 继续阅读

- [01 - 快速开始](01-quickstart.md)
- [架构文档](https://github.com/c9r-io/orchestrator/blob/main/docs/architecture.md)
- [How OpenAI uses Codex](https://cdn.openai.com/pdf/6a2631dc-783e-479b-b1a4-af0cfbd38630/how-openai-uses-codex.pdf)
