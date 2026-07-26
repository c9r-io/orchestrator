# FR-138: bash 3.2 兼容性扫描器的跨行词法状态与漏报面

## 优先级: P2

## 状态: Proposed

## 背景

FR-135 建立的 `scripts/qa/bash32-compat.rb` 扫描全部 95 个受追踪 shell 文件，覆盖面由
`git ls-files '*.sh'` 得出、无豁免清单，语义半边在真实 `/bin/bash 3.2` 下逐类**执行**验证。
其主张——`boundary-coverage` 的原始缺陷被消灭、全仓同类形态已清理——经独立变异复核成立：
把 `${branch_args[@]+"${branch_args[@]}"}` 改回裸展开，门禁立即在 `coverage-governance.sh:39`
报出；run `30182768742` 中该 job 在 77 个提交以来首次成功。

但**这把尺子本身有盲区**，且盲区无声。闭环后审计发现三处，全部潜伏。

### 缺陷 A：逃逸的 heredoc 静默截断扫描面

`Bash32Compat.code_lines` 逐行跟踪引号状态（`strip_comment` 在每一行开始时重置
`in_single` / `in_double`）。因此**跨行的单引号区域不被识别**，其内部文本按 shell 代码解读。
只要该区域含有形如 `<< WORD` 的片段，扫描器就认为开启了一个 heredoc，此后逐行丢弃直到出现
一行 strip 后等于 `WORD`——而那一行永远不会出现，于是**文件剩余部分全部退出扫描，且不产生
任何诊断**。

当前树上有两处正在生效：

| 文件 | 起始行 | 未扫描行数 | 触发物 |
|---|---|---|---|
| `scripts/qa/test-qa-gate-surface.sh` | 900 | 252 | `perl -e` 程序中的替换串含 `<<EOF` |
| `scripts/qa/test-bash32-compat.sh` | 369 | 16 | `ruby -e` 程序中的 `hosting << job_name` |

对照实验（`git archive HEAD` 出的干净副本上）：把 `arr=()` + 裸 `"${arr[@]}"` + `declare -A` +
`mapfile` 四行追加到 `test-qa-gate-surface.sh` 的被吞区域——

```
bash 3.2 compatibility: PASS (95 shell file(s) scanned, 0 finding(s))
```

同一段追加到 `scripts/lib/gate_preamble.sh`（无 heredoc 逃逸）——

```
scripts/lib/gate_preamble.sh:63: [empty-array-expansion] ...
scripts/lib/gate_preamble.sh:64: [associative-array] ...
scripts/lib/gate_preamble.sh:65: [mapfile] ...
bash 3.2 compatibility: FAIL (95 shell file(s) scanned, 3 finding(s))
```

**当前是潜伏的**：两处被吞尾部单独重扫均为 0 finding，没有真实危险构造被藏住。

值得记下的是第二处的触发行的身份。`test-bash32-compat.sh` 的 case 9 解析 workflow，断言
"CI 中确实存在跑本门禁的 macOS job，语义半边不会在所有宿主上被 skip"——这是整套设计里最
关键的一条防空转断言。它用 ruby 写成，而 ruby 的数组追加记号正是 `<<`。**证明门禁有 bash 3.2
宿主的那一行，就是让门禁看不见自己最后 16 行的那一行。** 嵌入 ruby 是本仓库 shell 包装器的
常规写法，因此这不是一次巧合，而是一条会反复触发的路径。

这与 FR-134 需求 9 在 Rust 侧刚消灭的缺陷同类：`strip_test_modules` 逐行统计花括号，字符串
字面量中的 `{` 使 `cfg(test)` 块永不闭合、其后生产代码从扫描中消失，由 `scripts/lib/rust_lexer.rb`
跨行维护字符串/字符/原始字符串/嵌套注释状态修复。FR-135 在七个提交之后，于 shell 侧重新
采用了逐行近似。DD-146 的 Known Limits 已写下"注释扫描器逐行跟踪引号"，但只推导到注释判定的
后果，未推导到 heredoc 判定的后果——后者的影响面大得多，且前者会误报（可见），后者会漏报
（不可见）。

### 缺陷 B：跨文件的可空数组是漏报

`emptyable_arrays` 逐文件判定：只有在**同一文件内**出现 `name=()` 或 `name=("$@")` 的数组名，
其值展开才会被检查。数组在被 `source` 的库中置空、在调用方展开的形态不会被发现。

复现（两文件均受追踪，且 `scripts/lib/` 下的文件正是被 source 的库）：

```
scripts/lib/gate_preamble.sh     + shared_args=()
scripts/lib/provider_isolation.sh + printf "%s\n" "${shared_args[@]}"
→ bash 3.2 compatibility: PASS (95 shell file(s) scanned, 0 finding(s))
```

DD-146 记录了这条逐文件规则的**过报**方向（"一个数组只要在文件任何位置被赋 `=()`，其全部值
展开都成为 finding，即使前面的守卫已证明非空"），未记录跨 `source` 边界的**漏报**方向。
两个方向来自同一条规则，但一个可见、一个不可见。

### 缺陷 C：`!` 不在命令位置的识别集内

`COMMAND_POSITION` 的关键字候选为 `if|then|else|elif|do|while|until|not`。`not` 不是 bash
关键字；bash 的取反记号是 `!`，而它不在候选内。

```
mapfile -t xs < /dev/null            → FAIL（正确命中）
if ! mapfile -t xs < /dev/null; then :; fi → PASS（漏报）
```

该候选集是 `3b5f9eb4`（"make the bash 3.2 rules match invocation, not mention"）为消除
**误报**而引入的，方向正确；缺的是取反这一种真实调用形态。

## 目标

- 让扫描器的覆盖面等于它声称扫描的文件，而不是"直到第一个 `<<` 形近物为止"。
- 让逐文件规则的漏报方向要么被消除，要么与已记录的过报方向同等可见。

## 非目标

- **不**改变 FR-135 确立的结构：静态扫描 + 真实 bash 3.2 下执行的 fixture 语料，两半互不
  替代。本 FR 只修静态半边的词法。
- **不**扩充七类危险构造之外的新类别。DD-146 已声明该清单非穷尽且"新一类到来时无人守护"，
  那是独立议题。
- **不**把 `.github/workflows/**` 的 `run:` 块纳入扫描面。DD-146 已如实声明其不被覆盖；
  2026-07-26 复核该面仍无七类构造（另查 `coproc`、`&>>`、`{a..b..step}`、`$'\u'`、
  `printf '%(...)T'`、`shopt -s lastpipe` 亦无）。要治理应单独立项，不混入本 FR。
- **不**引入完整 shell 解析器。所需的是跨行的引号与 heredoc 状态，与 `rust_lexer.rb` 同一
  量级，不是语法树。

## 需求

### 1. 跨行词法状态，且未闭合即失败

- `code_lines` 的引号状态须跨行保持，使跨行单引号 / 双引号区域内的文本不被当作代码解读。
- **文件读完时若仍处于 heredoc 中，该文件须报为 finding 而非静默通过。** 这是本需求里最
  便宜也最重要的一条：上述两处逃逸都会被它抓到，而一个真正未闭合的 heredoc 本就是坏脚本，
  两种情况都值得失败。
- 实现应复用 FR-134 为同类问题建立的做法（`scripts/lib/rust_lexer.rb` 的跨行状态机），
  或在设计记录中说明为何 shell 侧不适用。

### 2. 消除可空数组的跨文件漏报

推荐做法：**放弃 emptyable 推断**，对任何未写成规范安全形式的 `${name[@]}` / `${name[*]}`
值展开一律报出。理由：

- 一次消除跨文件漏报（缺陷 B）与 DD-146 已记录的流不敏感过报两个方向，规则从"推断"降为
  "匹配"，不再有可绕过的推断面。
- 与既有政策一致。DD-146 记录 `test-agent-driver-documentation-alignment.sh` 本可豁免却
  "照样重写了，因为守卫形式不花什么代价"——那已经是本条规则的实质立场。
- 代价已实测：当前树上有 **37 处未加守卫的值展开，分布在 14 个文件**，集中于
  `check-async-lock-governance.sh`(10)、`test-markdown-link-integrity.sh`(4)、
  `test-skill-mirror-integrity.sh`(4)、`test-docs-publishing-integrity.sh`(4)。改写是纯文本
  替换，无语义变化。
- **该 37 是下界**：它由当前带缺陷 A 的 `code_lines` 统计得出，两处被吞尾部未计入。因此
  需求 1 必须先落地，需求 2 的范围才是真实的。

若实现方选择保留推断并改为解析 `source` 图，须书面说明为何值得承担该复杂度，并给出
"库被条件 source"与"数组名被间接引用"两种形态的处置。

### 3. 取反形态的命令位置

- `COMMAND_POSITION` 须接受 `!`；`not` 若确非 bash 关键字应一并移除，避免留下一条永不命中的
  候选给读者以已覆盖的印象。
- 修正后须复核不引入新的误报：`3b5f9eb4` 引入该候选集正是为压制"提及即命中"。

### 4. 披露补齐

- DD-146 的 Known Limits 须补记本 FR 消除的与保留的边界。特别是：若需求 2 采用推荐做法，
  原"逐文件、流不敏感"一条应改写而非保留；若保留推断，则须显式补上漏报方向。

## 验收标准

- [ ] 负向 fixture：受追踪脚本中含跨行单引号区域内的 `<< WORD`，其后的危险构造仍被报出
- [ ] 负向 fixture：文件以未闭合 heredoc 结束 → 该文件被报为 finding，且诊断指明是未闭合
      heredoc 而非某一类构造
- [ ] 回归证据：`test-qa-gate-surface.sh` 与 `test-bash32-compat.sh` 的**全部**行进入扫描；
      以"被扫描行数 / 总行数"的逐文件统计作为机器可读证明，而非目测
- [ ] 负向 fixture：数组在 A 文件置空、在 B 文件裸展开 → 检查失败（或按需求 2 的替代路径，
      任何未加守卫的值展开均失败）
- [ ] 需求 2 落地后全仓无未加守卫的值展开，或残余点带理由记录
- [ ] 负向 fixture：`if ! mapfile ...` 被报出；且 `case 4` 既有的"提及不命中"断言仍通过
- [ ] `test-bash32-compat.sh` 在真实 `/bin/bash 3.2` 下全绿且 skip 数为 0（macOS 本机）
- [ ] `coverage-policy-fixtures` 的 ubuntu 与 macOS 两条腿均通过，其余 CI job 状态不变
- [ ] DD-146 的 Known Limits 已按需求 4 更新

## QA 计划

- **两类 fixture 各自独立**：逃逸 heredoc 与未闭合 heredoc 是不同断言。只做前者会让"读完
  仍在 heredoc 中"这条兜底无人验证，而它才是通用的那一条。
- **隔离断言**：沿用 FR-127 建立的约定——每条 fixture 必须失败于其目标规则，且其余规则在
  同一棵树上仍全部通过。
- **覆盖面的量化证明**：本 FR 的核心主张是"扫描面等于文件"，其证据必须是逐文件的行数统计，
  不能是"门禁通过"。当前缺陷正是在门禁通过的前提下发生的——沿用会通过的那种证据无法证伪它。
- **改前改后 finding 数不变**：需求 1、3 落地后，在**未做需求 2 的改写之前**，全仓 finding
  数应从 0 变为若干（缺陷 A 解除封印后新暴露的项）。若仍为 0，说明词法修复未生效，或两处
  逃逸的尾部确实无危险构造——须显式区分这两种解释，方式是同时报告被扫描行数的变化。
  这是 FR-130 "尺子先于测量"同一条判据的应用：修的是缺陷还是换了口径，靠双向数字区分。
- **不需要 CI 实证**：本 FR 不改变任何 job 的运行结果，证据完全在 fixture 与行数统计内。
  但 `coverage-policy-fixtures` 的 macOS 腿是语义半边的唯一宿主，仍须观察其真实结果。
