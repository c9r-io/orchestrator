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

> **修正（Phase 2 step 0，`74af0cd9`）：C 档不是 14 个孤儿，是 12 个。**
> `qa107-s1-parallel` 与 `stagger-test-scenario3` 各有一个活消费者，而那个消费者
> 正是 FR-148 自己建的 `docs/qa/orchestrator/196-fixture-bundle-validity.md`：
> 它的负向 fixture **点名**这两个文件（场景 3 改 `qa107-s1-parallel` 的 `expect`，
> 场景 2a 从账本里滤掉 `stagger-test-scenario3`）。B 档的 `cycle-overflow-test`
> 同样被 QA 196 场景 5 的预期结果点名。
> 上表漏掉它们，是因为账本 `consumers` 字段本身就漏了——两处同源。
> 这正是 §4.4 shape 7 落在「即将搬走目标的那个 FR」上，处置见需求 6。

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

~~单一路径判断，未逐 ID 验证：**A 档与 B 档的 5 个 bundle 各自定义的 workflow ID 是否
仍被 QA 文档引用，实现时必须先跑一遍再删。**~~

**已验证（`74af0cd9`，双路径）。** 删掉全部 19 个 bundle 后，**22 个 workflow ID
离开集合**：

```
fixed_no_dynamic  fixed_with_dynamic_items  infinite_with_dynamic_items  narrow-test
prehook_test  qa107-parallel-guard  s1-mixed-text  s2-fenced-block  s3-pure-json
s4-malformed-json  s5-multi-json  s5_pipeline_var  stagger-no-delay
stagger-sequential-ignored  stagger-step-override  stagger-workflow-level
test_s3_correct  test_s5_prehook_declared  wp05-items-invariant  wp05-items-select
wp05-store-items-select  wp05-verify-winner
```

两条独立推导一致：(1) 复刻 qa-doc-lint 自己的
`rg -A3 'kind: Workflow' | rg 'name:'`，(2) Ruby `YAML.load_stream` 逐文档取
`metadata.name`。两者都给 158 → 136，差集完全相同（0 处 parse 失败）。

**与 QA 文档 `--workflow` 引用相撞的只有一个：`narrow-test`，在
`docs/qa/orchestrator/84-generate-items-regression-narrowing.md:61`。**
A 档三个 bundle 的 ID 一个都没被引用；风险全在 B 档，且只有一条。
（`docs/qa/orchestrator` 全库共 30 处可判定的 `--workflow <id>` 引用。）

顺带一处：`fixed_no_dynamic` 与 `wp05-verify-winner` 这两个 ID 账本的 `expect`
从未点名——bundle 定义的 workflow 比诊断提到的多。删除影响面按 ID 算，不按诊断算。

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

### 6. 重新武装 QA 196——它的三个负向 fixture 会被本 FR 打哑

（Phase 2 step 0 新增。）QA 196 是 FR-148 的验证文档，它的负向 fixture 点名了
本 FR 要删的文件、并把本 FR 要改的数字写成了字面量。逐条：

| QA 196 | 现在 | 本 FR 之后 | 性质 |
|---|---|---|---|
| 场景 2a | 滤掉 `stagger-test-scenario3`，置 `rotted_count = 18` | 该条目不存在 → 过滤是空操作；`18` vs 实际 0 → 门禁以 **rot ratchet** 报错，而文档写的预期是 `undeclared rejection: ...stagger-test-scenario3.yaml` | 红，但**走的是另一条分支**——§4.4 shape 7「reported through a different branch than the one they claimed to test」 |
| 场景 3 | 改 `qa107-s1-parallel` 的 `expect` | 循环匹配不到 → 账本原样 → `cargo test` **通过** | **空过**（文档预期是失败），fail-open |
| 场景 5 | `s.replace('"rotted_count": 19', ...)` | 字面量匹配不到 → 文件不变 → 两轮全绿 | **空过**，fail-open |

场景 2b（`rotted_count = 20`）会同时触发 ratchet，预期诊断仍在，但噪音变大。

要求：**目标与期望值都从账本派生，不再复述。** §4.4 shape 7 第三条
——「Derive the expected value from the ledger; never restate it. A gate whose
subject is a number that is meant to move cannot have fixtures that only work
while it does not.」`rotted_count` 恰恰是本 FR 要移动的那个数。
同时更新场景 1 与 checklist 里的 `93 bundles / 62 accepted / 31 declared`
与 scope 段的 93，以及「19 rotted entries」的已知限制段。

### 7. 修掉 DD-158 与 FR README 里被本 FR 暴露的一处计数错

DD-158 写「Nineteen ... `behavior.captures` in nine, and `generate_items`
JSONPath post-actions in ten. ... One is prehook schema drift.」

两条推导都不给 9：按**拒绝诊断**是 `legacy_coordination_removed` 8 条 /
`legacy_json_path_removed` 10 条 / prehook drift 1 条；按**文件内容**是 10 个
文件含 `captures:`（`wp05-items-select` 与 `wp05-store-items-select` 两种构造都带）。
且该段枚举 19+5+4+1+1+2 = **32**，与它自己声明的 31 差一——`prehook-test`
在 19 之内，不在 19 之外。

同一句「`behavior.captures` 9 个」被 `docs/feature_request/README.md:107`
（FR-148 的闭环记录）原样复述，两处一起改。

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
- [ ] QA 196 的三个负向 fixture 目标与期望值均从账本派生；逐条给出**改造后仍会
      变红、且红在它声称的那条分支上**的证据（不接受「跑了一遍是红的」）
- [ ] DD-158 与 `docs/feature_request/README.md:107` 的 8/10/1 计数已改正，且该段
      枚举求和等于 31

## 备注

由 FR-148 闭环时立项，设计与测量记录在
`docs/design_doc/orchestrator/158-fixture-bundle-validity.md` 与
`docs/qa/orchestrator/196-fixture-bundle-validity.md`。
两者不互相依赖：FR-148 的门禁在本 FR 开工前就已经生效，且正是它把这 19 条数出来的。
