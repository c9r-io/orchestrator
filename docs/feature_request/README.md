# Feature Requests

本目录收录 `orchestrator` 的正式功能需求文档。Agent Process Console v1 已完成闭环；产品结构与发布边界分别由[信息架构](../design_doc/orchestrator/110-process-console-information-architecture.md)和[发布验收设计](../design_doc/orchestrator/116-process-console-release-acceptance.md)持续承载。Slack reaction 驱动的 Skill 任务自动化已发布（FR-107 至 FR-113 全部闭环删除），其设计、验证与用户指南现由 `docs/design_doc/orchestrator/118-`～`124-`、`docs/qa/orchestrator/155-`～`161-` 与 [用户指南](../guide/slack-reaction-skill-automation.md)承载；Managed Slack Connection、官方 App OAuth 与[每 workspace 独立 App provisioning](../design_doc/orchestrator/126-dedicated-slack-app-auto-provisioning.md)均已闭环。Agent 执行后端契约的供应商中立化已由 [Agent Driver 设计](../design_doc/orchestrator/127-agent-driver-abstraction.md)闭环；生产工作流的协调坍缩、冻结棘轮与退役标准由[协调 Strangler 收尾设计](../design_doc/orchestrator/136-coordination-strangler-completion.md)持续承载。

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
| FR-083 | CRD 插件系统 — Webhook 拦截器与自动化生命周期 | P3 | Closed |
| FR-084 | Agent 条件命令规则 + Session 复用 | P1 | Closed |
| FR-085 | Filesystem Trigger — 文件系统变更原生触发器 | P1 | Closed |
| FR-086 | CLI Command to Simulate Agent Selection Logic | P3 | Closed |
| FR-087 | Agent Health Policy CLI 测试夹具 — 自定义策略 QA 可验证性 | P2 | Closed |
| FR-088 | QA Doctor CLI — 可观测性指标暴露 | P2 | Closed |
| FR-089 | SecretStore 加密密钥紧急恢复机制 | P2 | Closed |
| FR-090 | 轻量化单步执行 — `orchestrator run` 命令 | P1 | Closed |
| FR-091 | Linux Sandbox Filesystem Isolation Backend | P3 | Closed |
| FR-092 | Pipeline 变量 Spill 路径可配置 | P1 | Closed |
| FR-093 | 沙箱可配置读取路径白名单 | P2 | Closed |
| FR-094 | 自定义 Step ID 的显式 Scope 跨 Round-Trip 漂移修复 | P1 | Closed |
| FR-130 | Core Crate 拆分 Phase 3 — persistence 提取 | P1 | In Progress |
| FR-133 | 依赖策略门禁 — 重复版本、许可证与来源约束 | P3 | Proposed |
| FR-137 | governance job 聚合清单的完整性断言 | P2 | Proposed |
| FR-138 | bash 3.2 兼容性扫描器的跨行词法状态与漏报面 | P2 | Proposed |

## 说明

- `P0`: 对安全性、控制面暴露面或系统可信边界有直接影响
- `P1`: 对系统一致性、平台成熟度、生产可用性有显著影响
- `Proposed`: 已形成正式需求，尚未进入实现阶段
- `In Progress`: 已有部分阶段落地，剩余阶段仍在治理中
- `Implemented`: 需求已完成并进入维护阶段
- 已闭环并删除的 FR，应由对应 `docs/design_doc/**` 与 `docs/qa/**` 继续承载设计和验证信息
- FR-127 至 FR-133 源自 2026-07-25 的技术负债深挖，共同特征是**治理编写侧严格而执行侧未接线**：门禁、镜像、同步链路、依赖策略均存在"写了但不跑"或"从未被检查"的缺口。FR-127、FR-128、FR-129、FR-131、FR-132 与 FR-134 均已闭环——执行面已打开并改由执行事实校验，覆盖面改为发现式，既有门禁的维护摩擦已降低，镜像缺口已消除，文档发布链路已单一来源化，CI 每个 job 的存活性进入台账；FR-135 也已闭环（`boundary-coverage` 已首次转绿，`known-failing` 标注移除）；后续实施顺序为 FR-133（门禁挂到该执行面上）→ FR-130 的剩余部分（唯一的结构性重构，与其余各项无依赖）。FR-076 的需求 1（GUI CI 集成）同源，已在该 FR 内单独提升为 P1
- FR-137 源自 FR-134 的闭环后审计：`governance` job 的门禁步骤改为 `continue-on-error: true` 后由末尾 `Governance result` 汇总，但那份 `OUTCOMES` 是手写枚举且无人守护——插入一个带 `id:`、`continue-on-error: true` 且恒失败、却不在 `OUTCOMES` 中的步骤，门禁仍报全绿。这是 FR-134 在别处消灭了六次的枚举式覆盖面，出现在它自己为诊断可见性所做的修复里。FR-136 闭环时该 job 增至 21 个 id，`OUTCOMES` 同步增至 21 条、差集仍为空——但"同步"靠的是作者记得，正是本 FR 要消除的东西，故属潜伏而非已发作
- FR-138 源自 FR-135 的闭环后审计：`bash32-compat.rb` 的 `code_lines` 逐行重置引号状态，跨行单引号内的 `<< WORD` 形近物被当作 heredoc 开启符，其后整个文件退出扫描且无诊断。当前两处生效——`test-qa-gate-surface.sh` 第 900 行起 252 行（`perl -e` 里的 `<<EOF`）、`test-bash32-compat.sh` 第 369 行起 16 行（`ruby -e` 里的 `hosting << job_name`）。把四类危险构造追加到被吞区域，门禁仍报 `PASS, 0 finding`；同一段放进无逃逸的文件报 3 finding。两处尾部单独重扫为 0，故属潜伏。触发第二处的正是 case 9——那条"证明 CI 中存在跑本门禁的 macOS 宿主"的防空转断言，其 ruby 数组追加记号让门禁看不见自己最后 16 行。这是 FR-134 需求 9 在 Rust 侧刚以 `rust_lexer.rb` 消灭的逐行近似，七个提交后于 shell 侧重现。另含两处漏报：数组在被 source 的库中置空、在调用方展开不被发现（DD-146 只记录了同一条规则的过报方向），以及 `COMMAND_POSITION` 的候选集含非 bash 关键字的 `not` 而缺真正的取反记号 `!`，使 `if ! mapfile` 漏过
- FR-139 已闭环删除；其三条修正（删除 `classification_errors` 中比较同一归约、任何输入都无法使其失败的总和分支，并同步改写 DD-147 与 QA-185 中把它当作现行保证的陈述；`SQL_STATEMENT` 补入 `PRAGMA` 并让引号锚跨过前导转义序列，台账 112 → 114 且增量恰为 `orchestrator-security/src/lib.rs` 与 `slack-gateway/src/store.rs` 各 +1；扫描面并入各 member 的 Cargo build script，台账新增 `scanRoots` 冻结实际读取的根）现由 `docs/design_doc/orchestrator/147-persistence-dependency-chokepoint.md` 的 Corrections 与 Known Limits 两节、`docs/qa/orchestrator/185-persistence-dependency-chokepoint.md`、`config/governance/persistence-dependency-ledger.json` 与 `scripts/qa/{persistence-dependency.rb,test-persistence-dependency.sh}`（12 → 18 条断言）承载。DD-147 不作 superseded：它仍是该机制的现行记录，本 FR 修正的是其中的陈述而非替换机制。

  三条中只有第二条改变了数字。`+2` 的双向判据是"修了缺陷而非换了口径"的证据本身——放宽匹配的诱人修法会把 `crates/cli/src/commands/guide.rs` 的 20 条帮助文案读成 SQL，故 case 14 以"日志散文必须**不**被计入"与 case 12 的"`PRAGMA` 必须被计入"同等强度断言，且两者在同一个文件上进行，使前者的绿不可能来自"这个文件根本没被读"。第三条选择放宽扫描面而非收窄散文，理由是条件 1 早已把 `[build-dependencies]` 归为生产声明，收窄会让两个条件对"生产"的定义不一致；五个 build script 当前均无驱动无 SQL，故台账 `references` 不因此变化。`scripts/lib/rust_source.rb` 因此接受单文件作为扫描根，`core-boundary.rb` 仍为 `200 / 37` 与 `52 / 924 / 143`、coordination 四条棘轮仍为 `53 / 30 / 9 / 0`，即该改动是纯增量的双向验证。

  未修而记录：`test*.rs` 的排除按**文件名**而非 `cfg(test)` 判定，`crates/orchestrator-runner/src/test_env.rs`（`lib.rs:23` 为无条件的 `pub(crate) mod test_env;`）是当前唯一活例，当前无驱动无 SQL。该规则与 `core-boundary.rb` 共用，改动会移动其已评审的 `200 / 37`，属另一次独立评审，已记入 DD-147 的 Known Limits。
- FR-136 已闭环删除；其收口决策（以 `agent_orchestrator.db` 而非 crate 层级划线：core／未来的 `orchestrator-persistence` 为持久化层，`orchestrator-scheduler` 与 `daemon` 禁止并冻结残量至 FR-130 偿清、`task_state.rs` 在被禁止的一侧，`orchestrator-security` 因位于 core 之下而书面豁免、`slack-gateway` 因自有 `gateway.db` 而不在范围内、`integration-tests` 冻结于 `[dev-dependencies]`）、core 之外全部 55 处引用的机器可读分类，以及**两条互不替代的**门禁条件（谁可以*声明*驱动，由 `[workspace] members` 发现并逐 section 解析 manifest 得出；谁可以*使用*，按文件冻结 SQL 语句数与驱动引用数并双向精确比较），现由 `docs/design_doc/orchestrator/147-persistence-dependency-chokepoint.md`、`docs/qa/orchestrator/185-persistence-dependency-chokepoint.md`、`config/governance/persistence-dependency-ledger.json`、`scripts/qa/{persistence-dependency.rb,test-persistence-dependency.sh}` 与 `.github/workflows/ci.yml` 承载。`core-boundary-ledger.json` 的 `rusqliteDependentCrates` 随之删除，规则只在一处表达。

  FR 原文的事实偏差：非 core 的"23 个文件 75 处引用"来自对 `src/` 的朴素 `grep`，把测试代码一并计入，与它所引用的 DD-142 口径（`RustSource.scannable_source` 剥离 `#[cfg(test)]`）相反——按同一口径 core 精确复现为 200/37，非 core 实为 **15 个文件 55 处**（scheduler 17 而非 37、security 6 而非 7、slack-gateway 10 而非 9，daemon 22 正确）；被点名为生产消费者的 `service/task.rs`(4) **一处生产引用都没有**，4 处全在第 462 行 `#[cfg(test)]` 之下；判定性用例 `task_state.rs` 是 8 处而非 9 处；`spawn.rs`(3) 实为两个不同文件。

  但真正改变答案的是原文从未建立的三条结构性事实：`orchestrator-security` **位于 core 之下**（`core/Cargo.toml` 依赖它，反向不成立），它按路径自开连接正是因为不能向上依赖，原文却把它当作可迁移的同层消费者；`slack-gateway` **没有任何工作区依赖且拥有另一个数据库**（`config.rs:23` 明称 gateway-owned），把它接入共享持久化 crate 会制造当前并不存在的耦合；而 `rusqlite` 出现次数本身就是**会漏报的代理指标**——`AsyncDatabase::writer()` 返回 `&tokio_rusqlite::Connection`，`conn.execute(sql, [])` 不需要任何 `rusqlite::` 路径，`secret_store_crypto.rs` 因此有 4 条生产 SQL、0 处引用，只查 manifest 的门禁会报它干净。所以 A/B/C 三分法与真实依赖图不吻合，分层线必须画在数据库上。

  需求 4 的前提也不成立：scheduler 与 daemon 中**没有任何显式事务**，core 之外全部 11 处显式事务属于 slack-gateway（10，自有库）与 security（1，已豁免）。原文称为最难一类的事务边界接口在被禁止的一侧根本不需要——这使严格形态远比原文估计的便宜，而这一"便宜"是原文的错误前提掩盖掉的
- FR-135 已闭环删除；其 bash 3.2 危险构造的全仓清除与门禁（空数组展开、`declare -A`、`mapfile`/`readarray`、`${x^^}`、`local -n`、`wait -n`、`shopt -s globstar` 七类，扫描面由 `git ls-files '*.sh'` 得出、无豁免清单）、在真实 `/bin/bash` 下**执行**每一类而非仅匹配的 fixture 组、覆盖率脚本 shell 主路径的桩化冒烟（逐字断言 `cargo llvm-cov` argv，空与非空两种分支），以及产物上传步骤的诊断保真，现由 `docs/design_doc/orchestrator/146-bash32-compatibility.md`、`docs/qa/orchestrator/184-bash32-compatibility.md`、`scripts/qa/{bash32-compat.rb,test-bash32-compat.sh,test-coverage-governance-mainpath.sh}`、`config/governance/qa-gate-surface.json` 与 `.github/workflows/ci.yml` 承载。`boundary-coverage` 已在 run `30182768742` 首次成功，日志出现 `coverage governance passed` 并上传 3.9 MB 产物，`config/governance/ci-job-liveness.json` 的 `knownFailing` 标注随之移除。

  FR 原文的四处事实偏差：解释器**不是**来自 workflow 的 `shell: /bin/bash -e`（`ci.yml` 无 `defaults`、该 step 也未声明 `shell:`），而是脚本自身 `#!/usr/bin/env bash` 在 macOS runner 上解析到 3.2 —— 这决定了给 step 加 `shell: bash` 修不了任何东西，且暴露面是**所有** macOS job 执行的 shell 文件而非一个 step；`BASH_COMPAT=3.2` 对 bash 5.3 实测无法恢复其中任何一类语义，故语义半边只能托管在 macOS job 上，Linux runner 上必须如实报 skip；`mapfile` 并非仅是 FR-126 期间的历史形态，`.claude/skills/security-test-doc-gen/scripts/extract_surface.sh` 仍有 4 处，另有 FR 未提及的 `declare -A`（`scripts/qa/test-coordination-strangler.sh:154`）；朴素规则会误报的 `${!a[@]}` 与 `${#a[@]}` 在 bash 3.2 下**实测安全**，`scripts/regression/lib/probe-runner-lib.sh` 正是这一形态，不应改写。提交数"42"在 FR 撰写时精确（今为 77）。

  实施中另发现两项 FR 未预见者：第一处缺陷修复后，`boundary-coverage` 才跑到 `cargo llvm-cov` 里两分半，暴露出第二道从未被观察到的阻塞——`tauri::generate_context!` 在编译期读取 `frontendDist: ../../gui/dist`，而该 job 只做了 `npm ci` 从未构建前端 bundle，`orchestrator-gui` 在此根本无法编译；一个 job 因某一原因常红，会把第二个原因无限期地藏起来，"第一处错误已修"并不等于"这个 job 能跑"。其次，本门禁的 wrapper 自身也在被扫描面内，因此其全部 fixture 必须以 here-document 写出，且 builtin 规则必须要求命令位置——否则路径 `$WORK/hazard/mapfile.sh` 里的字样会被当成调用；case 8 原本自称测试"无法解析的脚本"，而其 fixture `if [ -z "$1" ; then` 在 `bash -n` 下合法，该 case 一直建立在一条不成立的前提上
- FR-134 已闭环删除；其**以执行事实取代文本存在性**的四项判定（由解析 workflow step 得出的接线真实性、逐 agent 关联的 bundle 钉死校验、真实执行并双向验证的 provider 遮蔽断言、全集减豁免的 stale-claim 扫描）、**由发现而非枚举得出的覆盖面**（`git ls-files '*.md'` 全集、递归的 `scripts/qa/**` 分类与 `supportFiles[]`、由 git index 中被追踪符号链接推导的镜像根、由解析 workflow 得出的 CI job 清单）、**门禁可运行性**（依赖一致性、workspace 范围、诊断保真、provider stub 覆盖）、CI 存活性台账与环境等价性门禁，以及共享 Rust 词法器，现由 `docs/design_doc/orchestrator/145-gate-surface-execution-truth.md`、`docs/qa/orchestrator/183-gate-surface-execution-truth.md`、`config/governance/ci-job-liveness.json`、`config/governance/qa-gate-surface.json`、`scripts/lib/{rust_lexer,workflow_model,manifest_model,ci_env,gate_preamble,provider_isolation}.*`、`scripts/qa/{ci-liveness.rb,test-ci-liveness.sh,test-ci-environment-parity.sh,test-qa-gate-surface.sh,test-skill-mirror-integrity.sh}` 与 `.github/workflows/ci.yml`（`governance` 与新增的 `ci-environment-parity` job、`./.github/actions/provider-stubs` 复合 action）承载。

  FR 原文的六处事实偏差：台账规模"45 = 45 / 12 ci-required"在开工时实为 53 = 53 / 20（FR-131 与 FR-132 在其撰写后各自接入门禁）；stale-claim 漏扫"83 个被追踪 Markdown"实为 41（FR-131 已取消追踪 36 个生成页）；"改为全集后需复核既有误报"实测为 **0 处**——`.agents`/`.cursor` 是符号链接，`git ls-files` 根本不会下降进镜像，故豁免清单以空清单发布，也因此需要一条专门证明豁免机制本身有效的 fixture；缺陷 Y 的"6 处 `monotonic` 表述"实为 8 处且其中 5 处行号已漂移，遗漏的三处是 DD-137 的治理小结、设计文档索引中 DD-137 那一行，以及 QA-175 的"the ledger remains exact and monotonic"（同时断言两条规则，其中一条已不成立）；`--write` 的 CI 识别面"两处重复"实为三处（FR-134 开启期间 `doc-lifecycle.rb` 又添了一份）；缺陷 V 的 `scripts/qa/lib/hidden-gate.sh` 并非假设——`scripts/qa/lib/slack-live-certification-lib.sh` 当时已被追踪且已完全不可见。

  实施中另发现三项 FR 未预见者：需求 12 的发现式规则立刻捕到 `1f5af317` 引入的 `.claude/skills/orchestrator-guide/orchestrator-guide`——一条指向自身、解析到从不存在的 `.claude/.claude` 的被追踪符号链接，六项镜像检查全部看不见它，已删除；需求 9 的"显然修法"（逐行正则剥离字面量）比缺陷本身更糟，它看不见 `item_generate.rs:199` 的跨行原始字符串 `r#"{"items": [`，会把该模块提前 245 行闭合并把测试夹具当作生产用量交给棘轮，使 `capturesOrJsonPath` 从 53 变成 60，因此"基线不变"是一条**双向**判据而非形式要求；补齐 ripgrep 后 `slack-certification-recorded` 的 ubuntu 腿首次真正执行到断言，随即暴露 `slack_cert_file_mode` 的 `stat -f '%Lp' || stat -c '%a'` 在 GNU coreutils 下 `-f` 意为 `--file-system`、会先向 stdout 打印文件系统块再让回退值追加其后——该门禁自建立起从未在 Linux 上跑到过这一行。

  FR-127 的实质交付（执行面 3→12、台账双向完整、发现红了整个 FR 周期的 `test-legacy-coordination-decommission.sh`）不受影响，故未撤销其闭环；其"46 个门禁只有 3 个在 CI"的立论则需补一句：那 3 个里至少 2 个是死的——被 job 引用、被调度、在日志中出现，却因所在 job 未装 ripgrep 而停在 `command -v` 前置检查，一条断言都没执行过。**接线了不等于在守**，这是 FR-127 所要终结的那句话的下一层
- FR-132 已闭环删除；其 DD/QA 生命周期 frontmatter 约定（`lifecycle` / `related_fr` / `superseded_by`）、全部受治理文档的一次性回填（回填时 378 篇；计入本 FR 自身产物后为 380 篇，377 active、3 superseded、244 篇带可考证的 FR 归属）、以真实 YAML 解析而非 `key: value` 正则读取前言、由文件系统而非名单推导覆盖面、对悬空/自指/成环的 `superseded_by` 的拒绝，以及双向精确比对的反向索引现由 `docs/design_doc/orchestrator/144-doc-lifecycle-governance.md`、`docs/qa/orchestrator/182-doc-lifecycle-governance.md`、`config/governance/doc-lifecycle-index.json`、`scripts/qa/doc-lifecycle.rb`、`scripts/qa/test-doc-lifecycle.sh` 与 `.github/workflows/ci.yml` 的 `governance` job 承载。FR 原文的四处偏差：字段名 `status` 与 71 篇 DD 已有的 `**Status**:`（Approved/Implemented/Released，含义是实现成熟度而非文档时效）冲突，二者是独立维度——DD-101 正是 `Released` 且已被取代，故改名为 `lifecycle`；背景段把 DD-127 列入需补历史围栏之列，实际它是 `**Status**: Released` 且被 DD-129/130/138 与 `docs/architecture.md` 引用为**现行**驱动抽象，其横幅是发布后更新而非取代围栏，真正被取代的恰是 DD-101/102/103 三篇；需求 4 的"豁免清单长度单调不增"是最弱形式（换一进一即可绕过，正是 FR-128 中 `capturesOrJsonPath` 54 对 55 的形态），故按用户决策取消豁免清单、以一次性回填完成；"生产代码行数 100213"无法复现（原始非测试 `.rs` 为 148733，按本仓库既有口径剔除 `cfg(test)` 后为 108710），文档:代码实为约 0.73:1 而非 0.86:1。此外原文未言明的起点是 150 篇 DD 中**零篇**带 YAML frontmatter，而 226 篇 QA 多数已有——两种互不兼容的编码，共约 172 处违规。均已在 DD-144 中留档。**2026-07-25 闭环后审计补正**：`DOC_ROOTS` 原为两个根，而 `docs/security/` 的 19 篇受治理文档 frontmatter 覆盖率为 0，门禁却始终报 12/12——绿是因为它们从未被看过。其中两篇正是 `FR-117` / `FR-117-A` 的闭环承载物，与 DD/QA 同等地位。根因在 FR 原文把范围写成"`docs/design_doc/**` 与 `docs/qa/**`"，实现忠实于一个过窄的需求。现已加入 `docs/security` 并回填 19 篇（只填 `lifecycle: active`，不从文中首个 FR 编号推断归属——那通常是引用而非作者，猜测会向反向索引注入 11 条未经审阅的归属），索引由 380 篇增至 399 篇、`byFeatureRequest` 保持 244，后者正是不猜的可见代价
- FR-129 已闭环删除；其镜像策略数据文件、逐镜像根的双向覆盖比对、规范符号链接形状校验、**逐个打开 `<root>/<name>/SKILL.md` 并要求非空常规文件的读取校验**（结构性检查全绿而仅该项失败的 fixture 4a 即为本 FR 缺陷的最小形态）、策略陈旧声明检测、"源树之外不得存在被追踪的 `SKILL.md`"的单一来源规则（删除只是一次性动作，该规则才是持久不变量），以及"检查不得从注册表中消失、亦不得无负向 fixture"的自校验现由 `docs/design_doc/orchestrator/141-skill-mirror-integrity.md`、`docs/qa/orchestrator/179-skill-mirror-integrity.md`、`config/governance/skill-mirrors.json`、`scripts/qa/test-skill-mirror-integrity.sh` 与 `.github/workflows/ci.yml` 的 `governance` job 承载。FR 原文的三处事实偏差（"30 个 skill" 实为 29 个 skill 加非 skill 的 `tools/`、`skills/orchestrator-guide/` 并非 `package-skills.sh` 产出而是无生产者、该副本非"容易漂移"而是已漂移约 32KB）与其遗漏的 `.cursor/skills` 第二镜像已在 DD-141 中留档
- FR-131 已闭环删除；其发布集合策略数据文件、逐 locale 双向的"声明 vs 产出"比对（由**真实运行生成器并 diff 目录树**得出，且期望集合独立于生成器推导，否则只能证明生成器与自己一致）、同步幂等性、导航可达性的双向校验（每条导航链接必须落在已产出页面上，每个已产出页面必须被导航链接——后者在修复前有 12 个页面不满足）、以及全仓相对链接解析门禁现由 `docs/design_doc/orchestrator/143-docs-publishing-integrity.md`、`docs/qa/orchestrator/181-docs-publishing-integrity.md`、`config/governance/docs-publishing.json`、`config/governance/markdown-links.json`、`scripts/qa/test-docs-publishing-integrity.sh`、`scripts/qa/test-markdown-link-integrity.sh` 与 `.github/workflows/ci.yml` 的 `governance` job 承载。FR 原文最危险的一处偏差是把 `site/*/showcases/` 当作 `docs/showcases/` 的"手工副本"并要求直接 gitignore——实际 `site/en` 是全仓唯一的英文 showcase 正文、`site/zh` 是链接改写过的中文，按原文执行会删除 17 篇英文与 1 篇中文译文，因此先做来源回收再做取消追踪；"已积累 3 处失效链接"实为 1 处（另两处分别位于行内代码块与围栏代码块中，本就不是链接），而 FR 未发现的 6 处无扩展名站点链接恰是朴素检查器会误报的形态，故该门禁的十条正向 fixture 才是主证据；此外 FR 未察觉 `docs.yml` 的 `paths:` 过滤器根本无法触发 showcase 变更、`sync-docs.mjs` 从不删除产物、12 个已发布页面无任何导航入口，以及其"不改动导航配置"的非目标与"缺失 showcase 须出现在站点"的验收标准自相矛盾。均已在 DD-143 中留档
- FR-130 的需求 1（边界冻结）与需求 3（迁移等价证明）已闭环，FR 文档保留并降为剩余的需求 2/4；其边界台账（core 的 52 `pub mod`、924 公开项、**逐文件** 200 处 `rusqlite` 引用、6 个直接依赖 rusqlite 的 crate）、双向精确比对的棘轮、74 条迁移产出的 46 表 + 92 索引 schema 基线，以及幂等性与全部 74 个中断点的续跑等价验证，现由 `docs/design_doc/orchestrator/142-core-boundary-freeze.md`、`docs/qa/orchestrator/180-core-boundary-freeze.md`、`config/governance/core-boundary-ledger.json`、`config/governance/schema-snapshot.sql`、`scripts/lib/rust_source.rb`、`scripts/qa/core-boundary.rb` 与 `scripts/qa/test-core-boundary.sh` 承载。FR 原文的关键偏差是需求 2 的文件清单（14 个文件 11049 行）与其验收标准"core 不再直接依赖 rusqlite"不相容——实际有 37 个文件 200 处引用，其中约 22 个在同一函数内混装 SQL 与领域逻辑；另有"43 个 `too_many_arguments`"实为全工作区总数（core 仅 3 个）、公开项 742 漏计 `pub async fn`（实为 924）、51 张表实为 46 表 + 92 索引，均已在 DD-142 与重写后的 FR 中留档
- FR-128 已闭环删除；其台账再生模式（`--emit-inventory` / `--emit-baseline` / 拒绝在 CI 下执行的 `--write`）、与比对逻辑共用同一表达式的候选生成、逐 Agent 失配报告（新增/消失/变更及变化的 spec 顶层键，经 `git show HEAD:<file>` 还原被审阅 spec，并在台账与 spec 分离提交时自我诊断）、精确化的四项 source 棘轮与修正后的 `cfg(test)` 扫描口径现由 `docs/design_doc/orchestrator/140-governance-ledger-regeneration.md`、`docs/qa/orchestrator/178-governance-ledger-regeneration.md`、`scripts/qa/coordination-governance.rb`、`scripts/qa/test-governance-ledger-tooling.sh` 与 `CONTRIBUTING.md` 的审阅流程一节承载
- FR-127 已闭环删除；其门禁执行面分类、清单与磁盘双向比对、workflow 接线真实性校验、provider 隔离不变量、失效治理声明扫描与七个互相隔离的负向 fixture 现由 `docs/design_doc/orchestrator/139-qa-gate-enforcement-surface.md`、`docs/qa/orchestrator/177-qa-gate-enforcement-surface.md`、`config/governance/qa-gate-surface.json`、`scripts/qa/test-qa-gate-surface.sh` 与 `.github/workflows/ci.yml` 的 `governance` job 承载
- FR-125 已闭环删除；其精确消费者清单、capture/JSONPath 生产路径退役、窄化持久状态、显式兼容性阻塞项与退役后工具工作流证据现由 `docs/design_doc/orchestrator/137-legacy-coordination-decommission.md`、`docs/qa/orchestrator/175-legacy-coordination-decommission.md`、`config/governance/coordination-collapse-ledger.json` 与 `scripts/qa/test-legacy-coordination-decommission.sh` 承载
- FR-124 已闭环删除；其 11 个生产 Workflow 精确分类、7 个非 governance-only 迁移、逐工作流 legacy/tool 对等证据、显式 tool/session 边界、`record_metric`、self-bootstrap 两周期生存回归、冻结棘轮与三级退役标准现由 `docs/design_doc/orchestrator/136-coordination-strangler-completion.md`、`docs/qa/orchestrator/174-coordination-strangler-completion.md`、`config/governance/coordination-collapse-ledger.json`、`fixtures/manifests/bundles/coordination-strangler-parity.yaml` 与 `scripts/qa/test-coordination-strangler.sh` 承载
- FR-123 已闭环删除；其 shared/dedicated/组合认证入口、可恢复 provider checkpoint、inert 最小 secret 环境、同消息双 badge smoke、recorded provider CI、可过期安全证据、README/release 状态与显式清理 inventory 现由 `docs/design_doc/orchestrator/135-slack-sandbox-continuous-certification.md`、`docs/qa/orchestrator/173-slack-sandbox-continuous-certification.md`、`docs/guide/slack-managed-sandbox-certification-runbook.md`、`docs/qa/evidence/slack-live-certification-latest.json` 与 `scripts/qa/certify-slack-managed-live.sh` 承载
- FR-122 已闭环删除；其统一覆盖率命令、批准基线非回退门禁、Rust branch `unsupported` 语义、五类 daemon 边界矩阵、CLI/Tauri 真实 gRPC 适配器模板及 FR-095～FR-118 证据索引现由 `docs/design_doc/orchestrator/134-boundary-layer-coverage-governance.md`、`docs/qa/orchestrator/172-boundary-layer-coverage-governance.md`、`coverage/boundary-baseline.json`、`coverage/README.md` 与 `scripts/coverage-governance.sh` 承载
- FR-121 已闭环删除；其独立查询/流/mutation 错误生命周期、统一失败对账、持久可访问 alert、安全错误边界、焦点恢复、隐私安全指标与双客户端竞争证据现由 `docs/design_doc/orchestrator/133-attention-mutation-error-reconciliation.md`、`docs/qa/orchestrator/171-attention-mutation-error-reconciliation.md` 与 `scripts/qa/test-attention-inbox.sh` 承载
- FR-118 已闭环删除；其 authenticated daemon tool host、transport-only stdio shim、五个真实协调工具、完整事件证据、pilot parity、协调行数塌缩与残余跨步通道度量现由 `docs/design_doc/orchestrator/130-coordination-collapse-mcp-tools.md`、`docs/qa/orchestrator/168-coordination-collapse-mcp-tools.md`、`docs/guide/coordination-tools.md`、`fixtures/manifests/bundles/coordination-collapse-pilot.yaml` 与 `scripts/qa/test-coordination-collapse.sh` 承载
- FR-117 已闭环删除；其 task Workspace、隐式 process item、FileSharing 天花板、HOME/XDG 隔离、全局 Skill 只读访问、Console 语义与 Slack inventory pilot 证据现由 `docs/design_doc/orchestrator/128-non-code-workspace-and-global-file-sharing.md`、`docs/qa/orchestrator/165-non-code-workspace-and-global-file-sharing.md`、`docs/security/authorization/02-file-sharing-ceiling.md`、`docs/security/file-security/02-workspace-home-isolation.md`、`docs/guide/non-code-workspace.md` 与 `scripts/qa/test-non-code-workspace.sh` 承载
- FR-117-A 已闭环删除；其 daemon UID、group/world 权限位、task-writable 路径重叠与跨平台 fail-closed 结论现由 `docs/design_doc/orchestrator/128-non-code-workspace-and-global-file-sharing.md`、`docs/qa/orchestrator/167-global-skill-directory-provenance.md` 与 `docs/security/authorization/02-file-sharing-ceiling.md` 承载
- FR-116 已闭环删除；其 driver 契约、三种 CLI provider、能力门禁、直接事件折叠、session 隐私、MCP 隔离与 shell pilot 证据现由 `docs/design_doc/orchestrator/127-agent-driver-abstraction.md`、`docs/qa/orchestrator/164-agent-driver-abstraction.md`、`docs/guide/agent-driver-model.md`、`fixtures/manifests/bundles/agent-driver-fixture.yaml` 与 `scripts/qa/test-agent-driver-abstraction.sh` 承载
- FR-116-A 已闭环删除；其 Codex CLI `0.144.5` resume 命令、同 thread 上下文继承、recorded JSONL 映射、session/认证隔离与版本漂移复验信息现由 `docs/design_doc/orchestrator/129-codex-session-resume-conformance.md`、`docs/qa/orchestrator/166-codex-session-resume-conformance.md`、`fixtures/driver/codex-cli-0.144.5-resume.json`、`scripts/qa/test-codex-session-resume.sh` 与 `scripts/qa/certify-codex-session-resume.sh` 承载
- FR-115 已闭环删除；其 per-workspace private App provisioning、local-only Configuration Token、receipt-gated credential import、exact-App OAuth/events、reviewed lifecycle/migration、same-message two-badge routing、离线 cursor 恢复与受控 Slack sandbox 清理证据现由 `docs/design_doc/orchestrator/126-dedicated-slack-app-auto-provisioning.md`、`docs/qa/orchestrator/163-dedicated-slack-app-auto-provisioning.md`、`docs/guide/slack-dedicated-app-provisioning.md` 与 `scripts/qa/test-slack-dedicated-app-provisioning.sh` 承载
- FR-114 已闭环删除；其 shared official App OAuth、SourceConnection/Gateway 边界、双 workspace/daemon live certification、恢复/转移/撤销/备份证据与可复跑 harness 现由 `docs/design_doc/orchestrator/125-managed-slack-connection-shared-oauth.md`、`docs/qa/orchestrator/162-managed-slack-connection-shared-oauth.md`、`docs/guide/slack-managed-sandbox-certification-runbook.md`、`scripts/qa/test-slack-managed-shared-oauth.sh` 与 `scripts/qa/certify-slack-managed-live.sh` 承载
- FR-113 已闭环删除；其 clean-tree aggregate、双 badge 垂直链路、并发/重启恢复、真实 Tauri 边界、前向升级/兼容回滚、用户指南与隐私诊断证据现由 `docs/design_doc/orchestrator/124-slack-reaction-skill-automation-release.md`、`docs/qa/orchestrator/161-slack-reaction-skill-automation-release.md`、`docs/guide/slack-reaction-skill-automation.md`、`fixtures/manifests/bundles/slack-skill-automation-release-fixture.yaml` 与 `scripts/qa/test-slack-skill-automation-release.sh` 承载
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
- FR-094 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/step-scope-roundtrip-leak.md` 与 `docs/qa/orchestrator/141-step-scope-roundtrip-leak.md` 承载（自定义 step id 的显式 scope 跨 spec↔config 往返漂移修复：`resolved_scope` capability fallback 限定为 conventions 已知 id、`workflow_step_config_to_spec` 不再省略默认值优化、`task_ops::resolve_task_targets` 在 `QaDirectoryScan` 触发时发出 `qa_directory_scan_triggered` info 事件，超过 50 个 item 升级为 `qa_directory_scan_oversize` warning；6 个回归单测覆盖三层修复 + 一个端到端 round-trip dry-run，复制了 v3 retest 中 D1/E1 的 180-item 爆炸场景）
- FR-119 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/131-expert-resources-governed-editing.md` 与 `docs/qa/orchestrator/169-expert-resources-governed-editing.md` 承载（五类 daemon 权威资源目录、可应用 canonical Describe、受审核 revision fence Apply、Action Audit 隐私与可访问 Expert UI 均已闭环）
- FR-120 已闭环删除；其设计与验证信息现由 `docs/design_doc/orchestrator/132-handoff-dialog-focus-lifecycle.md` 与 `docs/qa/orchestrator/170-handoff-dialog-focus-lifecycle.md` 承载（手动与 Attention 自动审查入口、焦点围栏与确定性恢复、异步失效防护、失败可操作性和 Chromium 可访问性均已闭环）
- FR-126 已经第四轮严格审计补证后重新闭环删除；mark-done showcase 已对齐 `claude/cli` typed-driver 当前事件与 artifact，全部 `docs/showcases/**/*.md` 进入退役语义扫描，EN/ZH 指南下游链接与正向语义由确定性门禁验证。设计与验证证据由 `docs/design_doc/orchestrator/138-agent-driver-execution-migration.md`、`docs/design_doc/orchestrator/guide-alignment.md`、`docs/qa/orchestrator/176-agent-driver-execution-migration.md` 与 `docs/qa/orchestrator/guide-alignment.md` 承载。
