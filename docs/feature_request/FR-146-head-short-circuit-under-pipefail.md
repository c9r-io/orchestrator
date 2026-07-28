# FR-146: `producer | head -N` 在 `pipefail` 下会让门禁中途终止，而截断的运行读起来和完整的一模一样

## 优先级: P2

## 状态: Proposed

## 背景

FR-145 关掉了 `producer | grep/rg -q` 这一形状：`-q` 一命中就退出，产出方还在写，
死于 EPIPE，`set -o pipefail` 把它交给调用方，于是一次成功的匹配报成失败——或者，
在命中通向失败分支的地方，一次真实违规报成干净。

`head` 是同一个动作，但后果不同，而且更难看见。

`head -N` 读满 N 行就退出，产出方随即 EPIPE。绝大多数站点的写法是**诊断输出**：

```sh
fail "control: the repository does not pass its own scanner"
scan | head -5 >&2
```

这一行的退出码没有任何人读——但 `set -e` 读。实测（`cae30e41`，macOS）：

```sh
#!/usr/bin/env bash
set -euo pipefail
echo "before"
{ printf 'line1\n'; sleep 0.3; printf 'line2\n'; } | head -1 >&2
echo "after"
```

输出 `before`、`line1`，**`after` 不打印，脚本退出 141**。

也就是说：一道门禁在打印失败诊断的那一刻**中途终止**，剩下的断言一条都没跑，
汇总行也没打印。SKILL.md §4.4 shape 7 已经记录过这句话——"被截断的运行读起来和
完整的运行一模一样"——这里是它的一个系统性来源。

**与 FR-145 的关系**：同一机制，不同后果面。FR-145 的站点全部在条件位置，退出码
被读，所以表现为断言反转；`head` 的站点大多不在条件位置，退出码只被 `set -e` 读，
所以表现为静默截断。两者不能用同一条修复覆盖：条件位置的修法是 here-string，
诊断位置的修法是别让它进 `set -e` 的视线（`|| true`、捕获后 `head`、或改用
`sed -n '1,5p'`，后者读到 EOF）。

## 测量（`cae30e41`）

`| head` 在开启 `pipefail` 的 tracked `.sh` 中共 **38 处 / 29 个文件**
（方法：`grep -nE '\|[[:space:]]*head\b'`，over `git ls-files '*.sh'` 中
`set -...o... pipefail` 的文件）。按后果分：

| 形状 | 处数 | 后果 |
|---|---|---|
| `... \| head -N >&2` 诊断，退出码只被 `set -e` 读 | 8 | **中途终止**，汇总行不打印 |
| 条件位置（`if` / `&&` / `\|\|`） | 4 | 断言反转，同 FR-145 |
| 其余（赋值、`$( ... \| head )` 等） | 26 | 需逐类判定，**未验证** |

「其余」26 处未分类，是本 FR 要做的第一件事，不是可以跳过的一栏。

## 目标

- 把 `| head` 从开启 pipefail 的 tracked shell 里清掉，或对每一处写明它为何不会
  把产出方留在写入状态。
- 把护栏扩到 `head`：`scripts/qa/pipefail-short-circuit.rb` 已有的规则形状可以直接
  容纳它，`READERS` 与短路判据是现成的。

## 非目标

- **不**改 `set -e` 或 `set -o pipefail`。
- **不**在本 FR 内处理 `| tee`、`| read`、进程替换。

## 需求

### 1. 逐处分类那 26 个未验证的站点

按「退出码被谁读」分类，而不是按产出方尺寸——FR-145 已经证明尺寸不可判定
（90 KB 产出方触发 2–3%，1 MB 产出方 0/200，决定因素是命中位置与行结构）。

### 2. 诊断位置与条件位置分别修

- 条件位置：捕获后 here-string，同 FR-145。
- 诊断位置：`sed -n '1,5p'`（读到 EOF，无竞态）或捕获后再截。`|| true` 能止血，
  但它把产出方的**真实**失败也一起吞掉，属于 FR-144 那一类混淆。

### 3. 护栏扩到 head

`pipefail-short-circuit.rb` 的规则是"pipefail 文件里，第一段之后的管道阶段不得是
短路读取方"。`head` 加进 `READERS` 即可；负向 fixture 需要新增两个：一个诊断位置
（断言脚本中途终止），一个条件位置。

## 验收标准

- [ ] 38 处已按"退出码被谁读"分类，每一处要么修掉，要么写明为何其产出方不会被留在
      写入状态
- [ ] `pipefail-short-circuit.rb` 覆盖 `head`，且有能让它响的负向 fixture 与对照
- [ ] 有一条**不依赖时序**的断言证明诊断位置的 `| head` 会终止脚本（形如 FR-145
      的 case 16：产出方 `sleep` 后再写，构造上必然还在写）
- [ ] 受影响门禁全绿且**通过数不减少**（基线在各自改动提交前现测）

## 备注

发现于 FR-145 的治理。FR-145 的非目标第 3 条要求先测量再决定是否合并处理；测量结果
是 38 处 / 29 文件，与 FR-145 自身规模相当，因此分开。设计记录见
`docs/design_doc/orchestrator/157-pipefail-short-circuit.md` 的已知限制一节。
