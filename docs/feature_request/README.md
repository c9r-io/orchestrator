# Feature Requests

本目录收录 `orchestrator` 的正式功能需求文档，来源于 2026-03-09 深度项目评估报告中优先级最高的改进建议。

## 当前条目

| ID | 标题 | 优先级 | 状态 |
|----|------|--------|------|
| FR-002 | Daemon 控制面认证、鉴权与传输安全 | P0 | Closed |
| FR-005 | Daemon 生命周期治理与运行态指标补完 | P1 | Closed |
| FR-011 | validate/scheduler/runner 职责拆分与验证逻辑去重 | P1 | Closed |
| FR-043 | loop_guard 收敛条件表达式 | P1 | Closed |
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
| FR-040 | QA Agent 子进程绕过 Daemon PID Guard 杀死 Daemon | P1 | Closed |
| FR-041 | Self-Restart 后 Socket 连接断裂导致后续步骤不可达 | P1 | Closed |
| FR-042 | follow_task_logs 流式回调重构 — gRPC TaskFollow 空流修复 | P1 | Closed |
| FR-044 | Sandbox 写入拒绝检测与 writable_paths 完善 | P1 | Closed |
| FR-045 | QA Agent 长生命周期命令防护 | P1 | Closed |
| FR-046 | Agent 子进程 Daemon PID Guard 穿透防护 | P1 | Closed |
| FR-047 | Core Crate 拆分 Phase 1 — orchestrator-config 提取 | P2 | Closed |
| FR-048 | Core Crate 拆分 Phase 2 — orchestrator-scheduler 提取 | P2 | Closed |
| FR-049 | Prehook CEL 表达式接入 Pipeline Variables | P1 | Closed |
| FR-050 | CLI UDS 连接回退鲁棒性 | P2 | Closed |
| FR-051 | Workflow YAML 步骤定义未知字段警告 | P1 | Closed |
| FR-053 | Full-QA Workflow 大规模 Item 分发中断 — max_cycles_enforced 过早触发 | P0 | Closed |
| FR-054 | Item 进度增量更新 — finalize_items 延迟导致 Progress 长时间为零 | P1 | Closed |
| FR-055 | Parallel Spawn Stagger Delay — 并行 Agent 启动间隔延迟 | P1 | Closed |
| FR-056 | Agent Health Policy 可配置化 — Disease 策略按 Agent/Workspace 设定 | P1 | Closed |
| FR-057 | orchestratord 真正 Daemon 化 | P1 | Closed |
| FR-058 | QA 自引用测试覆盖率恢复 — 场景级安全分级治理 | P1 | Closed |
| FR-060 | 减少 QA 场景中的不安全操作 | P1 | Closed |
| FR-061 | Daemon 日志环境变量覆盖 | P2 | Closed |
| FR-062 | Agent Health 状态可观测性 | P2 | Closed |
| FR-063 | GUI 架构设计 — Tauri + gRPC 安全客户端 | P1 | Closed |
| FR-064 | GUI 用户界面设计 — 许愿池 + 进度观察 | P1 | Closed |
| FR-065 | Agent 间通信接口草案 — Mailbox + Session Control Plane | P1 | Closed |
| FR-066 | GUI 实时状态推送与许愿池数据隔离 | P0 | Closed |
| FR-067 | GUI CLI 功能对齐 — 补全缺失 RPC 覆盖 | P1 | Closed |
| FR-068 | GUI 连接韧性与系统通知 | P1 | Closed |
| FR-069 | GUI 体验打磨 — 主题切换 / 动画 / i18n / 响应式 / 构建分发 | P2 | Closed |
| FR-070 | evo_apply_winner 可观测性增强 — 候选选择与代码应用决策日志 | P1 | Closed |
| FR-071 | 开源合规基础设施 — LICENSE / CHANGELOG / CONTRIBUTING / v0.1.0 Release | P0 | Closed |
| FR-072 | 分发渠道扩展 — Docker 镜像与 Homebrew Tap | P1 | Closed |
| FR-073 | 文档站点与 Landing Page — 外部可发现性 | P1 | Closed |
| FR-076 | GUI 正式发布 — Tauri App 打包分发 | P3 | Deferred |
| FR-077 | Workflow 模板库 — 常见 SDLC 自动化场景预设 | P1 | Closed |
| FR-078 | Task Items 与 Event List CLI 命令 | P1 | Closed |
| FR-079 | 数据生命周期治理 — 日志清理、DB 瘦身与自动化回收 | P1 | Closed |
| FR-080 | Webhook Trigger 基础设施 — HTTP 事件入口与通用事件源扩展 | P0 | Closed |
| FR-081 | Per-Trigger Webhook 认证与 CEL Payload 过滤 | P1 | Closed |
| FR-082 | 集成 Manifest 包 — Slack / GitHub / Line 预制配置 | P2 | Closed |
| FR-083 | CRD 插件系统 — Webhook 拦截器与自动化生命周期 | P3 | Proposed |
| FR-084 | Agent 条件命令规则 + Session 复用 | P1 | Closed |
| FR-085 | Filesystem Trigger — 文件系统变更原生触发器 | P1 | Closed |

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
- FR-040 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/52-prehook-self-referential-safe-filter.md` 与 `docs/qa/orchestrator/95-prehook-self-referential-safe-filter.md` 承载
- FR-041 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/53-self-restart-socket-continuity.md` 与 `docs/qa/orchestrator/96-self-restart-socket-continuity.md` 承载
- FR-042 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/54-follow-task-logs-callback.md` 与 `docs/qa/orchestrator/97-follow-task-logs-callback.md` 承载
- FR-043 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/55-convergence-expression.md` 与 `docs/qa/orchestrator/98-convergence-expression.md` 承载
- FR-002 已闭环；其设计与验证信息现由 `docs/design_doc/orchestrator/22-control-plane-security.md` 与 `docs/qa/orchestrator/58-control-plane-security.md` 承载（mTLS、RBAC 授权、审计日志均已实现）
- FR-005 已闭环；其设计与验证信息现由 `docs/design_doc/orchestrator/24-daemon-lifecycle-runtime-metrics.md` 与 `docs/qa/orchestrator/60-daemon-lifecycle-runtime-metrics.md` 承载（生命周期状态机、运行时指标、优雅 drain 均已实现）
- FR-011 已闭环；代码已自然实现 validate/scheduler/runner 的职责分离（config_load/validate/、output_validation.rs、runner/sandbox.rs 各司其职），无需进一步重构
- FR-044 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/56-sandbox-denial-detection.md` 与 `docs/qa/orchestrator/56-sandbox-denial-anomaly-trace.md` 承载
- FR-045 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/57-long-lived-command-guard.md` 与 `docs/qa/orchestrator/99-long-lived-command-guard.md` 承载
- FR-046 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/58-agent-subprocess-daemon-pid-guard.md` 与 `docs/qa/orchestrator/100-agent-subprocess-daemon-pid-guard.md` 承载
- FR-047 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/59-core-crate-split-config.md` 与 `docs/qa/orchestrator/101-core-crate-split-config.md` 承载
- FR-048 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/60-core-crate-split-scheduler.md` 与 `docs/qa/orchestrator/102-core-crate-split-scheduler.md` 承载
- FR-049 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/61-prehook-pipeline-vars.md` 与 `docs/qa/orchestrator/103-prehook-pipeline-vars.md` 承载
- FR-050 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/62-cli-uds-fallback-robustness.md` 与 `docs/qa/orchestrator/104-cli-uds-fallback-robustness.md` 承载
- FR-051 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/63-workflow-yaml-unknown-field-warning.md` 与 `docs/qa/orchestrator/105-workflow-yaml-unknown-field-warning.md` 承载
- FR-052 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/64-inflight-wait-heartbeat-aware-timeout.md` 与 `docs/qa/orchestrator/106-inflight-wait-heartbeat-aware-timeout.md` 承载
- FR-053 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/65-parallel-dispatch-completeness-guard.md` 与 `docs/qa/orchestrator/107-parallel-dispatch-completeness-guard.md` 承载
- FR-054 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/66-incremental-item-progress.md` 与 `docs/qa/orchestrator/108-incremental-item-progress.md` 承载
- FR-055 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/67-parallel-spawn-stagger-delay.md` 与 `docs/qa/orchestrator/109-parallel-spawn-stagger-delay.md` 承载
- FR-056 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/68-agent-health-policy-configuration.md` 与 `docs/qa/orchestrator/110-agent-health-policy-configuration.md` 承载
- FR-057 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/69-daemon-proper-daemonize.md` 与 `docs/qa/orchestrator/111-daemon-proper-daemonize.md` 承载
- FR-058 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/70-scenario-level-self-referential-safety.md` 与 `docs/qa/orchestrator/112-scenario-level-self-referential-safety.md` 承载
- FR-061 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/71-logging-env-var-override.md` 与 `docs/qa/orchestrator/113-logging-env-var-override.md` 承载
- FR-062 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/72-agent-health-state-observability.md` 与 `docs/qa/orchestrator/114-agent-health-state-observability.md` 承载
- FR-065 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/75-agent-mailbox-session-communication.md` 与 `docs/qa/orchestrator/115-agent-mailbox-session-communication.md` 承载
- FR-063 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/73-gui-architecture-tauri-grpc.md` 与 `docs/qa/orchestrator/116-gui-architecture-tauri-grpc.md` 承载
- FR-064 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/74-gui-uiux-wish-pool-progress.md` 与 `docs/qa/orchestrator/117-gui-uiux-wish-pool-progress.md` 承载
- FR-066 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/76-gui-realtime-wish-isolation.md` 与 `docs/qa/orchestrator/118-gui-realtime-wish-isolation.md` 承载
- FR-060 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/77-reduce-unsafe-qa-operations.md` 承载（13 次迭代将 unsafe 文档从 114 降至 33，+360 安全场景，23.1% unsafe 率达成 < 30% 目标）
- FR-067 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/77-gui-cli-rpc-parity.md` 与 `docs/qa/orchestrator/119-gui-cli-rpc-parity.md` 承载
- FR-068 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/78-gui-connection-resilience-notification.md` 与 `docs/qa/orchestrator/120-gui-connection-resilience-notification.md` 承载
- FR-069 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/79-gui-polish-theme-i18n-responsive.md` 与 `docs/qa/orchestrator/121-gui-polish-theme-i18n-responsive.md` 承载
- FR-070 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/80-evo-apply-winner-observability.md` 与 `docs/qa/orchestrator/122-evo-apply-winner-observability.md` 承载
- FR-071 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/81-open-source-compliance.md` 与 `docs/qa/orchestrator/123-open-source-compliance.md` 承载（LICENSE、CHANGELOG、CONTRIBUTING、GitHub 模板已就绪；v0.1.0 release 待 tag 推送）
- FR-072 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/82-homebrew-tap-distribution.md` 与 `docs/qa/orchestrator/124-homebrew-tap-distribution.md` 承载（Docker 分发因架构不兼容已排除——orchestratord 以子进程方式 spawn agent，需宿主机工具与凭证；已实现 Homebrew tap 与 cargo install 两条分发渠道）
- FR-073 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/83-documentation-site.md` 与 `docs/qa/orchestrator/125-documentation-site.md` 承载（VitePress 文档站 + Landing Page + "Why Orchestrator?" 对比页；README 精简至 74 行；Cloudflare Pages 自动部署）
- FR-078 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/84-task-items-event-list-cli.md` 与 `docs/qa/orchestrator/126-task-items-event-list-cli.md` 承载（新增 `task items` 和 `event list` CLI 命令，消除 showcase 中的 sqlite 直接查询）
- FR-079 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/85-data-lifecycle-governance.md` 与 `docs/qa/orchestrator/127-data-lifecycle-governance.md` 承载（日志 TTL 默认 30 天自动清理、`db vacuum` 命令、`db cleanup` 命令、`db status` 显示磁盘用量、可选 task 自动清理）
- FR-080 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/86-webhook-trigger-infrastructure.md` 与 `docs/qa/orchestrator/128-webhook-trigger-infrastructure.md` 承载（HTTP webhook 端点、`source: webhook` 触发器、HMAC 签名验证、`trigger fire --payload`、axum HTTP 服务与 gRPC 并行运行）
- FR-081 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/87-per-trigger-webhook-auth.md` 与 `docs/qa/orchestrator/129-per-trigger-webhook-auth-cel-filter.md` 承载（Per-trigger SecretStore 签名验证 + 多密钥轮替、自定义签名 header、CEL payload 过滤、全局 secret fallback）
- FR-082 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/88-integration-manifest-packages.md` 与 `docs/qa/orchestrator/130-integration-manifest-packages.md` 承载（`c9r-io/orchestrator-integrations` 独立仓库，Slack/GitHub/LINE 集成包，密钥轮替 showcase）
- FR-084 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/89-agent-command-rules-step-vars.md` 与 `docs/qa/orchestrator/100-agent-command-rules-step-vars.md` 承载（Agent `command_rules` CEL 条件命令选择、Step `step_vars` 临时变量覆盖、`command_rule_index` 审计列；Session 复用为纯 workflow 编排示例）
- FR-077 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/90-workflow-template-library.md` 与 `docs/qa/orchestrator/131-workflow-template-library.md` 承载（5 个渐进复杂度模板：hello-world / qa-loop / plan-execute / scheduled-scan / fr-watch，echo agent 零成本运行，文档站 Templates 分组）
- FR-085 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/91-filesystem-trigger.md` 与 `docs/qa/orchestrator/132-filesystem-trigger.md` 承载（`source: filesystem` 原生触发器，`notify` crate 跨平台文件监控，按需启停 watcher，路径白名单 + 事件类型 + 防抖 + CEL 四层过滤，macOS symlink 兼容）
- FR-086 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/92-daemon-config-hot-reload.md` 与 `docs/qa/orchestrator/133-daemon-config-hot-reload.md` 承载（ArcSwap 原子快照机制实现无重启配置热加载，`persist_config_and_reload()` 在 apply 响应前同步更新 `config_runtime`；QA 128 S2/S3 限制已移除）
