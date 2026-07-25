# FR-132: QA/DD 文档生命周期治理 — 退役标注与索引可导航性

## 优先级: P2

## 状态: Proposed

## 背景

本项目以"FR → DD + QA + guide + zh 镜像 + showcase"为标准闭环产物，这套约定产生了极高的可追溯性，但也带来单调增长的文档面。2026-07-25 实测：

| 指标 | 值 |
|---|---|
| Markdown 文件总数 | 596 |
| Markdown 总行数 | 85702 |
| `docs/qa/orchestrator/` | 209 篇 |
| `docs/design_doc/orchestrator/` | 141 篇 |
| 生产代码行数 | 100213 |
| 文档:代码 行数比 | ≈ 0.86 : 1 |

关键问题不是数量本身，而是**没有退役机制**：DD/QA 文档一经创建即永久有效，即使其描述的机制已被后续 FR 取代。FR-126 的四轮审计已经暴露了这个模式的代价——DD-101、DD-102、DD-103、DD-127 都需要**事后**补上"此为历史记录，非当前配置指导"的围栏，而每一轮都是靠人工审计发现的，因为没有任何东西标记它们已被取代。

现状下判断一篇 DD 是否仍然有效，唯一途径是通读全文并与代码比对。对 141 篇 DD 而言这不可持续，且随每个新 FR 恶化。

`docs/feature_request/README.md` 的闭环说明段落（当前 120+ 行）承载了"哪个 FR 由哪些 DD/QA 承载"的映射，但它是单向的：从 DD 出发无法反查它属于哪个 FR、是否已被取代。

## 目标

- 为 DD/QA 文档引入轻量、机器可读的生命周期状态（active / superseded），使"这篇还算数吗"可脚本回答。
- 让取代关系双向可导航：从被取代文档能找到取代者。
- 用门禁保证新增 DD/QA 必须带生命周期元数据，并保证 `superseded-by` 指向的目标存在。

## 非目标

- **不**删除任何历史 DD/QA——它们是治理审计轨迹的一部分，只标注不销毁。
- **不**重写 141 篇 DD 的正文；只补前言元数据，正文内容原样保留。
- **不**引入外部文档管理工具或数据库；元数据用 Markdown frontmatter 承载。
- **不**改变 FR 闭环流程本身（`fr-governance` skill 的四阶段不变，只在 Phase 5 增加元数据要求）。
- **不**要求一次性回填全部历史文档（见需求 4 的分阶段策略）。

## 需求

### 1. 生命周期 frontmatter 约定

- 定义 DD/QA 的 frontmatter 字段：`status`（`active` / `superseded`）、`related_fr`、`superseded_by`（当 status 为 superseded 时必填，指向取代文档的仓库相对路径）。
- 与既有的 `self_referential_safe` frontmatter 约定共存，不冲突。
- 在 `.claude/skills/fr-governance/SKILL.md` 的 Phase 5 与 `.claude/skills/qa-doc-gen/SKILL.md` 中登记该要求，使新产物默认带元数据。

### 2. 元数据完整性门禁

- 断言：`docs/design_doc/**` 与 `docs/qa/**` 下每篇文档都带合法 frontmatter；`superseded` 状态必须有存在的 `superseded_by` 目标；`related_fr` 格式合法。
- 允许豁免清单（带理由），用于结构性索引文件（如 README）。
- 按 FR-127 的分类进入 CI 强制执行面。

### 3. 取代关系与已知历史围栏对齐

- 将 FR-126 期间人工补上的历史围栏（DD-101/102/103 等"此为历史记录"表述）转为结构化的 `status: superseded` + `superseded_by`。
- 保留正文中的自然语言围栏——门禁读元数据，人读正文，两者不互相替代。

### 4. 回填策略

- 分阶段：新增文档立即强制；存量文档按"被后续 FR 触及时回填"的机会主义策略，并记录回填进度。
- 门禁初期以豁免清单容纳未回填的存量文档，清单长度单调不增（棘轮），确保回填只进不退。

### 5. 反向索引

- 提供从 DD/QA 反查所属 FR 与取代链的能力（可由脚本从 frontmatter 生成索引文件，不要求人工维护）。

## 验收标准

- [ ] frontmatter 约定已文档化，并在 `fr-governance` / `qa-doc-gen` skill 中登记
- [ ] 元数据完整性门禁存在并进入 CI
- [ ] 负向 fixture：新增无 frontmatter 的 DD → 失败；`superseded_by` 指向不存在文件 → 失败；`superseded` 但缺 `superseded_by` → 失败
- [ ] DD-101/102/103 等已知历史文档标注为 `superseded` 且指向现行取代者
- [ ] 未回填存量文档的豁免清单存在，且其长度有单调不增棘轮与负向 fixture
- [ ] 反向索引可由脚本生成，且与 frontmatter 一致（有测试）
- [ ] 既有 `scripts/qa-doc-lint.sh` 与文档对齐门禁不因 frontmatter 引入而误报
- [ ] `cargo test --workspace`、strict Clippy、既有 CI job 全部通过

## QA 计划

- **元数据负向 fixture**（三条独立证伪）：缺 frontmatter、`superseded_by` 目标不存在、`superseded` 缺 `superseded_by`——各自单独触发失败。
- **棘轮负向 fixture**：向豁免清单追加一项 → 失败（证明清单只能缩短）。
- **回填正确性抽检**：对已标注 `superseded` 的文档，验证其 `superseded_by` 目标确实描述了取代后的机制（人工核对，记录于 QA 文档）。
- **兼容性回归**：确认新增 frontmatter 后 `qa-doc-lint.sh` 的既有断言、`test-agent-driver-documentation-alignment.sh` 的全量 Markdown 扫描、以及 VitePress 构建均不受影响。
- **索引一致性**：修改一篇文档的 `related_fr` 后重新生成索引，断言索引随之变化且与 frontmatter 一致。
