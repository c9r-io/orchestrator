# Feature Requests

本目录收录 `orchestrator` 的正式功能需求文档。Agent Process Console v1 已完成闭环；产品结构与发布边界分别由[信息架构](../design_doc/orchestrator/110-process-console-information-architecture.md)和[发布验收设计](../design_doc/orchestrator/116-process-console-release-acceptance.md)持续承载。Slack reaction 驱动的 Skill 任务自动化已发布（FR-107 至 FR-113 全部闭环删除），其设计、验证与用户指南现由 `docs/design_doc/orchestrator/118-`～`124-`、`docs/qa/orchestrator/155-`～`161-` 与 [用户指南](../guide/slack-reaction-skill-automation.md)承载；Managed Slack Connection、官方 App OAuth 与[每 workspace 独立 App provisioning](../design_doc/orchestrator/126-dedicated-slack-app-auto-provisioning.md)均已闭环。Agent 执行后端契约的供应商中立化已由 [Agent Driver 设计](../design_doc/orchestrator/127-agent-driver-abstraction.md)闭环；生产工作流的协调坍缩、冻结棘轮与退役标准由[协调 Strangler 收尾设计](../design_doc/orchestrator/136-coordination-strangler-completion.md)持续承载。FR-160 已闭环删除；QA harness 的共享守护进程拆解（`scripts/lib/gate_daemon.sh` 与 check 16）现由 [DD-174](../design_doc/orchestrator/174-qa-harness-daemon-teardown.md) 与 [QA 211](../qa/orchestrator/211-qa-harness-daemon-teardown.md) 承载。

## 当前条目


<!-- BEGIN GENERATED FR REGISTRY -->
> 由 `scripts/lib/fr_registry.rb` 从完整 `HEAD` 祖先历史生成：156 个历史编号 / 161 条历史路径，另有 13 条无 FR 文件历史的审阅例外；5 个编号存在多路径碰撞。浅克隆拒绝生成。

| ID | 标题 | 优先级 | 状态 | 来源 / 碰撞 |
|----|------|--------|------|-------------|
| FR-001 | Step 执行隔离与按需 Sandbox | P0 | Closed | git history |
| FR-002 | Daemon 控制面认证、鉴权与传输安全 | P0 | Closed | git history |
| FR-003 | Self-Referential Safety 约束语义收敛 | P1 | Closed | git history |
| FR-004 | DAG / 动态编排主路径化与可观测化 | P1 | Closed | git history |
| FR-005 | Daemon 生命周期治理与运行态指标补完 | P1 | Closed | git history |
| FR-006 | 彻底消除全局设定与实现纯粹的 Project-Only 架构 | — | Closed | collision (2): FR-006-project-scoped-isolation.md; FR-006-sandbox-network-allowlist-backend.md |
| FR-009 | 数据库迁移治理与持久化边界收敛 | — | Closed | git history |
| FR-010 | 控制面安全基线收紧与强制 mTLS | P0 | Closed | git history |
| FR-011 | validate/scheduler/runner 职责拆分与验证逻辑去重 | P1 | Closed | git history |
| FR-012 | SecretStore 密钥轮换、吊销与审计链 | P0 | Closed | git history |
| FR-013 | gRPC 控制面速率限制与 DoS 防护 | P0 | Closed | git history |
| FR-014 | 关键路径 `expect()` 清退与错误语义收敛 | P1 | Closed | git history |
| FR-015 | 高频 `clone()` 优化与共享所有权治理 | P2 | Closed | git history |
| FR-016 | 异步上下文锁模型收敛到 `tokio::sync::RwLock` | P1 | Closed | git history |
| FR-017 | Agent Drain 与 Enabled 开关 | P1 | Closed | git history |
| FR-018 | 用户指南编译验证对齐 | P1 | Closed | git history |
| FR-019 | 修复 libc 类型编译错误 | P0 | Closed | git history |
| FR-020 | 自动化 protoc 依赖安装 | P0 | Closed | git history |
| FR-021 | 审计并减少 expect() 调用 | P1 | Closed | git history |
| FR-022 | 补充公共 API 文档注释 | P1 | Closed | git history |
| FR-023 | 增加集成测试覆盖 | P2 | Closed | git history |
| FR-024 | 审计 unsafe 块 | P2 | Closed | git history |
| FR-026 | 事件表归档与 TTL 清理策略 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-027 | Worker 轮询优化 — Notify 唤醒机制 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-029 | Item-Scoped Git 工作目录隔离 | P0 | Closed | git history |
| FR-030 | Self-Evolution 数据库 Schema 对齐验证 | P1 | Closed | git history |
| FR-031 | generate_items 对 LLM 非标准 JSON 输出的容错解析 | — | Closed | git history |
| FR-032 | Daemon 进程崩溃韧性与 Worker 存活保障 | — | Closed | git history |
| FR-033 | Daemon 重启后孤立 Running Items 自动恢复 | — | Closed | git history |
| FR-034 | QA Testing 自引用安全防护 | — | Closed | git history |
| FR-035 | 退化循环检测与熔断机制 | — | Closed | git history |
| FR-036 | Plan Output 上下文溢出缓解 | — | Closed | git history |
| FR-037 | Dynamic Items 触发的循环溢出 — max_cycles 约束失效 | P1 | Closed | git history |
| FR-038 | Daemon 重启时在途步骤竞态 — task_completed 提前发出与动态 Item 状态丢失 | — | Closed | git history |
| FR-039 | Trigger 资源 — Cron 与事件驱动的任务自动创建 | — | Closed | git history |
| FR-040 | QA Agent 子进程绕过 Daemon PID Guard 杀死 Daemon | P1 | Closed | git history |
| FR-041 | Self-Restart 后 Socket 连接断裂导致后续步骤不可达 | P1 | Closed | git history |
| FR-042 | follow_task_logs 流式回调重构 — gRPC TaskFollow 端点从空流变为真实日志流 | P1 | Closed | git history |
| FR-043 | loop_guard 收敛条件表达式 | P1 | Closed | git history |
| FR-044 | Sandbox 写入拒绝检测与 writable_paths 完善 | P1 | Closed | git history |
| FR-045 | QA Agent 长生命周期命令防护 | P1 | Closed | git history |
| FR-046 | Agent 子进程 Daemon PID Guard 穿透防护 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-047 | Core Crate 拆分 Phase 1 — orchestrator-config 提取 | — | Closed | git history |
| FR-048 | Core Crate 拆分 Phase 2 — orchestrator-scheduler 提取 | — | Closed | git history |
| FR-049 | Prehook CEL 表达式接入 Pipeline Variables | P1 | Closed | git history |
| FR-050 | CLI UDS 连接回退鲁棒性 | P2 | Closed | git history |
| FR-051 | Workflow YAML 步骤定义未知字段警告 | — | Closed | git history |
| FR-052 | Inflight Wait Heartbeat-Aware Timeout | P1 | Closed | git history |
| FR-053 | Full-QA Workflow 大规模 Item 分发中断 — max_cycles_enforced 过早触发 | P0 | Closed | git history |
| FR-054 | Item 进度增量更新 — finalize_items 延迟导致 Progress 长时间为零 | P1 | Closed | git history |
| FR-055 | Parallel Spawn Stagger Delay | — | Closed | git history |
| FR-056 | Agent Health Policy 可配置化 | — | Closed | git history |
| FR-057 | orchestratord 真正 Daemon 化 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-058 | QA 自引用测试覆盖率恢复 — 场景级安全分级治理 | P1 | Closed | git history |
| FR-060 | 减少 QA 场景中的不安全操作 | — | Closed | git history |
| FR-061 | Daemon 日志环境变量覆盖 | P2 | Closed | git history |
| FR-062 | Agent Health 状态可观测性 | P2 | Closed | git history |
| FR-063 | GUI 架构设计 — Tauri + gRPC 安全客户端 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-064 | GUI 用户界面设计 — 许愿池 + 进度观察 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-065 | Agent 间通信接口草案 — Mailbox + Session Control Plane | — | Closed | git history |
| FR-066 | GUI 实时状态推送与许愿池数据隔离 | P0 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-067 | GUI CLI 功能对齐 — 补全缺失 RPC 覆盖 | — | Closed | git history |
| FR-068 | GUI 连接韧性与系统通知 | — | Closed | git history |
| FR-069 | GUI 体验打磨 — 主题切换 / 动画 / i18n / 响应式 / 构建分发 | — | Closed | git history |
| FR-070 | evo_apply_winner 可观测性增强 — 候选选择与代码应用决策日志 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-071 | 开源合规基础设施 | P0 | Closed | git history |
| FR-072 | 分发渠道扩展 — Docker 镜像与 Homebrew | P1 | Closed | git history |
| FR-073 | 文档站点与 Landing Page | P1 | Closed | git history |
| FR-074 | 可观测性导出 — Prometheus Metrics 端点 | P2 | Closed | git history |
| FR-075 | VS Code 扩展 — Manifest Schema Validation & Autocomplete | P2 | Closed | git history |
| FR-076 | GUI 正式发布 — Tauri App 打包分发 | P1 | Closed | git history |
| FR-077 | Linux Sandbox Filesystem Isolation Backend | P3 | Closed | collision (2): FR-077-linux-sandbox-filesystem-isolation.md; FR-077-workflow-template-library.md |
| FR-078 | Task Items 与 Event List CLI 命令 | P1 | Closed | git history |
| FR-079 | 数据生命周期治理 — 日志清理、DB 瘦身与自动化回收 | P1 | Closed | git history |
| FR-080 | Webhook Trigger 基础设施 — HTTP 事件入口与通用事件源扩展 | P0 | Closed | git history |
| FR-081 | Per-Trigger Webhook 认证与 CEL Payload 过滤 | P1 | Closed | git history |
| FR-082 | 集成 Manifest 包 — Slack / GitHub / Line 预制配置 | P2 | Closed | git history |
| FR-083 | CRD 插件系统 — Webhook 拦截器与自动化生命周期 | P3 | Closed | git history |
| FR-084 | Daemon Configuration Hot Reload | P2 | Closed | git history |
| FR-085 | Filesystem Trigger — 文件系统变更原生触发器 | P1 | Closed | collision (2): FR-085-filesystem-trigger.md; FR-085-long-running-agent-test-fixture.md |
| FR-086 | CLI Command to Simulate Agent Selection Logic | P3 | Closed | git history |
| FR-087 | Agent Health Policy CLI 测试夹具 — 自定义策略 QA 可验证性 | — | Closed | git history |
| FR-088 | QA Doctor CLI — 可观测性指标暴露 | — | Closed | git history |
| FR-089 | SecretStore 加密密钥紧急恢复机制 | — | Closed | git history |
| FR-090 | 轻量化单步执行 — orchestrator run 命令 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-091 | Linux Sandbox Filesystem Isolation Backend | P3 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-092 | Pipeline 变量 Spill 路径可配置 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-093 | 沙箱可配置读取路径白名单 | P2 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-094 | 自定义 Step ID 的显式 Scope 跨 Round-Trip 漂移修复 | P1 | Closed | legacy exception: Pre-registry README entry has no FR document path in the complete HEAD ancestry. |
| FR-095 | Process Timeline Read Model | P0 | Closed | git history |
| FR-096 | Attention Inbox | P0 | Closed | git history |
| FR-097 | Handoff Briefing and Safe Resume | P1 | Closed | git history |
| FR-098 | Agent Session Control Plane | P1 | Closed | git history |
| FR-099 | Source Events and Slack Process Binding | P1 | Closed | git history |
| FR-100 | Agent Process Console UI | P1 | Closed | git history |
| FR-101 | Canonical Control-Plane Action Audit Envelope | P0 | Closed | git history |
| FR-102 | Agent Session Control Plane Hardening And Acceptance | P1 | Closed | git history |
| FR-103 | Process Console Recovery, Attention Notifications, And Live E2E | P1 | Closed | git history |
| FR-104 | Process Console Operational Metrics And Local Dashboard | P1 | Closed | git history |
| FR-105 | Session RuntimePolicy Authority And Deterministic Control Gates | P0 | Closed | git history |
| FR-106 | Agent Process Console Release Acceptance And Rollback Runbook | P1 | Closed | git history |
| FR-107 | Slack Reaction Source Event Contract | P1 | Closed | git history |
| FR-108 | Source Task Template And Skill Invocation Resource | P1 | Closed | git history |
| FR-109 | Source Task Binding And Badge Matching Resource | P1 | Closed | git history |
| FR-110 | Slack Permalink Resolution And Canonical Task Routing | P0 | Closed | git history |
| FR-111 | Source Automation Reliability, Policy, And Operations | P1 | Closed | git history |
| FR-112 | Process Console Source Automation UI | P1 | Closed | git history |
| FR-113 | Slack Reaction Skill Automation Release Acceptance | P1 | Closed | git history |
| FR-114 | Managed Slack Connection 与官方 App OAuth 快速路径 | P0 | Closed | git history |
| FR-115 | 每 Workspace 独立 Slack App 自动 Provisioning | P0 | Closed | git history |
| FR-116 | A: Codex Driver 会话续接语义验证（Follow-up） | P3 | Closed | collision (2): FR-116-A-codex-session-resume-verification.md; FR-116-agent-driver-abstraction.md |
| FR-117 | A: 全局 Skill 目录属主/权限位校验（Follow-up） | P2 | Closed | collision (2): FR-117-A-global-skill-directory-ownership-check.md; FR-117-non-code-workspace-and-global-file-sharing.md |
| FR-118 | 协调塌缩 — 用 orchestrator-owned MCP 工具替换过渡态 CEL 层 | P1 | Closed | git history |
| FR-119 | Expert Resources 可达列表与受治理编辑闭环 | P1 | Closed | git history |
| FR-120 | Handoff 恢复审查对话框焦点生命周期 | P2 | Closed | git history |
| FR-121 | Attention Mutation 错误反馈与权威状态对账 | P1 | Closed | git history |
| FR-122 | CLI、Daemon 与 Tauri 边界层覆盖率治理 | P1 | Closed | git history |
| FR-123 | 受控 Slack Sandbox 持续认证与证据保鲜 | P1 | Closed | git history |
| FR-124 | 协调坍缩 Strangler 迁移收尾与遗留路径退役治理 | P1 | Closed | git history |
| FR-125 | 遗留协调机器分级退役（Deprecate → Remove） | P2 | Closed | git history |
| FR-126 | Agent 执行路径迁移 — Showcase 链接闭环与文档门禁补强 | P1 | Closed | git history |
| FR-127 | 治理门禁执行面补完 — QA 门禁 CI 接线与脚本执行面分类 | P0 | Closed | git history |
| FR-128 | 治理台账再生与审阅工具 — 消除手工 SHA256 维护摩擦 | P1 | Closed | git history |
| FR-129 | Skill 单一来源与镜像完整性 — 修复损坏的 `.agents` 镜像 | P1 | Closed | git history |
| FR-130 | Core Crate 拆分 Phase 3 — persistence 提取 | P1 | Closed | git history |
| FR-131 | 文档发布链路单一来源与链接完整性门禁 | P2 | Closed | git history |
| FR-132 | QA/DD 文档生命周期治理 — 退役标注与索引可导航性 | P2 | Closed | git history |
| FR-133 | 依赖策略门禁 — 重复版本、许可证与来源约束 | P3 | Closed | git history |
| FR-134 | 门禁执行事实校验 — 消除 FR-127 中"文本存在性即执行"的代理 | P1 | Closed | git history |
| FR-135 | 边界层覆盖率 job 恢复 — bash 3.2 空数组与产物路径 | P1 | Closed | git history |
| FR-136 | 持久化依赖收口决策 — 新 crate 是收口点还是又一个共享依赖 | P1 | Closed | git history |
| FR-137 | governance job 聚合清单的完整性断言 | P2 | Closed | git history |
| FR-138 | bash 3.2 兼容性扫描器的跨行词法状态与漏报面 | P2 | Closed | git history |
| FR-139 | 持久化收口门禁的扫描面与断言有效性 | P2 | Closed | git history |
| FR-140 | 治理执行成本 — 使其可见、可归因、可预算 | P3 | Closed | git history |
| FR-141 | 持久化层不再交出驱动连接 — 连接能力的 API 边界 | P1 | Closed | git history |
| FR-142 | 触发历史上限从未生效 — 级联删除的决策与修复 | P1 | Closed | git history |
| FR-143 | 变异 fixture 的靶标漂移 — 第三条 meta 断言 | P2 | Closed | git history |
| FR-144 | jq 供给的门禁循环在输入畸形时静默变为空转 | P2 | Closed | git history |
| FR-145 | `producer \| consumer -q` 在 `pipefail` 下是一个假失败——也是一个假通过 | P2 | Closed | git history |
| FR-146 | `producer \| head -N` 在 `pipefail` 下会让门禁中途终止，而截断的运行读起来和完整的一模一样 | P2 | Closed | git history |
| FR-147 | 两个由 CI 执行的 shell 门禁不在执行面清单里，因此每一道派生扫描器都看不见它们 | P2 | Closed | git history |
| FR-148 | 没有任何东西检查 fixture 是否还能被产品接受 | P2 | Closed | git history |
| FR-149 | DD-137 移除了两个构造，19 个 fixture、一道门禁和四份 QA 文档还在描述它们 | P2 | Closed | git history |
| FR-150 | 发布链路完整性修复 | P0 | Closed | git history |
| FR-151 | 0.4.0 版本发布与 Unreleased 清算 | P0 | Closed | git history |
| FR-152 | 首跑路径现代化 — quickstart、fixture 与错误码可读性 | P1 | Closed | git history |
| FR-153 | 供应链与依赖面治理 | P1 | Closed | git history |
| FR-154 | CLI 输出正确性与三文档面一致性 | P2 | Closed | git history |
| FR-155 | 文档与仓库现实对齐 — AGENTS.md、幻觉基础设施、台账再生 | P2 | Closed | git history |
| FR-156 | pipelineVariables 清单授权面退役 | P2 | Closed | git history |
| FR-157 | Source 域分解与测试补强 | P3 | Closed | git history |
| FR-158 | 治理体系自省 — 门禁的门禁、成本与新鲜度 | P3 | Closed | git history |
| FR-159 | 交互会话进程回收 — 孤儿泄漏与 OS 层回收缺口 | P1 | Closed | git history |
| FR-160 | QA harness 的守护进程拆解 —— `wait` 在 23 个门禁里是空操作 | P2 | Closed | git history |
| FR-161 | path-shadow 隔离对登录 shell 下的 provider 解析不成立 | P2 | Closed | git history |
| FR-162 | 失败可见性契约 —— 步骤失败、任务状态与收件箱的三方矛盾 | P1 | Closed | git history |
| FR-163 | 连接、路径与就绪的单源化 | P2 | Closed | git history |
| FR-164 | 审计动作具名化与无信封缺口 | P1 | Closed | git history |
| FR-165 | 账本与契约的驱动化 —— 从"有记录"到"有排程" | P2 | Closed | git history |
| FR-166 | 概念面收敛 —— 双词汇表、重叠 kind 与概念预算 | P2 | Closed | git history |
| FR-167 | Delete 路径的审计缺口——具名与信封双缺 | P1 | Closed | git history |
| FR-168 | Task 删除的引用处置策略 —— 级联清 1/8，其余七表无裁决 | P1 | Closed | git history |
| FR-169 | 数据目录消失后守护进程不自证死亡 —— 22 小时、零日志、服务零客户端 | P2 | Closed | git history |
| FR-170 | 单例守卫把证据存在它必须幸存的那个目录里 | P2 | Closed | git history |
| FR-171 | 四种资源可以写入但读不出来 | P1 | Closed | git history |
| FR-172 | 治理记录只增不减 —— 闭环尾注 17 倍膨胀与 superseded 文档的无限留存 | P1 | Closed | git history |
| FR-173 | v0.7 兼容层退休窗口 —— 六个 legacy 码、两个别名，以及「可解析拒绝」自身何时退休 | P2 | Proposed | git history |
| FR-174 | PR 反馈延迟由治理决定 —— 关键路径 24 分钟，其中产品验证 5 分钟 | P2 | Proposed | git history |
<!-- END GENERATED FR REGISTRY -->

## 说明

- `P0`: 对安全性、控制面暴露面或系统可信边界有直接影响
- `P1`: 对系统一致性、平台成熟度、生产可用性有显著影响
- `Proposed`: 已形成正式需求，尚未进入实现阶段
- `In Progress`: 已有部分阶段落地，剩余阶段仍在治理中
- `Implemented`: 需求已完成并进入维护阶段
- 已闭环并删除的 FR，应由对应 `docs/design_doc/**` 与 `docs/qa/**` 继续承载设计和验证信息
- FR-172 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/190-governance-record-compaction.md` 与 `docs/qa/orchestrator/228-governance-record-compaction.md` 承载。净减 111,214 字节（57.4%）；本条自身受它设立的 400 字符上界约束。
- FR-171 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/189-resource-observability-tiers.md` 与 `docs/qa/orchestrator/227-resource-observability-tiers.md` 承载。四条裁决、step 0 对本 FR 自身表格的更正、以及三次变异验证（含第一版测试通过了它本该抓住的那个变异）均记于 DD-189，此处不复述。
- FR-127 至 FR-133 源自 2026-07-25 的技术负债深挖，共同特征是**治理编写侧严格而执行侧未接线**：门禁、镜像、同步链路、依赖策略均存在「写了但不跑」或「从未被检查」的缺口。七条均已闭环，各自的结果见其闭环尾注与承载文档。
- FR-150 至 FR-158 源自 2026-08-01 的全维度技术负债审计（需求→架构→用户体验→工程化，at `9bcfaa96`），共同特征是**核心代码质量高而「外壳」系统性断裂**：发布链路、首跑路径、依赖治理、CLI 文档与门禁自省各有缺口。九条均已闭环，各自的结果见其闭环尾注与承载文档。
- FR-170 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/186-daemon-artifact-ownership.md` 与 `docs/qa/orchestrator/224-daemon-artifact-ownership.md` 承载。
- FR-169 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/185-daemon-data-dir-vanish-self-termination.md` 与 `docs/qa/orchestrator/223-daemon-data-dir-vanish-self-termination.md` 承载。
- FR-168 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/184-task-delete-reference-disposition.md` 与 `docs/qa/orchestrator/222-task-delete-reference-disposition.md` 承载。
- FR-167 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/183-delete-audit-completeness.md` 与 `docs/qa/orchestrator/221-delete-audit-completeness.md` 承载。
- FR-166 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/182-concept-surface-convergence.md` 与 `docs/qa/orchestrator/220-concept-surface-convergence.md` 承载。
- FR-165 已闭环删除；其四项需求的设计与验证信息现由 `docs/design_doc/orchestrator/{179-ledger-driven-manual-gate-freshness,180-forward-only-rollback-contract,181-two-sided-ratchets,187-markdown-link-gate-process-substitution-crash}.md` 与 `docs/qa/orchestrator/{217-manual-gate-freshness-enforcement,218-forward-only-rollback-contract,219-two-sided-ratchets,225-markdown-link-gate-loop-shape}.md` 承载。
- FR-150 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/161-release-pipeline-integrity.md` 与 `docs/qa/orchestrator/199-release-pipeline-integrity.md` 承载。
- FR-151 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/162-release-0-4-0-phantom-reconciliation.md` 与 `docs/qa/orchestrator/200-release-0-4-0.md` 承载。
- FR-157 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/170-source-domain-decomposition.md` 与 `docs/qa/orchestrator/208-source-domain-decomposition.md` 承载。
- FR-152 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/163-first-run-path-modernization.md` 与 `docs/qa/orchestrator/201-first-run-path-modernization.md` 承载。
- FR-154 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/167-cli-output-render-chokepoint.md` 与 `docs/qa/orchestrator/205-cli-output-doc-parity.md` 承载。
- FR-156 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/169-pipeline-variable-surface-retirement.md` 与 `docs/qa/orchestrator/207-pipeline-variable-surface-retirement.md` 承载。
- FR-155 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/168-docs-reality-alignment.md` 与 `docs/qa/orchestrator/206-docs-reality-alignment.md` 承载。
- FR-153 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/164-supply-chain-dependency-governance.md` 与 `docs/qa/orchestrator/202-supply-chain-dependency-governance.md` 承载。
- FR-137 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/149-governance-aggregation-completeness.md` 与 `docs/qa/orchestrator/187-governance-aggregation-completeness.md` 承载。
  FR 原文的四处事实偏差：计数"20 个 id / 20 条 `OUTCOMES`"实为 **22 / 22**，而本 README 第 98 行当时写的 21 也是错的——该清单历经 **19 → 20 → 21 → 22** 四个 FR 周期各长一条，而描述它的三份文档各自停在不同数字上，这一漂移本身比它试图陈述的数字更能支持该 FR 的论点，故写入 DD-149 而非当作笔误抹去；需求 1 限定"**带 `id:` 且** `continue-on-error: true`"会**放过它自己描述的复现**——无 `id` 的吞掉失败步骤从构造上就无法被聚合且对该检查完全不可见，而这恰是更易犯的那一种（`id` 只在有人已打算读 outcome 时才会写下，忘 `id` 与忘 `OUTCOMES` 是同一次疏忽），故规则改为 `continue-on-error` ⇒ 必须有 `id` ⇒ 该 `.outcome` 必须被读；反向断言的**论证是反的**——FR 称悬空引用"永远解析为空、其效果与遗漏相同"，把真实聚合脚本抽出实测:空 outcome 退出码为 **1**，job 永久变红且指名一个已不存在的门禁,是**响亮**而非静默,与遗漏方向正好相反,规则保留而理由重写,并由行为断言钉死;非目标"不扩展到其他 job"若照办就要把 `ci.yml` 与 `governance` 写成字面量,正是该 FR 要消灭的枚举面本身,实测全仓库仅 `governance` 一个 job 用到 `continue-on-error`,故泛化版零成本通过,依 DD-145 为 FR-134 非目标所立的先例采纳泛化并留档
- FR-138 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/152-shell-lexical-state.md` 与 `docs/qa/orchestrator/190-bash32-scanner-lexical-state.md` 承载。
- FR-142 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/150-trigger-history-limit-cascade.md` 与 `docs/qa/orchestrator/188-trigger-history-limit-cascade.md` 承载。
- FR-143 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/155-fixture-target-drift.md` 与 `docs/qa/orchestrator/193-fixture-target-drift.md` 承载。
- FR-141 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/151-persistence-connection-capability-boundary.md` 与 `docs/qa/orchestrator/189-persistence-connection-capability-boundary.md` 承载。
  **治理期七处事实更正**（FR 文件删除后，此处是它们唯一的存续形式）：① 层外调用点实测 **54 处**（core 22 / daemon 21 / scheduler 11），不是原文的 165——原数字写于 Phase B 搬迁之前且含 `cfg(test)` 代码；② 原文的 crate 清单**漏了 `crates/integration-tests`**（`tests/trigger_fire.rs` 5 处 `.reader()`，台账给它 `test-only` 角色的合法消费者），这正是需求 1 自己写下的"枚举式清单只守得住写它时已知的东西"发生在 FR 自己身上；③ 公开面泄漏是 **87 项而非 5 项**，其中 82 项是*索取*连接的 `pub fn(conn: &Connection)` 而非*交出*——需求 1 与需求 4 因此不是同一个范围，任何单一改动都不可能同时满足两条验收标准（category conflation）；④ **改 `AsyncDatabase` 关不上门**：`db::open_conn(path)` 按路径新开连接完全绕过它，层外 27 处生产调用点，两个 forbidden crate 都持有 `state.db_path`；⑤ **`orchestrator-security` 是第三扇门而非局外人**——它的 11 个（原文记 9 个）`pub fn(conn: &Connection)` 是 daemon 那 4 处 `open_conn` 存在的唯一原因；DD-147 因它位于 core *之下*而判 `exempt`，那是关于**位置**的真话与关于**形状**的假话：一个豁免 crate 的 API 索取连接，就是把驱动**向上推过层**、推进那个被禁止持有它的 crate；⑥ `fn other` 是 4 份而非 6 份，其中 2 份在层内、不会也不该消失；⑦ 本 FR 是 DD-147 冻结残量的**偿付方**，而原文对这笔最大的 ledger diff 只字未提。三扇门串联（门 3 → 门 2 → 门 1），已裁决全关；只关门 1 会让新门禁在自己的 §4.4 反问上失败——"这条断言还会在什么坏状态上通过？"答案是"今天这个状态"。
  **结果**：公开面 `yields` 与 `demands` 双双归零（原 6 与 79），层外连接获取归零（原 81 处 / 25 文件），`core-boundary-ledger.json` 的 rusqlite 引用归零（原 9）。**DD-147 冻结的残量已清零**：daemon 22 引用 / 19 SQL 与 scheduler 17 / 16 均为 0，`crates/daemon` 的 manifest 任何 section 都不再出现驱动（`forbidden` → `none`），`core` 的角色由 `persistence` 更正为 `forbidden`（它在 FR-130 Phase A 就不再是持久化层，而台账此后又宣称了四个 FR），驱动声明移入 `[dev-dependencies]`。DD-147 写下却从未实现的那条规则——"残量归零后声明本身开始失败"——由 `stale_residual_errors` 补上。`schema-snapshot.sql` 全程逐字节未变（语句逐条搬家，未改写）。搬不走的测试消费者（core 自己的测试用 `TestState`/`create_task_impl` 构造夹具，在层*之上*；以及按 Rust 设计就在 crate 隐私边界之外的 `tests/round_trip.rs`）经 `test-support` feature 取连接，而门禁**清点该模块而非跳过它**——跳过等于认证一项自己观察不到的豁免；生产代码调用它是 `E0433` 编译错误，已实测。
- FR-140 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/153-governance-execution-cost.md` 与 `docs/qa/orchestrator/191-governance-execution-cost.md` 承载。
  **但它把成本归错了地方，而这处更正重写了需求 2 的全部内容。** FR 称主要开销是隔离机制的实现——逐 case 全树复制。实测：`test-persistence-dependency.sh` 全部 22 次 `new_case` 复制合计 **6.2 秒**，而该脚本整体 195 秒，复制占 **3.2%**；真正的代价是**每棵隔离树上都要跑一次完整门禁**，而四道门禁各有 9–22 秒耗在同一个函数里——`RustLexer.mask_literals` 用 `String#[]` 逐字符走过 6.4 MB 的 Rust 并对每个字符调用一次 `raw_string_start`（415 文件合计 **37.4 秒**，而拿到掩码后的 `strip_test_modules` 只要 0.099 秒）。在最大的单文件上：现行实现 2726ms，什么都不做的 `String#[]` 空走 892ms，改走 `chars` 数组的同一趟空走 15ms——贵的不是遮蔽，是**走路**。另有一项边界：把治理 job 的每个门禁步骤按是否声明 `cargo` 分类并接上真实步骤时长，声明 cargo 的占 **1449s（63.6%）**，而"不优化 cargo 编译时间"是本 FR 自己的非目标，故按原文实施的需求 2 命中的是它获准触碰的那 36% 里的 3%。
  改法因此换成重写 `mask_literals`（走字符数组），它比原方案**更严格**地满足需求 2 自己的约束："以现有 fixture 套件的通过与否作为改造正确性的判据"——换 worktree 或按需复制会改变每棵 fixture 树装的东西，一条在新树上仍然通过的断言可能是因为新的理由；重写词法器则输入输出逐字节相同，不动任何 fixture、不动任何断言、不共享任何树，判据可以比"套件全绿"更强。实测：**415/415 文件逐字节相同**、26 条手写对抗构造全同、7000 条随机输入零差异；速度 37416ms → **1498ms（25×）**。逐套件（断言数均未减少）：`test-persistence-dependency.sh` 195s → 29s（22 条）、`test-core-boundary.sh` 360s → 57s（14 条）；对照组 `test-doc-lifecycle.sh` 与 `bash32-compat.rb` 均不是该函数的消费者，**均未移动**——若什么都变快了，那是换了尺子而非修好了缺陷。
  需求 3 的上限定为 **2700 秒（45 分钟）**，理由不取自当前值（FR 明令禁止）：45 分钟是本 FR 立项时**整条流水线**端到端的区间上界，这条线说的是"治理自己不得花掉整条流水线过去所花的时间"；定线时该二 job 实测 4798 秒，故它立刻且大幅地约束。复核条件写在台账里：新门禁塞不下时，加它的人要么腾出空间、要么在文件里书面抬高上限。另引入 `pendingMeasurement`——新加的步骤从未跑过、不可能有数字，该窗口须具名并写明理由，且**其间预算不予执行**（对一份已知缺步的总数比对上限，会报出不存在的余量），刷新时自动清除。
  附注中"`test-governance-ledger-tooling.sh` 在 CI 中从未成功过"已不成立（最近五次运行均 success）；`boundary-coverage` 连红 6 次属实但是历史事实，`ci-liveness.rb` 的文件头注释正以它作为该台账存在的理由。**本 FR 自身撞出了 FR-144**：往清单里把 `providerIsolation` 写成字符串而非对象，`jq` 中途报错退出 5，而 `done < <(jq ...)` 不观察退出码，`check_provider_isolation` 读到零行返回成功——**真实门禁报告 PASS，只有 fixture 套件的三条负向 fixture 失败**。
- FR-147 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/160-enforcement-manifest-completeness.md` 与 `docs/qa/orchestrator/198-enforcement-manifest-completeness.md` 与 `docs/qa/README.md` 承载。
- FR-149 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/159-dd137-fixture-residue-retirement.md` 与 `docs/qa/orchestrator/197-dd137-fixture-residue-retirement.md` 承载。
- FR-148 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/158-fixture-bundle-validity.md` 与 `docs/qa/orchestrator/196-fixture-bundle-validity.md` 承载。
- FR-146 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/157-pipefail-short-circuit.md` 与 `docs/qa/orchestrator/195-pipefail-short-circuit.md` 承载。
- FR-145 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/157-pipefail-short-circuit.md` 与 `docs/qa/orchestrator/195-pipefail-short-circuit.md` 承载。
- FR-144 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/154-jq-status-observed.md` 与 `docs/qa/orchestrator/192-jq-status-observed.md` 承载。
- 「零行」必须由调用方声明（`require-rows` / `allow-empty`，**无默认值**）的理由由方向给出而非风格：同一份清单里 `check_surface_complete` 读空会让磁盘上每个文件显得未分类，**向失败一侧倒**；`check_provider_isolation` 读空则让循环体一次不执行，**向通过一侧倒**——而代码里没有任何东西区分这两者。逐调用点捕获**并不充分**：`test-docs-publishing-integrity.sh` 在四层嵌套进程替换内部读策略，非零返回无处可去，故失败另行落盘（子 shell 不能回传状态但可以写文件），门禁在末尾问一次。实测注入 `.sources` 类型错误时 `check_policy_fresh` **对一份它读不了的策略报告 PASS**，仅靠该运行级记录才转红——这一条单独证明了该机制值得存在。
- 本 FR 由撞上缺陷的人当场写下、未经复核，因此它自己犯了它所记录的那一类错误：用一个廉价的文本代理代替了要测的事实。三处实现缺陷是**跑出来的而不是读出来的**：`gate_jq_rows` 最初以 `rows="$(jq …)"; status=$?` 读状态，这只在 `set -e` 已被抑制处成立，在 `set -e` 生效的进程替换内该赋值直接触发 ERR，shell 在 `status` 被读取前就离开，失败记录从未写下——**FR 自身的缺陷在它的修复内部重现**；扫描器的可达性表把「问过且答否」当作「可达」，误报了一个 awk 函数与一个 ruby 函数；fixture 脚本自身的 pass/fail 文案在双引号内引用了被禁模式（词法器按设计将双引号区域视作代码），并使用了 governance job 未提供的 `python3`。
- FR-130 已闭环删除；其三个阶段的设计与验证信息现由 `docs/design_doc/orchestrator/{142-core-boundary-freeze,148-persistence-crate-extraction}.md` 与 `docs/qa/orchestrator/{180-core-boundary-freeze,186-persistence-crate-extraction}.md` 承载。

  闭环判据不是「三个 phase 都做完」也不是「还有残余所以不能关」：**「保留并记录理由」与「被阻塞」是合法的闭环状态，因为各自带着一个决策和一个后继；没有决策的不是。** 18 个 Phase B 文件各有书面结论（15 迁出或拆分、3 保留并记录理由），残余 3 处文件全部指向 FR-141——而 FR-141 的非目标写明须在 Phase B 闭环之后开始，所以本 FR 闭环恰是那个后继的前置条件。Phase C 亦非原文预设的二选一：实测 `impl From<rusqlite::Error> for OrchestratorError` 只有一个消费者，搬走那段 SQL 后它即为死代码并被删除，其保证的 `ExternalDependency` category 由具名映射函数显式承接并经反向变异钉住。

  三件交给后继而非随本 FR 消失的事：`persistence/repository/config.rs` 原记作「被阻塞于 crd 下沉」，收尾时重新推导发现**那个解锁条件回答的是 Phase A 的问题**（整个文件能否下沉——会成环）而非 Phase B 的（语句能否下沉——不需要任何 crd 类型），故无需为 crd 立 FR；**保留的三个文件的 SQL 护栏从未被审计**，而做审计的机制（搬迁时不得不逐条读语句）永远照不到它们，已随调用点一并移交 FR-141；触发历史上限**从未生效过**（`task_items` 引用 `tasks(id)` 无 `ON DELETE CASCADE`，任何真跑过的 task 都删不掉，调用方只记日志），已记入 DD-148 的 Known limits，其修法需先回答一个产品决策而非重构问题。

  搬迁本身产出的最大价值不在计数：五条 routing 安全护栏此前无任何测试覆盖、一个 resume 竞态被顺手关掉、多条断言在第一次变异下就通过并各自暴露断言自身的缺口。DD-148 记下两条变异测试的系统性失效方式——一条不变量可能由多条语句各持拷贝；施加于语句的断言不等于施加于守卫的断言。
- FR-139 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/147-persistence-dependency-chokepoint.md` 与 `docs/qa/orchestrator/185-persistence-dependency-chokepoint.md` 承载。

  三条中只有第二条改变了数字。`+2` 的双向判据是"修了缺陷而非换了口径"的证据本身——放宽匹配的诱人修法会把 `crates/cli/src/commands/guide.rs` 的 20 条帮助文案读成 SQL，故 case 14 以"日志散文必须**不**被计入"与 case 12 的"`PRAGMA` 必须被计入"同等强度断言，且两者在同一个文件上进行，使前者的绿不可能来自"这个文件根本没被读"。第三条选择放宽扫描面而非收窄散文，理由是条件 1 早已把 `[build-dependencies]` 归为生产声明，收窄会让两个条件对"生产"的定义不一致；五个 build script 当前均无驱动无 SQL，故台账 `references` 不因此变化。`scripts/lib/rust_source.rb` 因此接受单文件作为扫描根，`core-boundary.rb` 仍为 `200 / 37` 与 `52 / 924 / 143`、coordination 四条棘轮仍为 `53 / 30 / 9 / 0`，即该改动是纯增量的双向验证。

  未修而记录：`test*.rs` 的排除按**文件名**而非 `cfg(test)` 判定，`crates/orchestrator-runner/src/test_env.rs`（`lib.rs:23` 为无条件的 `pub(crate) mod test_env;`）是当前唯一活例，当前无驱动无 SQL。该规则与 `core-boundary.rb` 共用，改动会移动其已评审的 `200 / 37`，属另一次独立评审，已记入 DD-147 的 Known Limits。
- FR-136 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/147-persistence-dependency-chokepoint.md` 与 `docs/qa/orchestrator/185-persistence-dependency-chokepoint.md` 承载。

  FR 原文的事实偏差：非 core 的"23 个文件 75 处引用"来自对 `src/` 的朴素 `grep`，把测试代码一并计入，与它所引用的 DD-142 口径（`RustSource.scannable_source` 剥离 `#[cfg(test)]`）相反——按同一口径 core 精确复现为 200/37，非 core 实为 **15 个文件 55 处**（scheduler 17 而非 37、security 6 而非 7、slack-gateway 10 而非 9，daemon 22 正确）；被点名为生产消费者的 `service/task.rs`(4) **一处生产引用都没有**，4 处全在第 462 行 `#[cfg(test)]` 之下；判定性用例 `task_state.rs` 是 8 处而非 9 处；`spawn.rs`(3) 实为两个不同文件。

  但真正改变答案的是原文从未建立的三条结构性事实：`orchestrator-security` **位于 core 之下**（`core/Cargo.toml` 依赖它，反向不成立），它按路径自开连接正是因为不能向上依赖，原文却把它当作可迁移的同层消费者；`slack-gateway` **没有任何工作区依赖且拥有另一个数据库**（`config.rs:23` 明称 gateway-owned），把它接入共享持久化 crate 会制造当前并不存在的耦合；而 `rusqlite` 出现次数本身就是**会漏报的代理指标**——`AsyncDatabase::writer()` 返回 `&tokio_rusqlite::Connection`，`conn.execute(sql, [])` 不需要任何 `rusqlite::` 路径，`secret_store_crypto.rs` 因此有 4 条生产 SQL、0 处引用，只查 manifest 的门禁会报它干净。所以 A/B/C 三分法与真实依赖图不吻合，分层线必须画在数据库上。

  需求 4 的前提也不成立：scheduler 与 daemon 中**没有任何显式事务**，core 之外全部 11 处显式事务属于 slack-gateway（10，自有库）与 security（1，已豁免）。原文称为最难一类的事务边界接口在被禁止的一侧根本不需要——这使严格形态远比原文估计的便宜，而这一"便宜"是原文的错误前提掩盖掉的
- FR-135 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/146-bash32-compatibility.md` 与 `docs/qa/orchestrator/184-bash32-compatibility.md` 承载。

  FR 原文的四处事实偏差：解释器**不是**来自 workflow 的 `shell: /bin/bash -e`（`ci.yml` 无 `defaults`、该 step 也未声明 `shell:`），而是脚本自身 `#!/usr/bin/env bash` 在 macOS runner 上解析到 3.2 —— 这决定了给 step 加 `shell: bash` 修不了任何东西，且暴露面是**所有** macOS job 执行的 shell 文件而非一个 step；`BASH_COMPAT=3.2` 对 bash 5.3 实测无法恢复其中任何一类语义，故语义半边只能托管在 macOS job 上，Linux runner 上必须如实报 skip；`mapfile` 并非仅是 FR-126 期间的历史形态，`.claude/skills/security-test-doc-gen/scripts/extract_surface.sh` 仍有 4 处，另有 FR 未提及的 `declare -A`（`scripts/qa/test-coordination-strangler.sh:154`）；朴素规则会误报的 `${!a[@]}` 与 `${#a[@]}` 在 bash 3.2 下**实测安全**，`scripts/regression/lib/probe-runner-lib.sh` 正是这一形态，不应改写。提交数"42"在 FR 撰写时精确（今为 77）。

  实施中另发现两项 FR 未预见者：第一处缺陷修复后，`boundary-coverage` 才跑到 `cargo llvm-cov` 里两分半，暴露出第二道从未被观察到的阻塞——`tauri::generate_context!` 在编译期读取 `frontendDist: ../../gui/dist`，而该 job 只做了 `npm ci` 从未构建前端 bundle，`orchestrator-gui` 在此根本无法编译；一个 job 因某一原因常红，会把第二个原因无限期地藏起来，"第一处错误已修"并不等于"这个 job 能跑"。其次，本门禁的 wrapper 自身也在被扫描面内，因此其全部 fixture 必须以 here-document 写出，且 builtin 规则必须要求命令位置——否则路径 `$WORK/hazard/mapfile.sh` 里的字样会被当成调用；case 8 原本自称测试"无法解析的脚本"，而其 fixture `if [ -z "$1" ; then` 在 `bash -n` 下合法，该 case 一直建立在一条不成立的前提上
- FR-134 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/145-gate-surface-execution-truth.md` 与 `docs/qa/orchestrator/183-gate-surface-execution-truth.md` 承载。

  FR 原文的六处事实偏差：台账规模"45 = 45 / 12 ci-required"在开工时实为 53 = 53 / 20（FR-131 与 FR-132 在其撰写后各自接入门禁）；stale-claim 漏扫"83 个被追踪 Markdown"实为 41（FR-131 已取消追踪 36 个生成页）；"改为全集后需复核既有误报"实测为 **0 处**——`.agents`/`.cursor` 是符号链接，`git ls-files` 根本不会下降进镜像，故豁免清单以空清单发布，也因此需要一条专门证明豁免机制本身有效的 fixture；缺陷 Y 的"6 处 `monotonic` 表述"实为 8 处且其中 5 处行号已漂移，遗漏的三处是 DD-137 的治理小结、设计文档索引中 DD-137 那一行，以及 QA-175 的"the ledger remains exact and monotonic"（同时断言两条规则，其中一条已不成立）；`--write` 的 CI 识别面"两处重复"实为三处（FR-134 开启期间 `doc-lifecycle.rb` 又添了一份）；缺陷 V 的 `scripts/qa/lib/hidden-gate.sh` 并非假设——`scripts/qa/lib/slack-live-certification-lib.sh` 当时已被追踪且已完全不可见。

  实施中另发现三项 FR 未预见者：需求 12 的发现式规则立刻捕到 `1f5af317` 引入的 `.claude/skills/orchestrator-guide/orchestrator-guide`——一条指向自身、解析到从不存在的 `.claude/.claude` 的被追踪符号链接，六项镜像检查全部看不见它，已删除；需求 9 的"显然修法"（逐行正则剥离字面量）比缺陷本身更糟，它看不见 `item_generate.rs:199` 的跨行原始字符串 `r#"{"items": [`，会把该模块提前 245 行闭合并把测试夹具当作生产用量交给棘轮，使 `capturesOrJsonPath` 从 53 变成 60，因此"基线不变"是一条**双向**判据而非形式要求；补齐 ripgrep 后 `slack-certification-recorded` 的 ubuntu 腿首次真正执行到断言，随即暴露 `slack_cert_file_mode` 的 `stat -f '%Lp' || stat -c '%a'` 在 GNU coreutils 下 `-f` 意为 `--file-system`、会先向 stdout 打印文件系统块再让回退值追加其后——该门禁自建立起从未在 Linux 上跑到过这一行。

  FR-127 的实质交付（执行面 3→12、台账双向完整、发现红了整个 FR 周期的 `test-legacy-coordination-decommission.sh`）不受影响，故未撤销其闭环；其"46 个门禁只有 3 个在 CI"的立论则需补一句：那 3 个里至少 2 个是死的——被 job 引用、被调度、在日志中出现，却因所在 job 未装 ripgrep 而停在 `command -v` 前置检查，一条断言都没执行过。**接线了不等于在守**，这是 FR-127 所要终结的那句话的下一层
- FR-132 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/144-doc-lifecycle-governance.md` 与 `docs/qa/orchestrator/182-doc-lifecycle-governance.md` 承载。
- FR-129 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/141-skill-mirror-integrity.md` 与 `docs/qa/orchestrator/179-skill-mirror-integrity.md` 承载。
- FR-131 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/143-docs-publishing-integrity.md` 与 `docs/qa/orchestrator/181-docs-publishing-integrity.md` 承载。
- FR-128 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/140-governance-ledger-regeneration.md` 与 `docs/qa/orchestrator/178-governance-ledger-regeneration.md` 承载。
- FR-127 已闭环删除；其门禁执行面分类、清单与磁盘双向比对、workflow 接线真实性校验、provider 隔离不变量、失效治理声明扫描与七个互相隔离的负向 fixture 现由 `docs/design_doc/orchestrator/139-qa-gate-enforcement-surface.md`、`docs/qa/orchestrator/177-qa-gate-enforcement-surface.md`、`config/governance/qa-gate-surface.json`、`scripts/qa/test-qa-gate-surface.sh` 与 `.github/workflows/ci.yml` 的 `governance` job 承载
- FR-125 已闭环删除；其精确消费者清单、capture/JSONPath 生产路径退役、窄化持久状态、显式兼容性阻塞项与退役后工具工作流证据现由 `docs/design_doc/orchestrator/137-legacy-coordination-decommission.md`、`docs/qa/orchestrator/175-legacy-coordination-decommission.md`、`config/governance/coordination-collapse-ledger.json` 与 `scripts/qa/test-legacy-coordination-decommission.sh` 承载
- FR-124 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/136-coordination-strangler-completion.md` 与 `docs/qa/orchestrator/174-coordination-strangler-completion.md` 承载。
- FR-123 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/135-slack-sandbox-continuous-certification.md` 与 `docs/qa/orchestrator/173-slack-sandbox-continuous-certification.md` 承载。
- FR-122 已闭环删除；其统一覆盖率命令、批准基线非回退门禁、Rust branch `unsupported` 语义、五类 daemon 边界矩阵、CLI/Tauri 真实 gRPC 适配器模板及 FR-095～FR-118 证据索引现由 `docs/design_doc/orchestrator/134-boundary-layer-coverage-governance.md`、`docs/qa/orchestrator/172-boundary-layer-coverage-governance.md`、`coverage/boundary-baseline.json`、`coverage/README.md` 与 `scripts/coverage-governance.sh` 承载
- FR-121 已闭环删除；其独立查询/流/mutation 错误生命周期、统一失败对账、持久可访问 alert、安全错误边界、焦点恢复、隐私安全指标与双客户端竞争证据现由 `docs/design_doc/orchestrator/133-attention-mutation-error-reconciliation.md`、`docs/qa/orchestrator/171-attention-mutation-error-reconciliation.md` 与 `scripts/qa/test-attention-inbox.sh` 承载
- FR-118 已闭环删除；其 authenticated daemon tool host、transport-only stdio shim、五个真实协调工具、完整事件证据、pilot parity、协调行数塌缩与残余跨步通道度量现由 `docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md`、`docs/qa/orchestrator/168-coordination-collapse-mcp-tools.md`、`docs/guide/coordination-tools.md`、`fixtures/manifests/bundles/coordination-collapse-pilot.yaml` 与 `scripts/qa/test-coordination-collapse.sh` 承载
- FR-117 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/128-non-code-workspace-and-global-file-sharing.md` 与 `docs/qa/orchestrator/165-non-code-workspace-and-global-file-sharing.md` 与 `docs/security/authorization/02-file-sharing-ceiling.md` 与 `docs/security/file-security/02-workspace-home-isolation.md` 承载。
- FR-117-A 已闭环删除；其 daemon UID、group/world 权限位、task-writable 路径重叠与跨平台 fail-closed 结论现由 `docs/design_doc/orchestrator/128-non-code-workspace-and-global-file-sharing.md`、`docs/qa/orchestrator/167-global-skill-directory-provenance.md` 与 `docs/security/authorization/02-file-sharing-ceiling.md` 承载
- FR-116 已闭环删除；其 driver 契约、三种 CLI provider、能力门禁、直接事件折叠、session 隐私、MCP 隔离与 shell pilot 证据现由 `docs/design_doc/orchestrator/127-agent-driver-abstraction.md`、`docs/qa/orchestrator/164-agent-driver-abstraction.md`、`docs/guide/agent-driver-model.md`、`fixtures/manifests/bundles/agent-driver-fixture.yaml` 与 `scripts/qa/test-agent-driver-abstraction.sh` 承载
- FR-116-A 已闭环删除；其 Codex CLI `0.144.5` resume 命令、同 thread 上下文继承、recorded JSONL 映射、session/认证隔离与版本漂移复验信息现由 `docs/design_doc/orchestrator/129-codex-session-resume-conformance.md`、`docs/qa/orchestrator/166-codex-session-resume-conformance.md`、`fixtures/driver/codex-cli-0.144.5-resume.json`、`scripts/qa/test-codex-session-resume.sh` 与 `scripts/qa/certify-codex-session-resume.sh` 承载
- FR-115 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/126-dedicated-slack-app-auto-provisioning.md` 与 `docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md` 承载。
- FR-114 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/125-managed-slack-connection-shared-oauth.md` 与 `docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md` 承载。
- FR-113 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/124-slack-reaction-skill-automation-release.md` 与 `docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md` 承载。
- FR-111 已闭环删除；其 bounded retry/lease recovery、Attention 恢复、安全查询/模拟/重放、暂停投影、隐私安全指标与保留策略现由 `docs/design_doc/orchestrator/122-source-automation-reliability-operations.md`、`docs/qa/orchestrator/159-source-automation-reliability-operations.md` 与 `scripts/qa/test-source-automation-operations.sh` 承载
- FR-110 已闭环删除；其 Slack outbound credential、permalink 验证、durable automation route、canonical task/audit、幂等重启收敛与角色感知深链现由 `docs/design_doc/orchestrator/121-slack-permalink-canonical-task-routing.md`、`docs/qa/orchestrator/158-slack-permalink-canonical-task-routing.md`、`fixtures/manifests/bundles/source-task-routing-fixture.yaml` 与 `scripts/qa/test-slack-reaction-task-routing.sh` 承载
- FR-109 已闭环删除；其 native SourceTaskBinding、确定性匹配、冲突回滚、热更新、引用治理与可复现证据现由 `docs/design_doc/orchestrator/120-source-task-binding-badge-matching.md`、`docs/qa/orchestrator/157-source-task-binding-badge-matching.md`、`fixtures/manifests/bundles/source-task-binding-fixture.yaml` 与 `scripts/qa/test-source-task-binding.sh` 承载
- FR-108 已闭环删除；其 native SourceTaskTemplate、确定性安全预览、热更新/重启、引用治理与可复现证据现由 `docs/design_doc/orchestrator/119-source-task-template-skill-invocation.md`、`docs/qa/orchestrator/156-source-task-template-skill-invocation.md`、`fixtures/manifests/bundles/source-task-template-fixture.yaml` 与 `scripts/qa/test-source-task-template.sh` 承载
- FR-107 已闭环删除；其 provider-neutral reaction contract、Slack normalization、非变更路由闸门与 Sources 验证现由 `docs/design_doc/orchestrator/118-slack-reaction-source-event-contract.md`、`docs/qa/orchestrator/155-slack-reaction-source-event-contract.md` 与 `scripts/qa/test-slack-reaction-source.sh` 承载
- FR-098 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/108-agent-session-control-plane.md`、`docs/qa/orchestrator/145-agent-session-control-plane.md` 与 `fixtures/manifests/bundles/session-control-mock.yaml` 承载
- FR-095 至 FR-106 已全部闭环为 DD/QA、运维手册与可执行验收产物；Console v1 已达到 release-complete，整体状态与证据由 `docs/design_doc/orchestrator/116-process-console-release-acceptance.md`、`docs/qa/orchestrator/153-process-console-release-acceptance.md`、`docs/guide/agent-process-console-v1-operations.md` 与 `scripts/qa/test-process-console-release.sh` 承载
- FR-106 已闭环删除；其 migration identity、populated upgrade、聚合发布门禁和 forward-only 运维/回滚证据现由 `docs/design_doc/orchestrator/116-process-console-release-acceptance.md`、`docs/qa/orchestrator/153-process-console-release-acceptance.md`、`docs/guide/agent-process-console-v1-operations.md` 与 `scripts/qa/test-process-console-release.sh` 承载
- FR-105 已闭环删除；其确定性 `_system` RuntimePolicy 权威语义、热更新/重启验证与 Session 回归证据现由 `docs/design_doc/orchestrator/115-session-runtime-policy-authority.md`、`docs/qa/orchestrator/152-session-runtime-policy-authority.md` 与 `scripts/qa/test-agent-session-control-plane.sh` 承载
- FR-104 已闭环删除；其精确产品指标、本地运营视图、投影健康、生命周期与性能证据现由 `docs/design_doc/orchestrator/114-process-console-operational-metrics.md`、`docs/qa/orchestrator/151-process-console-operational-metrics.md` 与 `scripts/qa/test-process-console-metrics.sh` 承载
- FR-103 已闭环删除；其审核恢复、Attention 通知、真实 Tauri/gRPC 垂直验收与可访问性证据现由 `docs/design_doc/orchestrator/113-process-console-recovery-notifications-e2e.md`、`docs/qa/orchestrator/150-process-console-recovery-notifications-e2e.md` 与 `scripts/qa/test-process-console-vertical-flow.sh` 承载
- FR-101 已闭环删除；其统一 Action Audit Envelope、迁移/兼容设计与可复现验证现由 `docs/design_doc/orchestrator/111-control-plane-action-audit-envelope.md`、`docs/qa/orchestrator/148-control-plane-action-audit-envelope.md` 与 `scripts/qa/test-control-plane-action-audit.sh` 承载
- FR-102 已闭环删除；其 Session Control Plane 硬化、迁移/重启/并发/RBAC/GUI 验收现由 `docs/design_doc/orchestrator/112-agent-session-control-plane-hardening.md`、`docs/qa/orchestrator/149-agent-session-control-plane-hardening.md` 与 `scripts/qa/test-agent-session-control-plane.sh` 承载
- FR-100 已闭环删除；其信息架构、迁移/回滚设计与可复现 UI 验证现由 `docs/design_doc/orchestrator/110-process-console-information-architecture.md`、`docs/qa/orchestrator/147-process-console-ui.md` 与 `scripts/qa/test-process-console-ui.sh` 承载
- FR-097 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/107-handoff-and-safe-resume.md`、`docs/qa/orchestrator/144-handoff-and-safe-resume.md` 与 `scripts/qa/test-handoff-safe-resume.sh` 承载
- FR-099 已闭环删除；其设计、验证与 deterministic fixture 现由 `docs/design_doc/orchestrator/109-source-events-and-slack-binding.md`、`docs/qa/orchestrator/146-source-events-and-slack-binding.md`、`fixtures/manifests/bundles/source-events-fixture.yaml` 与 `scripts/qa/test-source-events-slack.sh` 承载
- FR-096 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/106-attention-inbox.md`、`docs/qa/orchestrator/143-attention-inbox.md` 与 `scripts/qa/test-attention-inbox.sh` 承载
- FR-095 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/105-process-timeline-read-model.md`、`docs/qa/orchestrator/142-process-timeline-read-model.md` 与 `scripts/qa/test-process-timeline.sh` 承载
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
- FR-007 已闭环删除；其收口结果由 `docs/architecture.md`、`docs/guide/**`、`.claude/skills/orchestrator-guide/**` 与 `docs/qa/**` 持续承载
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
- FR-035 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/12-degenerate-cycle-loop-guard.md` 与 `docs/qa/orchestrator/88-degenerate-cycle-loop-guard.md` 承载
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
- FR-068 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/78-gui-connection-resilience-notification.md`、`docs/qa/orchestrator/120-gui-connection-resilience.md` 与 `docs/qa/orchestrator/120b-gui-notification-error-humanization.md` 承载
- FR-069 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/79-gui-polish-theme-i18n-responsive.md`、`docs/qa/orchestrator/121-gui-polish-visual.md` 与 `docs/qa/orchestrator/121b-gui-i18n-ux.md` 承载
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
- FR-086 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/94-agent-selection-threshold-verification.md` 与 `docs/qa/orchestrator/110b-agent-health-policy-advanced.md` 承载（采用 Option 2 单元测试验证路径：`is_capability_healthy_custom_threshold` + `test_diseased_agent_with_passing_capability_threshold_is_selected` 确定性证明 diseased agent 在自定义 `capability_success_threshold` 下的选中行为）
- FR-086（原 daemon config hot reload 议题）已闭环；其设计与验证信息现由 `docs/design_doc/orchestrator/92-daemon-config-hot-reload.md` 与 `docs/qa/orchestrator/133-daemon-config-hot-reload.md` 承载（ArcSwap 原子快照机制实现无重启配置热加载，`persist_config_and_reload()` 在 apply 响应前同步更新 `config_runtime`；QA 128 S2/S3 限制已移除）
- QA-106 inflight wait test fixture 已闭环删除（原 FR-085 编号冲突）；其设计与验证信息现由 `docs/design_doc/orchestrator/93-long-running-agent-test-fixture.md` 与 `docs/qa/orchestrator/106-inflight-wait-heartbeat-aware-timeout.md` 承载（3 项集成测试直接验证 `wait_for_inflight_runs()` 的 heartbeat 重置、超时回收、诊断事件；S1-S5 全部 ☑）
- FR-088 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/qa-doctor-observability.md` 与 `docs/qa/orchestrator/134-qa-doctor-observability.md` 承载
- FR-089 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/95-secretstore-key-emergency-recovery.md` 与 `docs/qa/orchestrator/135-secretstore-key-emergency-recovery.md` 承载
- FR-083 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/96-crd-plugin-system.md` 与 `docs/qa/orchestrator/136-crd-plugin-system.md` 承载（通用 CRD 插件系统：interceptor/transformer/cron 三种插件类型，webhook.authenticate/webhook.transform 扩展点，crdRef 触发器关联，内置 orchestrator tool 工具库）
- FR-087 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/97-agent-health-policy-cli-fixtures.md` 与 `docs/qa/orchestrator/110b-agent-health-policy-advanced.md` 承载（经审计确认 fixture manifest + `orchestrator apply --project` 完整管线已正确保留 health_policy，新增 `scripts/qa/test-health-policy-check.sh` 自动化验证 3 场景：自定义阈值、disease DISABLED、默认策略基线）
- FR-090 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/98-lightweight-step-run.md` 与 `docs/qa/orchestrator/138-lightweight-step-run.md` 承载（三阶段轻量化执行：Phase 1 `--step`/`--set` 步骤过滤与变量注入、Phase 2 `orchestrator run` 同步执行命令、Phase 3 `RunStep` RPC 脱离 workflow 直接组装单步执行）
- FR-091 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/99-linux-sandbox-filesystem-isolation.md` 与 `docs/qa/orchestrator/139-linux-sandbox-filesystem-isolation.md` 承载（Linux mount namespace + bind-mount 文件系统隔离：workspace_readonly / workspace_rw_scoped 两种模式，unshare -m 组合网络命名空间，preflight 动态检测 unshare/mount 二进制）
- FR-092 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/configurable-spill-path.md` 与 `docs/qa/orchestrator/100-configurable-spill-path.md` 承载（Workspace 级 `artifacts_dir` 配置，spill 文件写入工作区内，沙箱 agent 可访问）
- FR-093 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/sandbox-readable-paths.md` 与 `docs/qa/orchestrator/101-sandbox-readable-paths.md` 承载（ExecutionProfile `readable_paths` 字段、`~`/`$VAR` 路径展开、Linux bind-mount RO 挂载、`ORCHESTRATOR_READABLE_PATHS` 环境变量供 agent wrapper 消费）
- FR-094 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/step-scope-roundtrip-leak.md` 与 `docs/qa/orchestrator/141-step-scope-roundtrip-leak.md` 承载。
- FR-119 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/131-expert-resources-governed-editing.md` 与 `docs/qa/orchestrator/169-expert-resources-governed-editing.md` 承载（五类 daemon 权威资源目录、可应用 canonical Describe、受审核 revision fence Apply、Action Audit 隐私与可访问 Expert UI 均已闭环）
- FR-120 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/132-handoff-dialog-focus-lifecycle.md` 与 `docs/qa/orchestrator/170-handoff-dialog-focus-lifecycle.md` 承载（手动与 Attention 自动审查入口、焦点围栏与确定性恢复、异步失效防护、失败可操作性和 Chromium 可访问性均已闭环）
- FR-126 已经第四轮严格审计补证后重新闭环删除；mark-done showcase 已对齐 `claude/cli` typed-driver 当前事件与 artifact，全部 `docs/showcases/**/*.md` 进入退役语义扫描，EN/ZH 指南下游链接与正向语义由确定性门禁验证。设计与验证证据由 `docs/design_doc/orchestrator/138-agent-driver-execution-migration.md`、`docs/design_doc/orchestrator/guide-alignment.md`、`docs/qa/orchestrator/176-agent-driver-execution-migration.md` 与 `docs/qa/orchestrator/guide-alignment.md` 承载。
- FR-133 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/156-dependency-policy-gate.md` 与 `docs/qa/orchestrator/194-dependency-policy-gate.md` 承载。
- FR-159 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/171-interactive-session-process-reclamation.md` 与 `docs/qa/orchestrator/209-interactive-session-process-reclamation.md` 承载。
- FR-076 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/165-gui-ci-integration.md` 与 `docs/design_doc/orchestrator/166-gui-release-packaging.md` 与 `docs/qa/orchestrator/203-gui-ci-integration.md` 与 `docs/qa/orchestrator/204-gui-release-packaging.md` 承载。
- FR-164 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/177-apply-audit-action-completeness.md` 与 `docs/qa/orchestrator/214-apply-audit-action-completeness.md` 承载。
- FR-162 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/176-failure-visibility-contract.md` 与 `docs/qa/orchestrator/213-failure-visibility-contract.md` 承载。
- FR-158 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/172-governance-expansion-boundary.md` 与 `docs/design_doc/orchestrator/173-ratchet-masking-and-surface-closure.md` 与 `docs/qa/orchestrator/210-governance-system-introspection.md` 承载。
- FR-159 与 FR-150~158 的审计批次不同源：它来自 2026-08-02 对开发机运行态的一次直接观测（`ps` 实测 28 个存活 19 天的 fixture 会话进程与 6 个 `ppid=1` 仍在 LISTEN 的 orchestratord），而非文档或门禁面的静态审计。
- FR-163 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/178-runtime-layout-single-source.md` 与 `docs/qa/orchestrator/215-connectivity-path-single-source.md` 与 `docs/qa/orchestrator/216-daemon-readiness-and-connection-semantics.md` 承载。
