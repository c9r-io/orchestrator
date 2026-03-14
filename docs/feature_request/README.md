# Feature Requests

本目录收录 `orchestrator` 的正式功能需求文档，来源于 2026-03-09 深度项目评估报告中优先级最高的改进建议。

## 当前条目

| ID | 标题 | 优先级 | 状态 |
|----|------|--------|------|
| FR-002 | Daemon 控制面认证、鉴权与传输安全 | P0 | Proposed |
| FR-005 | Daemon 生命周期治理与运行态指标补完 | P1 | Proposed |
| FR-011 | validate/scheduler/runner 职责拆分与验证逻辑去重 | P1 | Proposed |
| FR-017 | Agent Drain 与 Enabled 开关 | P1 | Implemented |
| FR-018 | 用户指南编译验证对齐 | P1 | Implemented |
| FR-019 | 修复 libc 类型编译错误 | P0 | Implemented |
| FR-020 | 自动化 protoc 依赖安装 | P0 | Implemented |
| FR-021 | 审计并减少 expect() 调用 | P1 | Implemented |
| FR-023 | 增加集成测试覆盖 | P2 | Closed |
| FR-024 | 审计 unsafe 块 | P2 | Closed |
| FR-026 | 事件表归档与 TTL 清理策略 | P1 | Closed |
| FR-027 | Worker 轮询优化 — Notify 唤醒机制 | P1 | Implemented |
| FR-030 | Self-Evolution 数据库 Schema 对齐验证 | P1 | Closed |
| FR-031 | generate_items 对 LLM 非标准 JSON 输出的容错解析 | P1 | Closed |
| FR-032 | Daemon 进程崩溃韧性与 Worker 存活保障 | P1 | Closed |
| FR-033 | Daemon 重启后孤立 Running Items 自动恢复 | P1 | Closed |
| FR-034 | QA Testing 自引用安全防护 | P1 | Closed |
| FR-035 | 退化循环检测与熔断机制 | P1 | Closed |
| FR-036 | Plan Output Context Overflow 缓解 | P1 | Closed |
| FR-037 | Dynamic Items 触发的循环溢出 — max_cycles 约束失效 | P1 | Closed |
| FR-038 | Daemon 重启时在途步骤竞态 — task_completed 提前发出与动态 Item 状态丢失 | P1 | Closed |
| FR-039 | Trigger 资源 — Cron 与事件驱动的任务自动创建 | P1 | Closed |
| FR-040 | QA Agent 子进程绕过 Daemon PID Guard 杀死 Daemon | P1 | Proposed |
| FR-041 | Self-Restart 后 Socket 连接断裂导致后续步骤不可达 | P1 | Proposed |

## 说明

- `P0`: 对安全性、控制面暴露面或系统可信边界有直接影响
- `P1`: 对系统一致性、平台成熟度、生产可用性有显著影响
- `Proposed`: 已形成正式需求，尚未进入实现阶段
- `In Progress`: 已有部分阶段落地，剩余阶段仍在治理中
- `Implemented`: 需求已完成并进入维护阶段
- 已闭环并删除的 FR，应由对应 `docs/design_doc/**` 与 `docs/qa/**` 继续承载设计和验证信息
- FR-011 聚焦内核复杂度治理，不直接引入用户可见新能力
- FR-012 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/27-secretstore-key-lifecycle.md` 与 `docs/qa/orchestrator/64-secretstore-key-lifecycle.md` 承载
- FR-013 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/27-grpc-control-plane-protection.md`、`docs/qa/orchestrator/65-grpc-control-plane-protection.md` 与 `scripts/qa/test-fr013-control-plane-protection.sh` 承载
- FR-014 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/28-error-semantics-governance.md` 与 `docs/qa/orchestrator/66-error-semantics-governance.md` 承载
- FR-015 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/29-clone-reduction-and-shared-ownership.md`、`docs/qa/orchestrator/67-clone-reduction-and-shared-ownership.md` 与 `docs/qa/orchestrator/68-clone-reduction-follow-up.md` 承载
- FR-016 已闭环删除；其设计、验证与门禁信息现由 `docs/design_doc/orchestrator/30-async-lock-model-alignment.md`、`docs/qa/orchestrator/69-async-lock-model-alignment.md` 与 `scripts/check-async-lock-governance.sh` 持续承载
- FR-017 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/agent-drain-enabled.md` 与 `docs/qa/orchestrator/agent-drain-enabled.md` 承载
- FR-018 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/guide-alignment.md` 与 `docs/qa/orchestrator/guide-alignment.md` 承载，`guide-alignment` skill 提供持续治理能力
- FR-019 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/31-libc-cross-platform-compilation.md` 与 `docs/qa/orchestrator/70-libc-cross-platform-compilation.md` 承载
- FR-009 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/25-database-persistence-bootstrap-repositories.md`、`docs/design_doc/orchestrator/26-database-migration-kernel-and-repository-governance.md`、`docs/qa/orchestrator/62-database-persistence-bootstrap-repositories.md` 与 `docs/qa/orchestrator/63-database-migration-kernel-and-repository-governance.md` 承载
- FR-008 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/13-unified-step-execution-model.md`、`docs/guide/**` 与 `docs/qa/orchestrator/61-chain-steps-execution.md` 承载
- FR-007 已闭环删除；其收口结果由 `docs/architecture.md`、`docs/guide/**`、`skills/orchestrator-guide/**` 与 `docs/qa/**` 持续承载
- FR-006 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/21-sandbox-resource-network-enforcement.md` 与 `docs/qa/orchestrator/56-sandbox-resource-network-enforcement.md` 承载
- FR-010 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/22-control-plane-security.md` 与 `docs/qa/orchestrator/58-control-plane-security.md` 承载
- FR-020 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/32-automate-protoc-dependency.md` 与 `docs/qa/orchestrator/71-automate-protoc-dependency.md` 承载
- FR-021 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/33-audit-reduce-expect-calls.md` 与 `docs/qa/orchestrator/72-audit-reduce-expect-calls.md` 承载
- FR-022 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/36-public-api-doc-comments.md` 与 `docs/qa/orchestrator/75-public-api-doc-comments.md` 承载
- FR-023 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/34-integration-test-coverage.md` 与 `docs/qa/orchestrator/73-integration-test-coverage.md` 承载
- FR-024 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/35-audit-unsafe-blocks.md` 与 `docs/qa/orchestrator/74-audit-unsafe-blocks.md` 承载
- FR-025 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/37-config-load-module-split.md` 与 `docs/qa/orchestrator/76-config-load-module-split.md` 承载
- FR-027 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/39-worker-notify-wakeup.md` 与 `docs/qa/orchestrator/78-worker-notify-wakeup.md` 承载
- FR-028 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/40-benchmark-score-capture.md` 与 `docs/qa/orchestrator/79-benchmark-score-capture.md` 承载
- FR-026 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/38-event-table-ttl-archival.md` 与 `docs/qa/orchestrator/77-event-table-ttl-archival.md` 承载
- FR-029 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/41-item-scoped-git-worktree-isolation.md` 与 `docs/qa/orchestrator/80-item-scoped-git-worktree-isolation.md` 承载
- FR-030 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/42-self-evolution-db-schema-alignment.md` 与 `docs/qa/orchestrator/81-self-evolution-db-schema-alignment.md` 承载
- FR-034 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/44-self-referential-daemon-pid-guard.md` 与 `docs/qa/orchestrator/87-self-referential-daemon-pid-guard.md` 承载
- FR-035 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/12-degenerate-cycle-loop-guard.md` 与 `docs/qa/orchestrator/23-degenerate-cycle-loop-guard.md` 承载
- FR-036 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/45-plan-output-context-overflow-mitigation.md` 与 `docs/qa/orchestrator/89-plan-output-context-overflow-mitigation.md` 承载
- FR-031 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/46-unquoted-json-extraction.md` 与 `docs/qa/orchestrator/90-unquoted-json-extraction.md` 承载
- FR-032 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/47-daemon-crash-resilience.md` 与 `docs/qa/orchestrator/91-daemon-crash-resilience.md` 承载
- FR-033 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/48-orphaned-running-items-recovery.md` 与 `docs/qa/orchestrator/86-orphaned-running-items-recovery.md` 承载
- FR-037 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/49-dynamic-items-cycle-overflow.md` 与 `docs/qa/orchestrator/92-dynamic-items-cycle-overflow.md` 承载
- FR-038 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/50-inflight-step-completion-race.md` 与 `docs/qa/orchestrator/93-inflight-step-completion-race.md` 承载
- FR-039 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/51-trigger-resource-cron-event-driven-task-creation.md` 与 `docs/qa/orchestrator/94-trigger-resource-cron-event-driven.md` 承载
