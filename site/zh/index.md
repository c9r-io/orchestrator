---
layout: home

hero:
  name: Agent Orchestrator
  text: AI 原生 SDLC 自动化
  tagline: 声明式工作流与 Agent 编排，覆盖从需求到部署的完整软件开发生命周期。
  actions:
    - theme: brand
      text: 快速开始
      link: /zh/guide/quickstart
    - theme: alt
      text: 为什么选择 Orchestrator?
      link: /zh/why
    - theme: alt
      text: GitHub
      link: https://github.com/c9r-io/orchestrator

features:
  - icon: "\U0001F4CB"
    title: 声明式工作流
    details: 通过 YAML 定义多步骤工作流，支持循环控制、守卫步骤、DAG 执行和动态步骤池。
  - icon: "\U0001F916"
    title: Agent 编排
    details: 基于能力的 Agent 选择，配合健康评分、轮换策略和指标驱动的负载均衡。
  - icon: "\u26A1"
    title: CEL 前置钩子
    details: 通过 CEL 表达式实现动态控制流 — 运行时决策：Run、Skip、Branch、DynamicAdd、Transform。
  - icon: "\U0001F512"
    title: 内置安全
    details: mTLS 认证、RBAC 授权、沙箱执行（macOS Seatbelt / Linux 命名空间）、输出脱敏。
  - icon: "\U0001F680"
    title: 客户端/服务端架构
    details: 轻量级 CLI 通过 gRPC/UDS 与守护进程通信。内嵌 Worker、SQLite 持久化、配置热加载。
  - icon: "\U0001F310"
    title: 多平台支持
    details: "通过 curl | sh、brew install 或 cargo install 安装。支持 macOS (ARM64) 和 Linux (x86_64, ARM64)。"
---

## 快速开始

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
