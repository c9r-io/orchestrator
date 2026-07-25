# FR-129: Skill 单一来源与镜像完整性 — 修复损坏的 `.agents` 镜像

## 优先级: P1

## 状态: Proposed

## 背景

本仓库以 `.claude/skills/` 为 skill 的权威来源（30 个 skill），并通过 `.agents/skills/` 的符号链接向其他 agent runtime 暴露同一批能力。2026-07-25 的深挖发现该镜像**既损坏又不完整**，且完全无门禁覆盖。

**损坏项**——`fr-governance` 的链接形状与其余 19 个都不同：

```
.agents/skills/fr-governance/SKILL.md -> ../../../.claude/skills/fr-governance   # 指向目录
.agents/skills/qa-doc-gen             -> ../../.claude/skills/qa-doc-gen          # 正确形状
```

其余 19 个是 `<name>` 直接指向 skill 目录；`fr-governance` 却是 `<name>/SKILL.md` 指向**目录**。任何按 `.agents/skills/<name>/SKILL.md` 读取的 runtime 都会拿到一个目录而非文件（`Errno::EISDIR`）。也就是说，**FR 治理 skill 本身在这个镜像里是不可用的**——这一点在本次扫描前从未被发现，因为没有任何检查会读它。

**缺失项**——30 个 skill 中有 10 个从未镜像：

```
dependabot-governance  design-brief-gen  design-governance  guide-alignment
integration-authoring  playwright-cli    qa-doc-governance  security-test-doc-gen
tools                  uiux-test-doc-gen
```

其中 `guide-alignment`、`qa-doc-governance`、`design-governance`、`dependabot-governance` 恰好都是治理类 skill——镜像缺口系统性地偏向治理能力。

**第三份副本**——仓库顶层还存在 `skills/orchestrator-guide/` 与 `skills/orchestrator-guide.skill`，由 `scripts/package-skills.sh` 从 `.claude/skills/orchestrator-guide` 打包产生，但其存在关系未被任何断言固定，容易与权威来源漂移。

## 目标

- 修复 `fr-governance` 的镜像形状，使 `.agents/skills/<name>/SKILL.md` 对全部 skill 均可解析为文件。
- 明确并固定"哪些 skill 需要镜像"的策略，消除 10 个静默缺口。
- 用确定性门禁冻结镜像不变量，使今后新增 skill 时缺口立即可见。

## 非目标

- **不**改变 `.claude/skills/` 作为唯一权威来源的地位——镜像永远是符号链接，不得出现内容副本。
- **不**在本 FR 内新增或修改任何 skill 的内容。
- **不**强制所有 skill 都镜像：允许"仅 Claude Code 可用"的 skill 存在，但必须显式声明，不能靠遗漏来表达。

## 需求

### 1. 修复 fr-governance 镜像

- 将 `.agents/skills/fr-governance` 改为与其余 19 个一致的形状（`<name>` → `../../.claude/skills/<name>`），删除错误的 `<name>/SKILL.md` → 目录链接。
- 验证修复后 `.agents/skills/fr-governance/SKILL.md` 解析为常规文件且内容与权威来源一致。

### 2. 镜像策略与缺口消除

- 为 10 个未镜像 skill 逐个决策：镜像，或在显式豁免清单中记录理由（如"依赖 Claude Code 专有工具，其他 runtime 无法执行"）。
- 豁免清单是数据文件而非注释，供门禁读取。

### 3. 镜像完整性门禁

- 新增确定性检查（可并入 `scripts/qa-doc-lint.sh` 或独立脚本），断言：
  - `.claude/skills/` 中每个 skill 要么有正确形状的镜像，要么在豁免清单中；
  - 每个 `.agents/skills/` 条目都是符号链接（非目录、非普通文件），且目标存在；
  - 每个镜像的 `SKILL.md` 可解析为常规文件；
  - 豁免清单中不存在已不复存在的 skill 名。
- 该门禁按 FR-127 的分类进入 CI 强制执行面。

### 4. 打包副本的来源固定

- 断言 `skills/orchestrator-guide/**` 与 `.claude/skills/orchestrator-guide/**` 内容一致，或将其改为构建产物并从版本控制中移除（二选一，需书面决策）。
- `scripts/package-skills.sh` 的源路径与该决策保持一致。

## 验收标准

- [ ] `.agents/skills/fr-governance/SKILL.md` 解析为常规文件，内容与 `.claude/skills/fr-governance/SKILL.md` 一致
- [ ] 全部 30 个 skill 要么已镜像，要么在豁免清单中带理由
- [ ] 镜像完整性门禁存在并进入 CI；负向 fixture 证明"新增未镜像且未豁免的 skill"会失败
- [ ] 负向 fixture 证明"镜像是目录而非符号链接"与"符号链接目标不存在"两种损坏形态均能被检出
- [ ] `skills/orchestrator-guide` 的来源关系已由断言固定或该副本已移除
- [ ] `scripts/package-skills.sh` 仍能正常产出发布包
- [ ] `cargo test --workspace`、strict Clippy、既有 CI job 全部通过

## QA 计划

- **损坏形态负向 fixture**（三条，逐条独立证伪）：
  1. 新建一个 `.claude/skills/<tmp>` 而不镜像 → 门禁失败；
  2. 把某个镜像替换为真实目录 → 门禁失败；
  3. 把某个镜像指向不存在的目标 → 门禁失败。
  每条恢复后门禁通过。
- **真实读取验证**：对全部镜像执行"按 `<name>/SKILL.md` 读取并断言为常规文件且非空"，这正是本次发现 `fr-governance` 缺陷的检查，必须固化为门禁的一部分而非一次性脚本。
- **豁免清单陈旧检测**：向豁免清单加入一个不存在的 skill 名 → 门禁失败。
- **打包回归**：`scripts/package-skills.sh` 产出的归档结构与既有发布一致。
