# FR-145: `producer | consumer -q` 在 `pipefail` 下是一个按负载触发的假失败

## 优先级: P2

## 状态: Proposed

## 背景

2026-07-28，FR-133 的认证扫描（41 道派生门禁连续跑）中，`scripts/qa-doc-lint.sh`
报出一条**不存在的缺陷**：

```
FAIL: CHANGELOG [Unreleased] does not name the removed runner selection seam
```

而 `RunnerExecutorKind` 就在 `CHANGELOG.md` 第 74 行，稳稳落在 `[Unreleased]`
区段（第 8–131 行）之内。单独重跑该门禁 10 次（前台、后台、裸跑、`--fixture-test`）
全部通过。

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

## 目标

- 把这一形状从 `ci-required` 门禁里清掉，或对每一处写明它为何不可能触发。
- 留下一条能被触发的护栏，否则下一个人写回同样的管道时没有任何东西会响。

## 非目标

- **不**全局禁止 `|`。绝大多数管道的消费方读到 EOF 才退出，不存在这个竞态。
- **不**改 `set -o pipefail`。它抓到的真实缺陷远多于这一处假阳性。
- **不**在本 FR 内处理 `head -n1`、`| tee`、进程替换等其它短路消费方，除非测量显示
  它们也在 `ci-required` 面上（见下方"未验证"标注）。

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

### 3. 一条能响的护栏

修完之后，`ci-required` 门禁里不应能再写回这个形状而无人知晓。可选形式：
在 `scripts/qa/jq-status-observed.rb` 或一道新扫描器里加一条规则，禁止
「`set -o pipefail` 文件中，把可能大于缓冲区的产出方接进 `-q` 消费方」。

**这条规则的难点是判定"可能大于缓冲区"，它不是文本属性。** 若无法可靠判定，退而求
其次的可测形状是：禁止 `printf`/`cat`/命令替换变量接入 `-q`，改用 here-string——
这是纯语法的、可被 fixture 触发的，而且 here-string 在任何尺寸下都正确。

## 验收标准

- [ ] 42 处已按"产出方尺寸可否超过 64 KB"分类，每一处要么改为 here-string / 捕获后
      判断，要么写明为何其产出方有界
- [ ] 至少一处已证实会触发的站点，附上前后各 400 次满载复测的数字
- [ ] 护栏存在，且有一个**能让它响**的负向 fixture 与一个不该让它响的对照
- [ ] 受影响门禁全绿且**通过数不减少**（基线在各自改动提交前现测）
- [ ] `cargo test --workspace`、strict Clippy、既有 CI job 全部通过

## QA 计划

- **复现**：以一个大于 64 KB 的产出方 + 靠前的命中点，在人为 CPU 负载下跑 400 次，
  断言假失败次数 > 0；改为 here-string 后断言为 0。这是唯一能证明修复有效的形状，
  因为空闲状态下两者都是 0/400。
- **护栏负向 fixture**：在一个 `set -o pipefail` 的探针脚本里写回该形状 → 门禁失败；
  改为 here-string → 通过。
- **对照**：一个产出方明显有界（如 `printf '%s\n' "$three_words"`）的站点不得被标记，
  否则规则会在抓到任何东西之前就被关掉。

## 备注

发现于 FR-133 的认证扫描，记录在
`docs/qa/orchestrator/194-dependency-policy-gate.md` 与
`docs/design_doc/orchestrator/156-dependency-policy-gate.md`。FR-133 的改动让
`[Unreleased]` 区段增长了约 4 KB，因此**提高了**这个既有竞态的触发概率——但没有引入
它：`printf | rg -q` 的写法早于 FR-133，90 KB 的区段也早已超过缓冲区。
