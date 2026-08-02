# FR-155: 文档与仓库现实对齐 — AGENTS.md、幻觉基础设施、台账再生

## 优先级: P2

## 状态: Proposed

## 背景

2026-08-01 审计（at `9bcfaa96`）发现多个"文档描述的仓库"与真实仓库分叉。以下事实已在治理规划阶段于 `57b02662` 重新从工作树与 git 历史派生；方法随条目记录：

1. **`AGENTS.md` 在教两个正在废弃的写法**。`git log -1 -- AGENTS.md` 定位最后修改为 2026-03-30 的 `7d70d4f4`;示例 Workspace 用 `spec.root_path`（deserialize-only 遗留别名）,示例 Agent 的 `command:` 没有 `spec.driver`（apply 即触发 `[legacy_agent_command_deprecated]`）,全文 `rg -n "driver" AGENTS.md` 零命中。这里的废弃形态是 **command-only Agent**，不是 `command:` 字段本身：显式 `shell/cli` driver 仍以 `spec.command` 为合法执行载荷。文档还没有 persistence crate、Slack gateway、Process Console 与治理门禁的维护者入口。
2. **6 个 skill 把 `project-bootstrap` 产物当成本仓库基础设施**。`deploy-gh-k8s`（承诺不存在的 `deploy/upgrade.sh`）、`ops`、`reset-local-env`、`grpc-regression`、`project-readiness`、`performance-testing` 引用本仓库不存在的 docker-compose/kubectl/k8s 路径；`docker/`、`k8s/`、`portal/` 与 reset/deploy 脚本只存在于 `.claude/skills/project-bootstrap/assets/template/`。`arch-guidance` 实际列出 8 个目录约定，其中 3 个不存在（`portal/`、`docker/`、`k8s/`）；`align-tests` 与 `e2e-testing` 仍把 `portal/` 写成前端入口，而真实 Web 前端在根 `gui/`。CI 的 skill-mirror 门禁只保证这些 skill 被正确镜像，没有验证内容中的路径声明。
3. **`docs/architecture.md` 的组件清单部分失真**。字面量 `crates/orchestrator-persistence` 零命中；根 `gui/` Web 前端缺失且被错误合并进 `crates/gui/` Tauri shell。Slack gateway 数据面已经在目录树、图和组件说明中出现，因此本 FR 只需校准而不是从零补写。根目录另有孤儿 `proto/orchestrator.proto`：`rg -o '^\s*rpc'` 分别得到旧副本 61、正本 `crates/proto/orchestrator.proto` 121，差 60；唯一 build script 只读取 crate 内正本。旧路径当前被 7 个 DD、2 个 QA 文档和英/中两份 showcase 引用（11 个非 FR 文档），不是原审计记录的 10 个 DD。注册迁移链由 `registered_migrations()` 与独立 count 测试共同给出 37，architecture 仍写 migration 35。
4. **QA 工单的版本控制语义倒置**。`.gitignore:5` 的 `docs/ticket/*.md` 只忽略 ticket 根下的 active 文件；`git check-ignore` 证明可选的 `docs/ticket/closed/x.md` 反而不被忽略。`CLAUDE.md`、运行时 ticket writer 与多个 skill 都向配置的 `ticket_dir` 写文件；`ticket-fix` 在验证后删除 ticket。运行时能递归扫描 ticket 并按状态排除 `CLOSED`，但没有实现 README 所述的归档命令或自动搬迁。
5. **两份台账失真**。表格对 FR-095→149 的 55 个编号全缺；更完整的大小写兼容 git-history 路径扫描得到 140 个历史 FR 编号，而表格只有 76 个，其中共有 77 个历史编号缺行。FR-112 全文无踪，6 行状态停在 `Implemented`。`.claude/skills/*/SKILL.md` 当前派生出 29 个 skill，`SKILLS.md` 只列 16 个，缺 13 个；原文的 30/14 把非 skill 的 `tools/` 算进了分母。
6. **根目录测试数据已失去执行消费者，但不再是零引用**。`test-yaml-warnings/` 有 5 个 tracked YAML，最早由 `fedab3d8` 于 2026-03-15 引入。FR-152 此后新增了两个引用：`core/src/fixture_driverless_tests.rs` 的临时 subtree exclusion，以及 DD-163 对该 exclusion 的退役说明。三个警告场景已有 `fixtures/manifests/bundles/qa105-*` 对应 fixture，正确/已声明 capture 场景由 `workflow_steps.rs` 单元测试覆盖；删除时必须同时移除已设计为 stale-on-delete 的 exclusion。

## 需求

### 1. 重写 `AGENTS.md`
- 示例全部改为 `spec.driver` + `work_dir`;补 persistence/slack-gateway/治理门禁的现状描述;与 `docs/architecture.md` 互链而非重复。

### 2. 幻觉 skill 处置
- 6 个 Docker/K8s skill 与 `portal/` 引用逐个决策：改写为本仓库真实路径（`gui/`、无容器的本地 daemon 运维）或明确标注"仅适用于 project-bootstrap 生成的项目"并移出本仓库 skill 根;
- 修复全量解析中发现的其他失效输入路径；对“将要创建的输出”、project-bootstrap 模板路径与 companion-repo 路径使用逐项、可失效的显式分类，不用 subtree/wildcard blanket;
- skill-mirror 门禁增加"SKILL.md 引用的仓库内路径必须存在或有精确作用域声明"检查（候选路径集合从文档解析派生，声明的目标与引用均做反向 stale 校验）。

### 3. `docs/architecture.md` 补全
- 补 `orchestrator-persistence`、区分根 `gui/` Web 前端与 `crates/gui/` Tauri shell、校准现有 `slack-gateway` 数据面；迁移编号更新到 37;
- 删除根 `proto/orchestrator.proto` 或以显式弃用头标注，11 个非 FR 文档中的旧正典路径修正为 `crates/proto/`。

### 4. 工单通道持久化决策
- 决策其一：取消 `docs/ticket/*.md` 的 gitignore（工单入库）,或改为工单写入运行时数据目录并由 CLI 提供查询;
- 无论哪个方向,`CLAUDE.md`、`docs/ticket/README.md` 与相关 skill 的指示随之一致化;`closed/` 归档约定要么实现要么删除。

### 5. 台账再生
- `docs/feature_request/README.md`：补齐全部 git-history 派生的历史 FR 编号（其中 FR-095→149 为连续 55 个），或改为脚本从 git 历史生成；6 个 `Implemented` 遗留状态收敛;
- `SKILLS.md`：由 `.claude/skills/` 目录派生生成,mirror 门禁加"SKILLS.md 覆盖全部 skill"断言。

### 6. 删除 `test-yaml-warnings/`
- 删除 5 个散落 YAML，并删除 `fixture_driverless_tests` 中为它们保留的 exclusion；保留场景由既有 `fixtures/manifests/bundles/qa105-*` 与单元测试承载。

## 验收标准

- [ ] `AGENTS.md` 不再使用 `root_path`；文档中的完整 manifest 经资源解析后每个 Agent 都有 typed driver，collect/apply warnings 中无 `[legacy_*]`（`spec.command` + `shell/cli` 是允许形态）
- [ ] 全部 `.claude/skills/*/SKILL.md` 的路径候选经解析器验证为当前仓库中存在、skill-relative 存在，或有逐项有效的 template/output/companion 作用域声明（ci-required；负向验证：坏路径、悬空声明和 blanket 声明分别失败）
- [ ] `docs/architecture.md` 明确包含 `crates/orchestrator-persistence`、根 `gui/`、`crates/gui/` 与 Slack gateway；注册迁移数为 37；根 `proto/orchestrator.proto` 已处置且 11 个非 FR 文档不再把旧路径写成正典
- [ ] 工单通道决策落地,`git check-ignore docs/ticket/x.md` 的结果与文档描述一致
- [ ] FR README 台账覆盖 git 历史派生的全部 FR 编号，SKILLS 台账与实现后的 authoritative `SKILL.md` 集合精确同集；两者均有可失败的生成/比对 fixture
- [ ] `test-yaml-warnings/` 与其 `fixture_driverless_tests` exclusion 均不存在于工作树，既有 QA-105 fixture/单元测试仍通过

## 依赖与关联

- 与 FR-152 互补:FR-152 修用户面首跑文档,本 FR 修 agent/维护者面文档。
- 需求 5 的 FR README 再生应在 FR-150~158 本批 FR 建立后执行,一并入表。
