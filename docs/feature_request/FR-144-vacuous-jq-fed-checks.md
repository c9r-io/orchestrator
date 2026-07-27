# FR-144: jq 供给的门禁循环在输入畸形时静默变为空转

## 优先级: P2

## 状态: Proposed

## 背景

FR-140 治理期间发现并实测复现。

`scripts/qa/test-qa-gate-surface.sh` 的检查大多是这个形状：

```bash
check_provider_isolation() {
  while IFS=$'\t' read -r path mode evidence; do
    ...
  done < <(jq -r '.scripts[] | select(...) | [.path, (.providerIsolation.mode // "null"), ...] | @tsv' "$manifest")
}
```

进程替换的退出码**没有人看**。`set -euo pipefail` 也管不到它——`< <(...)` 里的失败不
传播。于是只要 `jq` 中途报错，循环读到零行，函数返回 0，**检查报告通过**。

复现（FR-140 实际撞上的那次）：在 `qa-gate-surface.json` 里把

```json
"providerIsolation": "no-provider"
```

写成字符串而不是清单要求的 `{"mode": "no-provider"}`。`jq` 报
`Cannot index string with string "mode"` 并以 5 退出，`check_provider_isolation`
读到零行返回成功。**真实门禁 `test-qa-gate-surface.sh` 对这份清单报告了 PASS**；只有
`--fixture-test` 的三条负向 fixture（6、10、20）失败，因为它们检查的是"注入的缺陷会不会
被拒绝"，而此时该检查已经什么都不检查了。

换句话说：**一处清单笔误可以让一道门禁在自称通过的同时停止执行**，而这道门禁正是 FR-127
以来所有"写了但不跑"治理的执行面本身。

### 范围

同一形状在该门禁内有 **13 处** `done < <(jq ...)`，另有四道门禁共用它：

```
scripts/qa/test-qa-gate-surface.sh          13 处
scripts/qa/test-markdown-link-integrity.sh
scripts/qa/test-ci-environment-parity.sh
scripts/qa/test-slack-live-certification.sh
scripts/qa/test-docs-publishing-integrity.sh
```

这不是"某一条 jq 表达式写错了"，是**读输入的方式本身不观察失败**。

### 与既有治理的关系

这正是 SKILL §4.4 列的第一种形状——文本／结构存在被当作执行发生——的一个变体，但更隐蔽：
不是断言写弱了，而是断言**根本没有运行**，且运行零次与运行 N 次全部通过在退出码上不可
区分。FR-137 在聚合层面处理过同构的问题（吞掉失败的步骤消失），FR-129 的两条 meta 断言
问"这道检查存在吗""有人试图让它失败过吗"，FR-143 补上"那次尝试真的施加了变异吗"——
**没有一条问"这道检查这次读到东西了吗"**。

## 目标

- 让 jq 供给的检查在输入畸形时**响亮失败**，而不是空转通过。
- 让"这次检查读到了多少行"成为可观察量，从而使空转与通过可区分。

## 非目标

- **不**重写这些门禁的判定逻辑。缺陷在输入通道，不在判定。
- **不**引入 jq 之外的 JSON 处理器。

## 需求

### 1. 观察 jq 的退出码

为五道门禁提供一个共用的读取入口（`scripts/lib/` 下，与 `gate_preamble.sh` 同级），
先执行 jq 并检查状态，失败即以清单路径与 jq 的诊断报错退出，再把结果喂给循环。

### 2. 空结果必须是一个决策

零行有时是合法的（`staleClaimExemptions` 可以为空），有时是缺陷（`ci-required` 条目不
可能为零）。调用方须**声明**哪一种，而不是让两者共用同一条静默路径。

### 3. 负向 fixture

- 清单中写入一处类型错误（对象位置写字符串）→ 门禁失败并点名该路径与 jq 的诊断，
  **而不是通过**。
- 一个声明"结果不得为空"的检查在结果为空时失败。

## 验收标准

- [ ] 五道门禁不再有不观察 jq 退出码的 `done < <(jq ...)`
- [ ] 负向 fixture：`providerIsolation` 写成字符串 → `test-qa-gate-surface.sh` **失败**
      并点名（当前：PASS）
- [ ] 负向 fixture：声明非空的检查在读到零行时失败
- [ ] 既有 fixture 套件全绿，断言数不减少
- [ ] 设计记录写明"零行"的两种含义为何必须由调用方声明而非默认

## QA 计划

- **判据是真实门禁的行为，不是 fixture 套件的行为**。本缺陷的证据恰恰是二者分叉：
  `--fixture-test` 抓到了而 `test-qa-gate-surface.sh` 没有。负向 fixture 必须断言**真实
  门禁**在畸形清单上失败。
- 变异选**类型错误**而非删除条目：删除是作者想得到的那一种，且不会让 jq 报错。

## 附注

发现路径值得记下来：这个缺陷不是审计出来的，是 FR-140 往清单里加两条条目时手误写错了
形状，而**真实门禁没有报错**、只有 fixture 套件报错，才把它暴露出来。一道门禁与它自己的
负向 fixture 给出相反结论时，对的那个是 fixture。
