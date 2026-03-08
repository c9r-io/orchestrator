# FR-003 - Self-Referential Safety 约束语义收敛

**Module**: orchestrator  
**Status**: Proposed  
**Priority**: P1  
**Created**: 2026-03-09  
**Last Updated**: 2026-03-09  
**Source**: 深度项目评估报告最高优先级改进建议 #3

## Background

项目文档将 self-referential workflow 描述为高安全等级执行模式，核心承诺包括：

- `safety.auto_rollback` 必须为 `true`
- `safety.checkpoint_strategy` 不能为 `none`
- `safety.binary_snapshot` 应启用
- 缺少必要保护时，orchestrator 应拒绝启动

但当前实现中，硬性拒绝与 warning 的边界并不完全符合文档描述，且标准 workflow 与 `self_referential_probe` profile 的语义存在混杂。

## Problem Statement

当前 self-referential 安全模型存在一致性问题：

- 文档承诺比代码中的强制约束更严格
- `checkpoint_strategy` 的检查语义与 `workspace_is_self_referential` 的关系不够清晰
- `auto_rollback`、`self_test`、`binary_snapshot` 的强制级别没有统一定义
- 用户可能以为平台已提供强保护，但实际上只收到 warning

这会削弱平台可信度，并提高自举流程的误用风险。

## Goals

- 统一文档、校验器、运行时三者的 self-referential 安全语义
- 明确区分 hard error、soft warning、recommended 三类约束
- 让用户能够从错误信息中明确知道缺什么、为什么被拒绝
- 为后续 self-bootstrap 演进建立稳定契约

## Non-goals

- 第一阶段重新设计整个 self-bootstrap workflow
- 第一阶段替换现有 rollback / snapshot 机制

## Scope

- In scope:
  - `validate_self_referential_safety()` 规则重构
  - `self_referential_probe` profile 语义收敛
  - 文档更新
  - preflight / manifest validate 输出改进
- Out of scope:
  - 新的 checkpoint backend
  - 新的 binary verification 机制

## Proposed Design

### 1. 约束等级明确化

定义三类约束：

- `required`: 缺失即拒绝启动
- `strongly_recommended`: 给 warning，但允许继续
- `informational`: 仅提示

建议收敛为：

- `required`
  - `workspace.self_referential == true`
  - `safety.checkpoint_strategy != none`
  - `safety.auto_rollback == true`
  - workflow 存在 `self_test`
- `strongly_recommended`
  - `safety.binary_snapshot == true`
- `informational`
  - self-restart 验证链路增强项

### 2. 语义边界清理

- 仅对 `workspace.self_referential == true` 的 workflow 应用上述强约束
- `self_referential_probe` 作为更严格 profile，应附加 probe 专属规则，而不是绕开标准规则

### 3. 错误输出标准化

`manifest validate`、`orchestrator check`、任务启动失败信息应统一输出：

- 违反的规则 ID
- 当前值
- 期望值
- 风险说明
- 修复建议

## Alternatives And Tradeoffs

- **维持当前 warning-only 模式**: 兼容性最好，但安全契约持续模糊
- **一次性全面强制**: 安全性高，但可能导致现有 workflow 大量失效
- **分阶段收紧**: 先统一规则与提示，再在下个版本提升为 hard error。更稳妥

## Risks And Mitigations

- **破坏现有自举配置**: 提供迁移说明和 preflight 自动诊断
- **历史文档不一致**: 将 guide、architecture、workflow 示例一次性同步

## Observability

- 新增 `self_referential_policy_checked`
- 对每个失败规则记录 `rule_id`, `severity`, `actual`, `expected`
- `task_failed` 中包含策略拒绝分类

## Operations / Release

- 发布说明明确 self-referential 规则变更
- 示例 workflow 全量升级到新契约
- `orchestrator check` 增加 self-bootstrap 专项检查分组

## Test Plan

- Unit tests:
  - 各规则 hard error / warning 分支
  - probe profile 与 standard profile 的边界
- Integration tests:
  - self-referential workspace 缺少 `auto_rollback` 时被拒绝
  - 缺少 `binary_snapshot` 时仅 warning
- Docs QA:
  - guide / architecture / workflow 示例一致性检查

## Acceptance Criteria

- self-referential 相关文档与代码约束一致
- `required` 级约束违反时任务启动被拒绝
- 错误消息明确包含规则和修复建议
- 示例 workflow 与 preflight 输出均反映最新契约
