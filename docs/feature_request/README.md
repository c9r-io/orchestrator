# Feature Requests

本目录收录 `orchestrator` 的正式功能需求文档，来源于 2026-03-09 深度项目评估报告中优先级最高的改进建议。

## 当前条目

| ID | 标题 | 优先级 | 状态 |
|----|------|--------|------|
| FR-002 | Daemon 控制面认证、鉴权与传输安全 | P0 | Proposed |
| FR-005 | Daemon 生命周期治理与运行态指标补完 | P1 | Proposed |

## 说明

- `P0`: 对安全性、控制面暴露面或系统可信边界有直接影响
- `P1`: 对系统一致性、平台成熟度、生产可用性有显著影响
- `Proposed`: 已形成正式需求，尚未进入实现阶段
- `Implemented`: 需求已完成并进入维护阶段
- 已闭环并删除的 FR，应由对应 `docs/design_doc/**` 与 `docs/qa/**` 继续承载设计和验证信息
- FR-006 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md` 与 `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md` 承载
