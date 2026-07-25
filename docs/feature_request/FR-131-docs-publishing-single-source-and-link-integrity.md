# FR-131: 文档发布链路单一来源与链接完整性门禁

## 优先级: P2

## 状态: Proposed

## 背景

`scripts/sync-docs.mjs` 把 `docs/guide/**` 单向同步到 `site/{en,zh}/guide/**`，`docs.yml` 在部署前调用它——这条链路是单一来源的。**但 `site/{en,zh}/showcases/`（各 18 篇）不在同步范围内，是手工复制的副本**，脚本自身的注释也说明 showcases "not published"（实际却被手工放进了站点）。

漂移已经发生：

```
docs/showcases/streaming-mark-done-convergence.md   ← 存在于仓库
site/en/showcases/                                  ← 缺失
site/zh/showcases/                                  ← 缺失
```

缺失的恰好是 FR-126 门禁强制要求 EN/ZH CEL 指南必须链接、且必须包含 `claude/cli`、`driver_tool_use`、`driver_tool_result`、`driver_terminal` 正向语义的那一篇。也就是说：**仓库内的门禁是绿的，但用户在文档站上点开链接会落空**——门禁的覆盖面止步于仓库，未延伸到实际发布物。

两条链路的处理方式还相互矛盾，这本身就是缺陷的形状：

| 目录 | 来源 | 版本控制 |
|---|---|---|
| `site/{en,zh}/guide/` | `sync-docs.mjs` 生成 | 已 gitignore（`.gitignore:34-35`），tracked 数 0 |
| `site/{en,zh}/showcases/` | 手工复制 | 已提交，tracked 数 18 × 2 |

`qa-doc-lint.sh` 甚至有一条断言专门检查"site guide 文件不得被 tracked"——说明"生成物不入库"的原则已被确立，只是 showcase 从未纳入该原则。

`docs.yml` 直接 `sync-docs.mjs` → `vitepress build` → 部署，全程无漂移检查。

此外全仓 596 篇 Markdown / 85702 行**没有任何链接检查器**，已积累 3 处失效链接：

```
core/README.md                             -> core/src/runner.rs      # runner 重构后该文件已删除
docs/qa/orchestrator/125b-...-advanced.md  -> resource-model.md
.claude/skills/playwright-cli/SKILL.md     -> .playwright-cli/page-2026-02-14T19-22-42-679Z.yml
```

其中 `core/README.md` 的失效链接正是执行路径重构（FR-116/126 系列）留下的残迹——与 FR-126 四轮审计发现的漂移同源：改动会波及的文档面比改动者预期的更宽。

## 目标

- 让 showcase 与 guide 一样由脚本单向同步，消除手工副本。
- 在部署前加入站点漂移检查，使"仓库有而站点无"的情况在 CI 中失败而非静默发布。
- 建立全仓 Markdown 相对链接完整性门禁，清偿现有 3 处失效链接。

## 非目标

- **不**改变 VitePress 的站点结构或导航配置。
- **不**校验外部 HTTP 链接（网络依赖会使门禁不稳定）；只校验仓库内相对路径与锚点。
- **不**要求 `docs/**` 全部发布到站点——发布集合仍由脚本显式定义，但"定义之内的必须一致"。
- **不**处理翻译质量或 EN/ZH 内容对等（那属于 `guide-alignment` skill 的范围）。

## 需求

### 1. showcase 纳入同步脚本

- 扩展 `scripts/sync-docs.mjs`，按与 guide 相同的规则同步 `docs/showcases/**` → `site/{en,zh}/showcases/**`，包含链接重写逻辑。
- 更新脚本头部注释，使其对发布集合的描述与实际行为一致（当前注释说 showcases 不发布，与事实相反）。
- 处理 EN/ZH 来源问题：明确 showcase 的中文版来源（若无独立中文源，需书面决策是复用英文、还是建立 `docs/showcases/zh/`）。

### 2. 站点漂移门禁

- 新增检查：对同步脚本定义的发布集合，断言"源文件集合 == 站点文件集合"，两侧任一多出即失败。
- 该检查在 `docs.yml` 部署前执行，并按 FR-127 的分类进入 CI 强制执行面。
- 与 guide 的既有原则保持一致：`site/{en,zh}/showcases/` 改为生成物并加入 `.gitignore`，同时把 `qa-doc-lint.sh` 中"site guide 文件不得被 tracked"的断言扩展到 showcases。若书面决策为保留在版本控制中，则漂移门禁必须存在，且需说明为何两条链路采用不同原则。

### 3. Markdown 链接完整性门禁

- 新增脚本，扫描 `git ls-files '*.md'` 全集的相对链接目标是否存在（默认全集减去带理由的豁免，与 `test-agent-driver-documentation-alignment.sh` 的覆盖策略一致）。
- 正确处理：站点绝对路径（`/en/guide/...`，属 VitePress 路由而非文件路径）、锚点片段、符号链接目录。
- 修复现有 3 处失效链接。

## 验收标准

- [ ] `docs/showcases/**` 由 `sync-docs.mjs` 同步，`streaming-mark-done-convergence` 出现在 EN/ZH 站点
- [ ] `sync-docs.mjs` 的注释与实际发布集合一致
- [ ] showcase 的中文来源策略已书面决策并落实
- [ ] 站点漂移门禁存在；负向 fixture 证明"新增 showcase 但未同步"会失败
- [ ] `site/{en,zh}/showcases/` 与 guide 采用同一入库原则（同为生成物并 gitignore，或差异有书面理由）
- [ ] 链接完整性门禁存在；负向 fixture 证明"新增指向不存在文件的链接"会失败
- [ ] 门禁不对 `/en/...` 形式的站点路由与锚点误报（有正向 fixture 覆盖）
- [ ] 3 处现有失效链接已修复
- [ ] `npx vitepress build` 成功，`docs.yml` 部署流程未破坏
- [ ] `cargo test --workspace`、strict Clippy、既有 CI job 全部通过

## QA 计划

- **漂移负向 fixture**：新建 `docs/showcases/<tmp>.md` 但不运行同步 → 漂移门禁失败；运行同步后通过；反向（站点有而源无）同样失败。
- **链接门禁负向 fixture**：在任意文档插入 `[x](./nonexistent.md)` → 失败；插入 `[x](/en/guide/quickstart)` → **不得**失败（防误报）；插入 `[x](./existing.md#some-anchor)` → 不得失败。
- **发布物实证**：本地 `vitepress build` 后确认 `streaming-mark-done-convergence` 页面存在且 CEL 指南的链接可解析——这是区分"仓库绿"与"站点对"的关键证据。
- **幂等性**：连续运行两次同步在无源变更时不产生 diff（沿用 FR-018 的既有约定）。
