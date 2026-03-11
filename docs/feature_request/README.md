# Feature Requests

本目录收录 `orchestrator` 的正式功能需求文档，来源于 2026-03-09 深度项目评估报告中优先级最高的改进建议。

## 当前条目

| ID | 标题 | 优先级 | 状态 |
|----|------|--------|------|
| FR-002 | Daemon 控制面认证、鉴权与传输安全 | P0 | Proposed |
| FR-005 | Daemon 生命周期治理与运行态指标补完 | P1 | Proposed |
| FR-010 | 控制面安全基线收紧与强制 mTLS | P0 | Proposed |
| FR-011 | validate/scheduler/runner 职责拆分与验证逻辑去重 | P1 | Proposed |
| FR-012 | SecretStore 密钥轮换、吊销与审计链 | P0 | Proposed |
| FR-013 | gRPC 控制面速率限制与 DoS 防护 | P0 | Proposed |
| FR-014 | 关键路径 `expect()` 清退与错误语义收敛 | P1 | Proposed |
| FR-015 | 高频 `clone()` 优化与共享所有权治理 | P2 | Proposed |
| FR-016 | 异步上下文锁模型收敛到 `tokio::sync::RwLock` | P1 | Proposed |

## 说明

- `P0`: 对安全性、控制面暴露面或系统可信边界有直接影响
- `P1`: 对系统一致性、平台成熟度、生产可用性有显著影响
- `Proposed`: 已形成正式需求，尚未进入实现阶段
- `In Progress`: 已有部分阶段落地，剩余阶段仍在治理中
- `Implemented`: 需求已完成并进入维护阶段
- 已闭环并删除的 FR，应由对应 `docs/design_doc/**` 与 `docs/qa/**` 继续承载设计和验证信息
- FR-010 是 FR-002 的强化续篇，聚焦默认关闭 `insecure-bind` 与强制 mTLS 握手
- FR-011 聚焦内核复杂度治理，不直接引入用户可见新能力
- FR-012 聚焦 SecretStore 的正式密钥生命周期治理，覆盖轮换、吊销与审计
- FR-013 聚焦 gRPC 控制面资源保护，优先降低恶意或异常客户端造成的拒绝服务风险
- FR-014 聚焦关键路径错误处理收敛，减少因 `expect()` 触发的非预期终止
- FR-015 聚焦字符串与配置对象的重复分配问题，优先治理高频热路径
- FR-016 聚焦异步上下文中的锁语义一致性，降低阻塞式锁带来的调度风险
- FR-009 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/25-database-persistence-bootstrap-repositories.md`、`docs/design_doc/orchestrator/26-database-migration-kernel-and-repository-governance.md`、`docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md` 与 `docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md` 承载
- FR-008 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/13-unified-step-execution-model.md`、`docs/guide/**` 与 `docs/qa/orchestrator/61-chain-steps-execution.md` 承载
- FR-007 已闭环删除；其收口结果由 `docs/architecture.md`、`docs/guide/**`、`skills/orchestrator-guide/**` 与 `docs/qa/**` 持续承载
- FR-006 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md` 与 `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md` 承载
