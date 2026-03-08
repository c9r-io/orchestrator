# Agent Orchestrator 用户指南

本指南帮助你理解和使用 Agent Orchestrator —— 一个通过智能 Agent 编排实现 AI 原生开发生命周期自动化的 CLI 工具。

## 适用人群

- **新用户**：希望快速上手
- **工作流作者**：设计自定义 SDLC 流水线
- **高级用户**：探索 CRD、持久化存储和自引导

## 指南结构

| 章节 | 主题 | 难度 |
|------|------|------|
| [01 - 快速开始](01-quickstart.md) | 5 分钟跑通你的第一个工作流 | 入门 |
| [02 - 资源模型](02-resource-model.md) | Workspace、Agent、Workflow、StepTemplate | 入门 |
| [03 - 工作流配置](03-workflow-configuration.md) | Step、作用域、循环、终结规则 | 中级 |
| [04 - CEL 预钩子](04-cel-prehooks.md) | 基于 CEL 表达式的动态步骤门控 | 中级 |
| [05 - 高级特性](05-advanced-features.md) | CRD、持久化存储、任务派生、不变量约束 | 高级 |
| [06 - 自引导](06-self-bootstrap.md) | 自修改工作流与生存机制 | 高级 |
| [07 - CLI 参考](07-cli-reference.md) | 命令速查表 | 参考 |

## 补充资源

- **架构文档**：`docs/architecture.md` —— 系统设计与内部原理
- **QA 测试文档**：`docs/qa/` —— 场景化验证（51+ 份文档）
- **设计文档**：`docs/design_doc/` —— 详细特性设计
- **Fixture 清单**：`fixtures/manifests/bundles/` —— 真实 YAML 示例

## 入口

### 单体模式（传统）

```bash
orchestrator <command>
```

### 客户端/服务端模式（推荐）

编排器支持 C/S 架构，守护进程长期运行持有所有状态，CLI 作为轻量 gRPC 客户端：

```bash
# 启动守护进程（内嵌后台工作器自动消费排队任务）
./target/release/orchestratord --foreground --workers 2

# 使用 CLI 客户端（通过 Unix 套接字连接守护进程）
./target/release/orchestrator <command>

# 或通过 CLI 管理守护进程生命周期
./target/release/orchestrator daemon start
./target/release/orchestrator daemon status
./target/release/orchestrator daemon stop
```

参见 [07 - CLI 参考](07-cli-reference.md) 获取完整 C/S 命令列表。

## Claude Code Skill（面向 AI Agent）

本指南的精简版以 Claude Code Skill 形式提供，安装后 Claude 将自动掌握编排器的配置与命令知识：

```bash
/install-skill skills/orchestrator-guide.skill
```

安装后，Claude 在处理编排器清单、工作流、CEL 表达式或 CLI 命令时会自动参考该指南。

## English Version / 英文版

See [../README.md](../README.md) for the English version of this guide.
