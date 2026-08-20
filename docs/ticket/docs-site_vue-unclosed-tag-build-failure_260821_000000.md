# 文档站自 2026-08-18 起构建失败：跨行的行内代码让 Vue 把 `<store>` 当成未闭合标签

**Status**: open
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

## 建议修法（未验证）

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
