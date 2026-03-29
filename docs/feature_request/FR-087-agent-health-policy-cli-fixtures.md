# FR-087: Agent Health Policy CLI 测试夹具 — 自定义策略 QA 可验证性

| 字段 | 值 |
|------|---|
| **优先级** | P2 |
| **状态** | Proposed |
| **关联** | FR-056 (Closed), DD-068, QA-110b S2 |

## 背景

FR-056 实现了 Agent Health Policy 可配置化（disease 策略按 Agent/Workspace 设定），核心逻辑已通过单元测试验证。但 QA-110b S2 要求通过 `orchestrator check` 验证自定义 health policy 的 CLI 展示效果，而当前 CLI 无法注册带有自定义 health policy 的 Agent 测试夹具。

## 问题描述

- `orchestrator check` 能正确显示默认 health policy（已验证）
- 自定义策略的 Agent（如 `custom-agent`, `nodisease-agent`）无法通过 CLI 创建
- S2 中 `disease DISABLED` 展示场景无法 end-to-end 验证

## 验收标准

1. 提供 CLI 机制或 fixture manifest 支持注册带有自定义 `health_policy` 的 Agent
2. `orchestrator check` 能展示自定义阈值和 `disease DISABLED` 状态
3. QA-110b S2 可完整通过 CLI 验证

## 来源

- QA ticket: `docs/ticket/qa-110b-s2-custom-health-policy-untested.md`
- 复现步骤：注册多个 Agent（部分包含自定义 health_policy），运行 `orchestrator check`，验证展示
