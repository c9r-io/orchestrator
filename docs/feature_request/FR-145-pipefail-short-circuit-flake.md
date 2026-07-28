# FR-145: `producer | consumer -q` 在 `pipefail` 下是一个假失败——也是一个假通过

## 优先级: P2

## 状态: Proposed

> **复核记录（`f9c` 治理，基线 `f105ce66`，macOS `Darwin 25.5.0 arm64`，
> `BSD grep 2.6.0-FreeBSD`，`rg` 来自 `/opt/homebrew/bin/rg`）**
>
> 机制成立且可复现。**七项事实陈述未通过复核**，其中两项改变了本 FR 的做法。
> 每处更正就地写在下文对应位置的「复核」块里，原文保留不删——被推翻的判断本身是
> 治理记录的一部分。标题也已更正：本缺陷不是单向的。

## 背景

2026-07-28，FR-133 的认证扫描（41 道派生门禁连续跑）中，
`scripts/qa-doc-lint.sh` 报出一条**不存在的缺陷**：

```
FAIL: CHANGELOG [Unreleased] does not name the removed runner selection seam
```

而 `RunnerExecutorKind` 就在 `CHANGELOG.md` 第 74 行，稳稳落在 `[Unreleased]`
区段（第 8–131 行）之内。单独重跑该门禁 10 次（前台、后台、裸跑、`--fixture-test`）
全部通过。

> **复核 F1——报出这条消息的不是 `qa-doc-lint.sh`。**
> `CHANGELOG [Unreleased] does not name the removed runner selection seam` 位于
> `scripts/qa/test-agent-driver-documentation-alignment.sh:149`。`qa-doc-lint.sh`
> 是它在 `qa-gate-surface.json` 里的 `invokedBy`——扫描时看到的是调用方的名字。
> 文件名写错，缺陷本身没错。

原因是这一行：

```sh
printf '%s' "$UNRELEASED" | rg -q 'RunnerExecutorKind' || fail "..."
```

`rg -q` **一命中就退出**。`[Unreleased]` 区段有 **90047 字节**（`awk` 抽取，
`1272d6c9`），远超 64 KB 管道缓冲区，所以 `rg` 离开时 `printf` 还在写；它随即
死于 EPIPE，而 `set -o pipefail` 把这个非零状态交给 `||`，断言就报告了一个不存在
的文档缺陷。

**这是一个失败方向"闭"的缺陷**，比失败方向"开"更隐蔽的地方在于：它产出的是莫名其妙
的红，而对莫名其妙的红，人的第一反应是重跑到绿——于是它训练所有人忽略这道门禁。

> **复核 B——方向不是"闭"的，取决于命中路径通向哪个分支。**
>
> 上面那处，命中 ⇒ 断言通过，所以 EPIPE 把通过变成假失败（失败方向"闭"）。
> 但只要把同一段代码写成 `if producer | grep -q P; then fail; else pass; fi`，
> 命中 ⇒ 断言失败，EPIPE 就把一次**真实违规变成一次干净的通过**。同一份输入实测
> **2 / 200 次**命中被报成"未找到"。
>
> 本仓库最要紧的一处正是后者，`scripts/qa/test-agent-driver-production-parity.sh:266`：
>
> ```sh
> if ! rg -a -q '…126' "$QA_ROOT/data" &&
>    ! sqlite3 "$DB" .dump | rg -q '00000000-0000-4000-8000-000000000126'; then
>   pass "provider session material stays out of persisted database evidence"
> ```
>
> 产出方是**整库 dump**——全仓唯一无界的产出方，且随 fixture 增长。若 UUID 真在
> dump 里而 dump 超过缓冲区：`rg` 命中，`sqlite3` 死于 EPIPE，`pipefail` 把非零交给
> `!`，门禁于是在**恰好泄漏的那一刻**报告"provider 会话材料没有落进数据库"。
>
> 原文把这条缺陷登记为单向的，而登记的方向恰好是危害较小的那一向。

### 实测（`1272d6c9`，macOS，方法即下方脚本）

同一个循环，同一台机器，唯一变量是 CPU 负载（8 个忙循环）：

| 形式 | 空闲 | 满载 |
|---|---|---|
| `printf '%s' "$U" \| rg -q P` | **0 / 400** | **10 / 400**，复测 **4 / 400** |
| `rg -q P <<< "$U"` | 0 / 400 | **0 / 400** |

```sh
U="$(awk '/^## \[Unreleased\]/{flag=1;next} /^## \[/{flag=0} flag' CHANGELOG.md)"
for i in $(seq 1 400); do
  printf '%s' "$U" | rg -q 'RunnerExecutorKind' || hits=$((hits+1))
done
```

Here-string 写临时文件而非管道，没有 writer 可被信号打断，语义对 `-q` 完全等价。

**这也解释了为什么它从未被看见**：需要产出方大于管道缓冲、命中点靠前、且机器有竞争
——一次连跑 47 道门禁的认证扫描恰好三者齐备。

> **复核 A——"空闲 0 / 400"不成立，负载不是触发条件。**
>
> 同一脚本、同一台机器、`f105ce66`（区段仍为 90047 字节，命中点在第 **59273** 字节）：
>
> | 形式 | 无人为负载 | 8 个忙循环 |
> |---|---|---|
> | `printf '%s' "$U" \| rg -q P` | **8 / 400**，复测 **13 / 400** | **10 / 400** |
> | `printf '%s' "$U" \| grep -q P` | **3 / 400** | **2 / 400** |
> | `rg -q P <<< "$U"` | **0 / 400** | **0 / 400** |
>
> 安静的机器上就有 2–3% 的触发率；负载把它抬高约四分之一，但不是它成立的前提。
> 原文由此推出的"为什么从未被看见"是错的，而且错在让缺陷显得比实际更罕见的方向。
>
> 另：原文只测了 `rg`。BSD `grep -q` 同样中招，只是速率较低——修复范围不能按
> 消费方的实现来划。

## 目标

- 把这一形状从 `ci-required` 门禁里清掉，或对每一处写明它为何不可能触发。
- 留下一条能被触发的护栏，否则下一个人写回同样的管道时没有任何东西会响。

> **复核 E——第一条目标的后半句（"写明它为何不可能触发"）不可执行，已废弃。**
> 判据见需求 2 下方的复核块。护栏的形状因此从"标注 + 检查标注"改为"语法规则，
> 无豁免口"。

## 非目标

- **不**全局禁止 `|`。绝大多数管道的消费方读到 EOF 才退出，不存在这个竞态。
- **不**改 `set -o pipefail`。它抓到的真实缺陷远多于这一处假阳性。
- **不**在本 FR 内处理 `head -n1`、`| tee`、进程替换等其它短路消费方，除非测量显示
  它们也在 `ci-required` 面上（见下方"未验证"标注）。

> **复核——`| head` 的测量已完成，结论是另开一个 FR。**
> 全仓 tracked `.sh` 中 `| head` 共 **38 处 / 28 个文件**（`f105ce66`），其中数处是
> `fail` 分支里的 `producer | head -5 >&2`——在 `set -e` 下 EPIPE 会**让门禁中途终止**，
> 而 SKILL.md §4.4 shape 7 已记录"被截断的运行读起来和完整运行一模一样"。
> 规模与本 FR 相当，合并会把一次有测量支撑的修复变成两倍面积的改写。闭环时另立 FR。

## 需求

### 1. 修掉已证实会触发的那一处

`scripts/qa/test-agent-driver-documentation-alignment.sh` 的四处
`printf '%s' "$UNRELEASED" | rg -q` 已在 FR-133 闭环时改为 here-string 并复测
（满载 0/400）。本需求只是把它记录为已完成的前置。

### 2. 逐个测量剩余站点，而不是逐个改写

实测 **42 处** `| (rg|grep) -*q`，分布在 **9 道 ci-required shell 门禁**，
全部处于 `set -o pipefail` 之下（方法：`grep -cE '\|[[:space:]]*(rg|grep)[[:space:]]+-[A-Za-z]*q'`
over `qa-gate-surface.json` 的 ci-required `.sh` 条目，`1272d6c9`）。

**其中绝大多数不可能触发**：产出方是几十到几百字节的路由名、slug、check 名清单，
`printf` 早在消费方退出前就写完了。逐个改写 42 处会把一次有测量支撑的修复变成一次
无差别的大扫除，而且要重新认证 9 道与本议题无关的门禁。

需要按"产出方是否可能超过管道缓冲"分类。初判的候选（**单一路径判断，未复测**）：

| 站点 | 产出方 | 为何可疑 |
|---|---|---|
| `test-agent-driver-production-parity.sh:266` | `sqlite3 "$DB" .dump` | 整库 dump，尺寸随 fixture 增长 |
| `test-persistence-extraction.sh:233` | `cargo tree -p "$MEMBER"` | 完整依赖树，本仓库 443 行量级 |
| `test-jq-status-observed.sh` ×9、`test-fixture-target-drift.sh` ×2 | `scan`（Ruby 扫描器） | 通过路径输出很小，但失败路径逐条打印 finding |

`test-jq-status-observed.sh:195` 有一段注释明确论证了 `scan | grep -q` 是**有意为
之**——作者推理的是**扫描器**的退出码，没有推理**管道写入方**的。这正说明这个形状
不是疏忽，是一个反直觉点。

> **复核 C——"42 处 / 9 道门禁"实为 35 处可执行站点 / 7 道门禁。**
>
> 42 在 `1272d6c9` 上按原文方法可复现，而那个方法是 `grep -c`——数的是**行，包括注释**。
> 今日 39 个匹配行里有 **4 行是描述该形状的注释**，其中一行正是 FR-133 为解释这次修复
> 写下的说明：
>
> | 文件 | 匹配行 | 其中注释 | 可执行 |
> |---|---|---|---|
> | `test-agent-driver-documentation-alignment.sh` | 1 | 1 | 0 |
> | `test-coverage-governance-mainpath.sh` | 1 | 1 | 0 |
> | `test-fixture-target-drift.sh` | 2 | 1 | 1 |
> | `test-jq-status-observed.sh` | 12 | 1 | 11 |
> | `test-docs-publishing-integrity.sh` | 11 | 0 | 11 |
> | `test-skill-mirror-integrity.sh` | 5 | 0 | 5 |
> | `test-markdown-link-integrity.sh` | 4 | 0 | 4 |
> | `test-persistence-extraction.sh` | 2 | 0 | 2 |
> | `test-agent-driver-production-parity.sh` | 1 | 0 | 1 |
>
> 这是 SKILL.md §4.4 shape 1——文本出现顶替事实本身——由一份专门讲断言强度的 FR 犯下。
> 护栏因此必须剥离注释与 here-document 正文；`scripts/lib/shell_lexer.rb` 已经为
> `jq-status-observed.rb` 做了这件事。
>
> 另：「全部处于 `set -o pipefail` 之下」对 ci-required 集合成立；全仓范围内
> `scripts/regression/scenarios/probe-{low-output,runtime-control}.sh` 两个文件**没有**
> 开 pipefail，今日免疫，而在有人补上 pipefail 的那天会静默获得这个缺陷。

> **复核 D——三个候选里有两个结构上不可能触发。**
>
> `test-jq-status-observed.sh ×9` 与 `test-fixture-target-drift.sh ×2` 中的十处走的是
>
> ```sh
> scan() { (cd "$CASE7" && ruby "scripts/qa/jq-status-observed.rb" 2>&1) || true; }
> ```
>
> `|| true` 让函数**无论子 shell 发生什么（含 SIGPIPE）都返回 0**，`pipefail` 根本
> 看不到产出方。真正暴露的只有两处裸子 shell（`jq-status-observed.sh:354`、
> `fixture-target-drift.sh:503`），产出方各约 1.2 KB。
>
> 第三个候选 `test-persistence-extraction.sh`：第 104 行产出方实测 **1694 字节**，
> 第 233 行 **5407 字节**——不是原文估计的"443 行量级"风险。但第 233 行是命中 ⇒ `fail`
> 的形状，即复核 B 说的失败方向"开"。

### 3. 一条能响的护栏

修完之后，`ci-required` 门禁里不应能再写回这个形状而无人知晓。可选形式：
在 `scripts/qa/jq-status-observed.rb` 或一道新扫描器里加一条规则，禁止
「`set -o pipefail` 文件中，把可能大于缓冲区的产出方接进 `-q` 消费方」。

**这条规则的难点是判定"可能大于缓冲区"，它不是文本属性。** 若无法可靠判定，退而求
其次的可测形状是：禁止 `printf`/`cat`/命令替换变量接入 `-q`，改用 here-string——
这是纯语法的、可被 fixture 触发的，而且 here-string 在任何尺寸下都正确。

> **复核 E——"可能大于缓冲区"不只是难判定，是不可判定，退而求其次的那条才是正解。**
>
> | 产出方 | 命中位置 | 触发 |
> |---|---|---|
> | 90 KB，131 行 | 第 59273 字节 | **8–13 / 400** |
> | 1 MB，单行 | 第 0 字节 | **0 / 200** |
>
> 尺寸不决定它——命中位置与行结构才决定，且 `rg` 与 BSD `grep` 表现不同。
> 更要命的是，"这个产出方有界"是关于**今天的数据**的断言，没有任何东西复查它：
> CHANGELOG 是花了几年才越过 64 KB 的，parity fixture 的 dump 每加一个场景就长一次。
> 那正是 §4.4 shape 2 披着标注的外衣。
>
> 因此：**验收标准 1 的"写明为何其产出方有界"作废**，改为语法规则，且**不设豁免口**——
> here-string 形式永远可用且永远正确，一条留了豁免口的规则就是一条可以被安静放宽的规则
> （§4.4 shape 8）。

### 4. 护栏覆盖面按属性划定，而不是按清单

> **复核新增需求。** 原文把范围定在"ci-required 门禁"。该边界与危害无关：危害是
> 「pipefail + 短路消费方」，与一道门禁是否 ci-required 无关。而且
> `scripts/qa-doc-lint.sh` 与 `scripts/coverage-governance.sh` **由 `ci.yml` 执行却
> 不在 `qa-gate-surface.json` 里**（见复核 G），按 manifest 划范围会漏掉本次事故的
> 调用方本身。
>
> 覆盖面定义为：**全部 tracked `.sh` 中开启了 `pipefail` 的文件**——这是从文件自身
> 读出的属性，没有任何人需要维护的清单。实测 **61 处可执行站点 / 22 个文件**：
>
> | 类别 | 文件 | 站点 |
> |---|---|---|
> | ci-required 门禁 | 8 | 35 |
> | `qa-doc-lint.sh`、`coverage-governance.sh` | 2 | 3 |
> | manual-runbook 门禁 | 9 | 18 |
> | `scripts/regression/lib/probe-runner-lib.sh` | 1 | 4 |
> | `.claude/skills/tools/grpc-smoke.sh` | 1 | 1 |

### 5. 六处依赖分词的站点不能机械替换

> **复核新增需求。** 以下六处的 `printf '%s\n' $targets` 是**故意不加引号**的，
> 靠 IFS 分词把清单拆成每行一项，配合 `grep -qxF` 的整行匹配：
>
> - `test-docs-publishing-integrity.sh:483,597`
> - `test-markdown-link-integrity.sh:266,340`
> - `test-skill-mirror-integrity.sh:378,527`
>
> 若机械替换成 `<<< "$targets"`，清单会塌成一行，`-x` 从此不再匹配——检查**静默失效**，
> 门禁照常全绿。正确形式是 `<<< "$(printf '%s\n' $targets)"`。
> 这六处的通过数必须逐一前后对照。

## 验收标准

- [ ] ~~42 处已按"产出方尺寸可否超过 64 KB"分类，每一处要么改为 here-string / 捕获后
      判断，要么写明为何其产出方有界~~
      **（复核 E 作废）** → 全部 tracked `.sh` 中 pipefail 文件内的 `-q` 管道站点
      （61 处 / 22 文件）已改为 here-string 或捕获后判断，无一处以标注豁免
- [ ] 至少一处已证实会触发的站点，附上前后各 400 次满载复测的数字
- [ ] **机制本身有一条不依赖时序的断言**（复核：400 次循环在别人的机器上是抛硬币，
      本机 1 MB 产出方就测出 0/200）
- [ ] 护栏存在，且有一个**能让它响**的负向 fixture 与一个不该让它响的对照
- [ ] **护栏对注释、here-document 正文、未开 pipefail 的文件均不响**——这三者正是
      本 FR 自己数错的地方
- [ ] 受影响门禁全绿且**通过数不减少**（基线在各自改动提交前现测），六处分词站点逐一对照
- [ ] `cargo test --workspace`、strict Clippy、既有 CI job 全部通过

## QA 计划

- **复现**：以一个大于 64 KB 的产出方 + 靠前的命中点，在人为 CPU 负载下跑 400 次，
  断言假失败次数 > 0；改为 here-string 后断言为 0。这是唯一能证明修复有效的形状，
  因为空闲状态下两者都是 0/400。
- **护栏负向 fixture**：在一个 `set -o pipefail` 的探针脚本里写回该形状 → 门禁失败；
  改为 here-string → 通过。
- **对照**：一个产出方明显有界（如 `printf '%s\n' "$three_words"`）的站点不得被标记，
  否则规则会在抓到任何东西之前就被关掉。

> **复核——"唯一能证明修复有效的形状"不成立，而且这个 fixture 不该被提交。**
>
> 前提"空闲状态下两者都是 0/400"已被复核 A 推翻（空闲 8–13/400）。更重要的是：把
> 400 次概率循环提交进门禁，等于在别人的 runner 上抛硬币。把竞态**去掉**而不是**赛跑**：
>
> ```sh
> { printf 'MATCHME\n'; sleep 0.3; printf 'tail\n'; } | grep -q MATCHME       # 30/30 报失败
> grep -q MATCHME <<< "$( { printf 'MATCHME\n'; sleep 0.3; printf 'tail\n'; } )"  # 0/30
> ```
>
> 实测 **30/30 与 0/30**。它证明的正是本 FR 全部立论所依赖的那条性质——`pipefail` 把
> 被短路的产出方之死当作"没匹配上"交给调用方——且不依赖缓冲区尺寸、命中位置、
> `grep` 实现或机器负载。400 次的缓冲区测量作为**现场观测**留在 QA 文档里。

## 备注

发现于 FR-133 的认证扫描，记录在
`docs/qa/orchestrator/194-dependency-policy-gate.md` 与
`docs/design_doc/orchestrator/156-dependency-policy-gate.md`。FR-133 的改动让
`[Unreleased]` 区段增长了约 4 KB，因此**提高了**这个既有竞态的触发概率——但没有引入
它：`printf | rg -q` 的写法早于 FR-133，90 KB 的区段也早已超过缓冲区。

> **复核 G——一个本 FR 没看到的缺口，不在本 FR 范围内。**
>
> `scripts/qa-doc-lint.sh`（`ci.yml:222`）与 `scripts/coverage-governance.sh` 都由
> `ci.yml` 执行，却**都不在 `config/governance/qa-gate-surface.json` 里**。仓库里每一道
> 以 manifest 派生扫描范围的门禁——`jq-status-observed.rb`、`fixture-target-drift.rb`——
> 对这两个文件都是瞎的。
>
> 补进 manifest 会同时改变另外三道门禁的派生集合，属于独立改动。闭环时另立跟进项，
> 并记入 DD-157 的已知限制。
