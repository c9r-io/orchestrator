# FR-075: VS Code 扩展 — Manifest Schema Validation & Autocomplete

## 优先级: P2

## 状态: Proposed

## 背景

用户编写 orchestrator YAML manifest 时缺乏编辑器辅助。手动查阅文档编写 `kind: Workflow` 等资源定义容易出错。VS Code 扩展可提供 schema validation 和 autocomplete，显著降低学习和使用成本。

## 需求

### 1. JSON Schema 生成
- 从 Rust 类型定义（`WorkspaceSpec`, `WorkflowSpec`, `AgentSpec` 等）自动生成 JSON Schema
- 使用 `schemars` crate 的 `#[derive(JsonSchema)]` 或手动维护 schema 文件
- Schema 覆盖所有 v2 资源类型:
  - Workspace, Agent, Workflow, StepTemplate, ExecutionProfile
  - SecretStore, EnvStore, WorkflowStore, Trigger, RuntimePolicy
  - CustomResourceDefinition

### 2. VS Code 扩展
- 基于 YAML Language Server + JSON Schema 实现
- 功能:
  - 自动识别 orchestrator manifest（通过 `apiVersion: orchestrator.dev/v2`）
  - 字段级 autocomplete
  - 实时 validation 错误提示
  - Hover 文档（字段说明）
- 发布到 VS Code Marketplace

### 3. 内置 CLI Manifest 验证增强
- `orchestrator manifest validate -f` 已存在
- 增强: 输出 JSON Schema validation 错误（行号 + 字段路径）

## 验收标准

- [ ] JSON Schema 文件覆盖所有资源类型
- [ ] VS Code 安装扩展后打开 `.yaml` 文件，输入 `kind:` 出现补全列表
- [ ] 错误的字段名/类型实时标红
- [ ] 扩展发布到 VS Code Marketplace 可搜索
