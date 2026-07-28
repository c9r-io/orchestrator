# FR-147: 两个由 CI 执行的 shell 门禁不在执行面清单里，因此每一道派生扫描器都看不见它们

## 优先级: P2

## 状态: Proposed

## 背景

`config/governance/qa-gate-surface.json` 是本仓库的执行面声明。仓库里越来越多的
扫描器把**扫描范围**从它派生出来，而不是写清单——这是 FR-143 之后确立的做法，
理由是"手写清单只守住写下它的那天已知的东西"。

但派生的前提是 manifest 本身完整。实测（`cae30e41`）：

| 脚本 | 由谁执行 | 在 manifest 里 |
|---|---|---|
| `scripts/qa-doc-lint.sh` | `.github/workflows/ci.yml:222` | **否** |
| `scripts/coverage-governance.sh` | `boundary-coverage` job | **否** |
| `scripts/check-async-lock-governance.sh` | `.github/workflows/ci.yml:30` | **否** |

第三项是在 FR-145 的认证扫描里、用工作流模型做第二次推导时才出现的——手数只找到前两个。
这本身就是本 FR 的论点：差集必须由 `workflow_model.rb run-commands` 派生，不能靠人看。

三者都由 CI 执行，都不在 `qa-gate-surface.json` 的 `scripts[]` 里。于是：

- `scripts/qa/jq-status-observed.rb`（范围＝manifest 中 ci-required 的 `.sh` ＋
  `scripts/lib`）看不见它们。
- `scripts/qa/fixture-target-drift.rb` 同理。

`qa-doc-lint.sh` 尤其刺眼：`test-agent-driver-documentation-alignment.sh` 在
manifest 里的 `invokedBy` 就是它——**被调用方被治理，调用方不被治理**。
FR-145 那次假失败正是从这条调用链上报出来的。

## 目标

- 让"由 CI 执行的 shell 门禁"与"manifest 中的条目"这两个集合可比对，并且差集为空
  或每一项都有写明的理由。

## 非目标

- **不**在本 FR 内改变任何扫描器的规则，只改变它们能看见的范围。

## 需求

### 1. 先测量差集，再决定补哪一边

从 `scripts/lib/workflow_model.rb` 的 `run_commands` 派生出每个 workflow job 实际
执行的 `./scripts/**.sh`，与 manifest 的 `scripts[].path` 求差集。**当前已测得三项**（`qa-doc-lint.sh`、`coverage-governance.sh`、
`check-async-lock-governance.sh`，`993c5509`，方法即上述派生）；反向差集是七项，
全部是 manifest 里带 `invokedBy` 的条目，由各自的 wrapper 执行，属预期。

### 2. 补进 manifest 会改变三道门禁的派生集合

`jq-status-observed.rb`、`fixture-target-drift.rb` 与 FR-145 新增的
`pipefail-short-circuit.rb`（后者范围是 `git ls-files '*.sh'`，不受影响）。
补条目之前必须先让这两道门禁在扩大后的范围上通过，否则一次 manifest 编辑会同时
点亮两道无关门禁——这正是 FR-145 把它排除在范围外的原因。

### 3. 一条能响的护栏

差集非空且没有写明理由时失败。形式可以是 `test-qa-gate-surface.sh` 的一条新 check，
它已经在读 workflow 模型。

## 验收标准

- [ ] 差集已由工作流模型派生（不是手数），且当前值有记录
- [ ] 差集为空，或每一项在 manifest 或策略文件里有写明的豁免理由
- [ ] 护栏存在，且有一个能让它响的负向 fixture（删掉一个条目 → 失败）与一个对照
- [ ] `jq-status-observed.rb` 与 `fixture-target-drift.rb` 在扩大后的范围上通过，
      且各自通过数不减少

## 备注

发现于 FR-145 的事实复核（复核 G）。记入
`docs/design_doc/orchestrator/157-pipefail-short-circuit.md` 的已知限制。
