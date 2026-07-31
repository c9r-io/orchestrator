# FR-155: 文档与仓库现实对齐 — AGENTS.md、幻觉基础设施、台账再生

## 优先级: P2

## 状态: Proposed

## 背景

2026-08-01 审计（at `9bcfaa96`）发现多个"文档描述的仓库"与真实仓库分叉。除非另注,均为子代理扫描（单一方法）：

1. **`AGENTS.md` 在教两个正在废弃的写法**。最后修改 2026-03-30（约 4 个月前）,示例 Workspace 用 `spec.root_path`（deserialize-only 遗留别名）,示例 Agent 用无 `spec.driver` 的 `command:`（apply 即触发 `[legacy_agent_command_deprecated]`）,全文 `grep -n "driver"` 零命中;它还早于 `orchestrator-persistence`、Slack gateway、Process Console 与整个治理门禁流。作为 agent 首要入门文档,它系统性地生产废弃配置。
2. **6 个 skill 假设不存在的 Docker/K8s 基础设施**。`deploy-gh-k8s`（承诺 `deploy/upgrade.sh`,不存在）、`ops`、`reset-local-env`、`grpc-regression`、`project-readiness`、`performance-testing` 引用 docker-compose/kubectl/k8s;仓库内相关文件仅存在于 `project-bootstrap` 的**模板资产**中。`arch-guidance` skill 宣称的 7 个"source of truth"目录有 3 个不存在（portal/、docker/、k8s/）;`align-tests` 与 `e2e-testing` 硬编码 `portal/` 而真实前端在 `gui/`。CI 的 skill-mirror 门禁保证这些坏 skill 被正确镜像,却不检查其引用路径存在。
3. **`docs/architecture.md` 遗漏两个主要组件**。零次提及 `crates/orchestrator-persistence`（6 个 FR、3 个 DD 的主角）与根目录 `gui/` Web 前端;§2 目录树同样缺失。根目录另有孤儿 `proto/orchestrator.proto`（落后正本 60 个 RPC,build.rs 只读 `crates/proto/`,已复核 10 个 DD 仍引用旧路径为正典）。
4. **QA 工单通道无持久化**。`.gitignore:5` 为 `docs/ticket/*.md`（已复核）,工单结构性排除在版本控制外;`CLAUDE.md` 与多个 skill 却指示向该目录写工单。`docs/ticket/README.md` 描述的生命周期与 `closed/` 归档目录无任何工具实现。
5. **两份台账失真**。`docs/feature_request/README.md` 表格止于 FR-094,其后 55 个 FR 未入表,FR-112 全文无踪,6 行状态停在 `Implemented`;`SKILLS.md` 缺 14/30 个 skill 条目。
6. **零引用的根目录残留**。`test-yaml-warnings/` 5 个 YAML,自 2026-03-15 起零引用（已复核 grep 全仓库零命中）,属 FR-051/DD-63 的散落测试数据。

## 需求

### 1. 重写 `AGENTS.md`
- 示例全部改为 `spec.driver` + `work_dir`;补 persistence/slack-gateway/治理门禁的现状描述;与 `docs/architecture.md` 互链而非重复。

### 2. 幻觉 skill 处置
- 6 个 Docker/K8s skill 与 `portal/` 引用逐个决策：改写为本仓库真实路径（`gui/`、无容器的本地 daemon 运维）或明确标注"仅适用于 project-bootstrap 生成的项目"并移出本仓库 skill 根;
- skill-mirror 门禁增加"SKILL.md 引用的仓库内路径必须存在"检查（路径集合从文档解析派生）。

### 3. `docs/architecture.md` 补全
- 补 `orchestrator-persistence`、根 `gui/`、`slack-gateway` 数据面;迁移编号更新到 37;
- 删除根 `proto/orchestrator.proto` 或以显式弃用头标注,10 个引用旧路径的 DD 修正为 `crates/proto/`。

### 4. 工单通道持久化决策
- 决策其一：取消 `docs/ticket/*.md` 的 gitignore（工单入库）,或改为工单写入运行时数据目录并由 CLI 提供查询;
- 无论哪个方向,`CLAUDE.md`、`docs/ticket/README.md` 与相关 skill 的指示随之一致化;`closed/` 归档约定要么实现要么删除。

### 5. 台账再生
- `docs/feature_request/README.md`：FR-095→149 补入表格或改为脚本从 git 历史生成;6 个 `Implemented` 遗留状态收敛;
- `SKILLS.md`：由 `.claude/skills/` 目录派生生成,mirror 门禁加"SKILLS.md 覆盖全部 skill"断言。

### 6. 删除 `test-yaml-warnings/`
- 确认零引用后删除;若其场景仍有价值,并入 `fixtures/` 并纳入 fixture 校验。

## 验收标准

- [ ] `AGENTS.md` 中 `grep -c "root_path\|^.*command:" ` 的废弃形态为 0,`driver` 出现且示例可 apply 无警告
- [ ] 全部 `.claude/skills/*/SKILL.md` 引用的仓库内路径经解析器验证存在（ci-required;负向验证:引入一个坏路径能失败）
- [ ] `docs/architecture.md` 包含 persistence 与 gui;根 `proto/` 处置完成且无 DD 再引用旧路径
- [ ] 工单通道决策落地,`git check-ignore docs/ticket/x.md` 的结果与文档描述一致
- [ ] FR README 台账覆盖至最新 FR 编号（以脚本比对 git 历史中的 FR 文件集合）
- [ ] `test-yaml-warnings/` 不存在于工作树

## 依赖与关联

- 与 FR-152 互补:FR-152 修用户面首跑文档,本 FR 修 agent/维护者面文档。
- 需求 5 的 FR README 再生应在 FR-150~158 本批 FR 建立后执行,一并入表。
