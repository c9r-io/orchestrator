# 文档站自 2026-08-18 起构建失败：跨行的行内代码让 Vue 把 `<store>` 当成未闭合标签

**Status**: fixed，待合并后由 `docs.yml` 验证（该 workflow 只在 push 到 `main`
且路径命中 `docs/guide/**` 时运行，PR 上不触发，所以本 ticket 到那时才能删）
**Found**: 2026-08-21，在 FR-174 收尾刷新 `ci-job-liveness.json` 时（`docs.yml` 的
`deploy` job 记录为 failure，此前被分支限定的 refresh 掩盖）
**Severity**: medium —— 文档站已经三天没有发布过；不影响产品运行

## 现象

`.github/workflows/docs.yml` 的 `deploy` job 自 run `32099510889`
（2026-08-18，head `e6081c6d`）起失败，此前的三次（08-16、08-17 两次）均成功。
失败在站点构建：

```
✗ Build failed in 1.29s
build error:
SyntaxError: [plugin vite:vue] en/guide/workflow-configuration.md (391:9): Element is missing end tag.
```

同一次运行里 `docs publishing integrity: 8 passed, 0 failed` —— 发布面的治理门禁
是绿的，构建本身红。两者检查的不是同一件事。

## 根因

`docs/guide/03-workflow-configuration.md:231-232`，行内代码跨了行：

```markdown
from an earlier step reads it from the store itself — `orchestrator store get
<store> <key> --project {project_id}` — rather than having the engine route it.
```

`site/**` 由 `scripts/sync-docs.mjs` 从 `docs/guide/**` 生成（生成物被 gitignore），
VitePress 用 Vue 编译 markdown。行内代码跨行后 `<store>` 不再位于一个闭合的
`<code>` 内，Vue 的模板编译器把它读成一个 HTML 元素，而它没有 `</store>`。

`{project_id}` 在同一段里没有报错，是因为 Vue 的插值在 markdown 里被 VitePress
默认转义；未闭合的**标签**没有这层保护。

## 复现

```bash
sed -n '229,234p' docs/guide/03-workflow-configuration.md   # 看跨行的反引号
cd site && npm ci && npm run build                          # 复现构建失败
```

## 期望

`docs.yml` 的 `deploy` 通过，文档站恢复发布。

## 已修（实测验证）

改动两处，**en 与 zh 各一处**：`docs/guide/03-workflow-configuration.md:231-232`
与 `docs/guide/zh/03-workflow-configuration.md:229-230`，把跨行的行内代码收进一行。

两处都要改这一点不是顺手：en 先失败，构建就停了，zh 那处**从未有机会暴露**。
只修 en 会让下一次构建红在 zh 上，看起来像「修了没用」。这类缺陷按定义会成对出现
——同一段落的两个语言版本由同一份原文翻译而来。

**验证方式是真的跑构建，不是读 markdown**：

| 步骤 | 结果 |
|---|---|
| 修改前 `cd site && npm run build` | `exit=1`，`Element is missing end tag` —— 与 CI 逐字相同 |
| `node scripts/sync-docs.mjs` | `Synced 68 files` |
| 修改后扫描两个生成物的裸露标签 | 各 0 处 |
| 修改后 `cd site && npm run build` | **`exit=0`**，`✓ building client + server bundles`、`✓ rendering pages` |

### 一处把我引偏的读数，值得记下

Vue 报的 `en/guide/workflow-configuration.md (391:9)` **不是源 markdown 的行号**，
而是编译后 SFC 的位置。第 391 行在一个闭合良好的 YAML 围栏里，我据此一度认为
「机制推断错了」并准备推翻重来。真正有效的定位是**扫描围栏与行内代码之外的裸露标签**，
它给出唯一的一处（第 232 行），与最初的机制推断一致。

教训与本 ticket 的缺陷同形：**一个数字回答的不是你问的问题**。把编译产物的行号
当成源文件的行号读，会让人放弃一个正确的判断。

## 原建议修法（保留，供对照）

把跨行的行内代码收进一行：

```markdown
from an earlier step reads it from the store itself —
`orchestrator store get <store> <key> --project {project_id}` — rather than
having the engine route it.
```

**修完必须本地跑一次 `cd site && npm run build`**，而不是只看 markdown 渲染 ——
本 ticket 的整个类别就是「markdown 看着没问题、Vue 编译器不同意」。

## 一个应当同时考虑的加固

这类缺陷可以被机械地挡住：`docs/guide/**` 里任何**裸露的** `<xxx>` 占位符
（不在行内代码或围栏代码块内）都是 Vue 眼中的标签。当前扫描器
（`test-docs-publishing-integrity.sh`）检查的是发布面的完整性，不解析 markdown，
所以它在这次失败中是绿的 —— 这正是 §4.4 的形状：一个门禁绿着，而它没有看那件事。

若加，判据应当是**真的跑一次站点构建**，或用 Vue 的编译器解析生成物，
而不是再写一条正则去猜哪些尖括号危险。

## 与 FR-174 的关系

无。FR-174 的分支上做过一次 `ci-liveness.rb --refresh --branch feat/...`，
该分支没有 `docs.yml` 的运行，refresh 保留了更早的成功记录，因而没暴露它。
按默认分支刷新后才显形。在本 ticket 关闭前，`docs.yml :: deploy` 在
`config/governance/ci-job-liveness.json` 中记为 `knownFailing` 并指向本文件。
