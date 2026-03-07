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

所有命令使用包装脚本：

```bash
./scripts/orchestrator.sh <command>
```

或直接使用二进制文件：

```bash
./core/target/release/agent-orchestrator <command>
```
