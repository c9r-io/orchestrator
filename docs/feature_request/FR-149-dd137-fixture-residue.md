# FR-149: DD-137 移除了两个构造，19 个 fixture、一道门禁和四份 QA 文档还在描述它们

## 优先级: P2

## 状态: Proposed

## 背景

DD-137（`1b0937ca`，2026-07-25）整类退役了 `behavior.captures` 与
`GenerateItems` / `SpawnTasks` 两种 JSONPath post-action
（拒绝点：`core/src/config_load/validate/workflow_steps.rs:57-74`，`ef458f16`）。

移除做完了，**依附于它的语料没有跟着走**。FR-148 建的语料门禁把残留量测了出来并
冻结在账上（`config/governance/fixture-bundle-validity.json`，`rotted_count: 19`），
但冻结不是清理——本 FR 是清理。

这条残留有活的症状：`scripts/qa/test-wp05-integration.sh:250,282,312` 对三个被拒的
bundle 执行整份 `orchestrator apply -f`，因此**自 07-25 起就跑不完**，与触发 FR-148
的 `test-coordination-collapse.sh` 是同一个病、同一个 commit。它是 `manual-runbook`
（`config/governance/qa-gate-surface.json`），所以四个多月没人看见。

## 目标

把 `rotted_count` 从 19 降到 0，且降的过程不留假绿。

## 非目标

- **不**重建 `generate_items` 或 `behavior.captures`。它们是按设计移除的。
- **不**顺手动 `fragment` / `environment` / `dependent` / `intentional` 那 12 条。
  它们内容是当前的，与本 FR 无关。

## 测量（全部在 `ef458f16`）

方法：`jq -r '.bundles[] | select(.status == "rotted") | .path'
config/governance/fixture-bundle-validity.json`，逐条对回 `git grep -l -F <basename>
-- ':!fixtures/'`。

### 19 个 rotted bundle，按消费者分三档

| 档 | 数量 | bundle | 消费者 |
|---|---|---|---|
| **A：有脚本消费者（门禁真的坏了）** | 3 | `wp05-items-invariant`、`wp05-items-select`、`wp05-store-items-select` | `scripts/qa/test-wp05-integration.sh`（`manual-runbook`）、`docs/qa/orchestrator/51-primitive-composition.md` |
| **B：只有 QA 文档消费者** | 2 | `cycle-overflow-test`、`generate-items-narrow-test` | `docs/qa/orchestrator/92-...md`、`84-...md` |
| **C：孤儿** | 14 | `qa83-s1..s5`（5）、`stagger-test-scenario1..4`（4）、`qa105-s3-correct-yaml`、`qa105-s5-prehook-declared-capture`、`qa107-s1-parallel`、`s5-pipeline-var`、`prehook-test` | 无 |

> `prehook-test` 的病因不是 DD-137，是 prehook schema 漂移（`missing field \`when\``）。
> 归在 `rotted` 是因为同一个判据：它声明的东西产品不再接受，且没人要它。

### 四份 QA 文档至今 `lifecycle: active` 却在描述被移除的机制

`docs/qa/orchestrator/` 下：`51-primitive-composition.md`、
`83-generate-items-mixed-text-extraction.md`、`84-generate-items-regression-narrowing.md`、
`92-dynamic-items-cycle-overflow.md`（方法：读四份的 frontmatter，逐份确认 scope 段
点名 `generate_items` / `GenerateItems`）。

### 删 bundle 会动到另一处，且那处不在 fixtures 里

`scripts/qa-doc-lint.sh:64-66` 用 `fixtures/manifests/bundles/*.yaml` 通配推导「已知
workflow ID 集合」，再拿 `docs/qa/orchestrator` 里每个 `--workflow <id>` 去对。
**93 个 bundle 全都喂着它**，包括 14 个孤儿。删掉任何一个，其定义的 workflow ID 就
从集合里消失；若还有 QA 文档引用该 ID，那道检查会报 `Unknown workflow ID`。

单一路径判断，未逐 ID 验证：**A 档与 B 档的 5 个 bundle 各自定义的 workflow ID 是否
仍被 QA 文档引用，实现时必须先跑一遍再删。**

## 需求

### 1. A 档：门禁与 QA 51

`test-wp05-integration.sh` 测的是已不存在的功能。要么退役该门禁并把
`qa-gate-surface.json` 的条目一并去掉，要么用类型化替代路径重写它。两条路都要求
`docs/qa/orchestrator/51-primitive-composition.md` 相应处理（`lifecycle: superseded`
+ `superseded_by`，或改写）。

### 2. B 档与四份 QA 文档

`92`、`84`（以及 `83`，其 fixture 在 C 档）按 §5.1 翻 `lifecycle: superseded` 并写
`superseded_by`，保留正文围栏。

### 3. C 档：删除

14 个孤儿删除。

### 4. 每删一个，先证明 `qa-doc-lint.sh` 不会因此变红

不是「跑一遍看是绿的」——删除前后各跑一次并 diff
`Unknown workflow ID` 行，且把「哪些 workflow ID 因此离开集合」显式列出。
`git diff --stat <before>..<after> -- fixtures/` 非空正是本 FR 与 FR-148 的差别所在。

### 5. `rotted_count` 归零，且账本条目一并删除

FR-148 的门禁对 `rotted_count` 做**等值**比较，所以少删一条账本条目就会红。
这是设计如此：清理必须把账一起清。

## 验收标准

- [ ] `config/governance/fixture-bundle-validity.json` 的 `rotted_count` 为 0，且
      `.bundles[] | select(.status == "rotted")` 为空
- [ ] `cargo test -p agent-orchestrator fixture_corpus` 通过（门禁自身会验证账与树一致）
- [ ] `bash scripts/qa-doc-lint.sh` 通过，且 `Unknown workflow ID` 行数删前删后均为 0，
      并附上「离开集合的 workflow ID」清单
- [ ] 四份 QA 文档不再以 `lifecycle: active` 描述已移除的机制
- [ ] `test-wp05-integration.sh` 要么被退役（含 `qa-gate-surface.json` 条目），要么
      在类型化路径上真的跑绿——**不接受「暂时跳过」**
- [ ] `cargo test --workspace --exclude orchestrator-gui`、strict Clippy 通过
- [ ] `ruby scripts/qa/doc-lifecycle.rb --emit-index --write` 后索引已重生成

## 备注

由 FR-148 闭环时立项，设计与测量记录在
`docs/design_doc/orchestrator/158-fixture-bundle-validity.md` 与
`docs/qa/orchestrator/196-fixture-bundle-validity.md`。
两者不互相依赖：FR-148 的门禁在本 FR 开工前就已经生效，且正是它把这 19 条数出来的。
