# `core-boundary` 与 `persistence-api-boundary` 的字节比对依赖 json gem 版本，且失败时会改写受版本控制的 ledger

**Status**: open
**Found**: 2026-08-20，在 FR-174 的 PR #131 上（CI run 32372530924，head `f4e93f8c`）
**Severity**: high —— 其一使两个 ci-required 门禁的判定取决于运行者的解释器版本；
其二在门禁失败时把 `config/governance/core-boundary-ledger.json` 就地改写，污染工作树

## 一句话

`scripts/lib/rust_source.rb:299` 的 `ledger_json` 对空对象的渲染不是版本稳定的，
两个门禁却用 `cmp` 做字节比对；而当比对失败时，同一个门禁的 Case 7 会**真的写入**
那个 ledger。

## 缺陷 1：`ledger_json` 的输出依赖 json gem 版本

```ruby
def ledger_json(value)
  JSON.pretty_generate(value)
    .gsub(/\[\n\s*\n\s*\]/, "[]")
    .gsub(/\{\n\s*\n\s*\}/, "{}") + "\n"
end
```

那两个 `gsub` 收的是 `{\n\n}`（中间有**空行**）。而不同 json 版本渲染空对象的形式不同，
其中一种它收不掉：

| 环境 | ruby | json gem | `JSON.pretty_generate({"k"=>{}})` |
|---|---|---|---|
| GitHub ubuntu-24.04 runner | 3.2.3 | **2.21.2** | `"{\n  \"k\": {}\n}"` 紧凑 |
| 开发机 macOS | 2.6.10 | **2.1.0** | `"{\n  \"k\": {\n  }\n}"` 多行 |

已提交的 `config/governance/core-boundary-ledger.json` 是**多行**形式：

```
    "rusqlite": {
      "total": 0,
      "files": {
      }
    }
```

`rusqlite.files` 为空是常态（core 已完成抽取，引用数为 0），所以这个分歧一直存在，
只是没人看见——因为**看得见它的那条路径自己是坏的**（见缺陷 2 与「为什么一直没暴露」）。

消费方是字节比对，容不下渲染差异：

```bash
# scripts/qa/test-core-boundary.sh Case 2
if cmp -s "$WORK/emitted.json" "$REPO_ROOT/$LEDGER"; then
# scripts/qa/test-persistence-api-boundary.sh Case 2 同形
```

## 缺陷 2：门禁失败时改写了受版本控制的 ledger

同一次 CI run 的 Case 7 输出：

```
FAIL: CI=false was treated as unattended (exit 0) or the write was not a no-op
wrote config/governance/core-boundary-ledger.json; review the diff and commit it with the change that caused it
```

Case 7 断言「`CI=false` 时 `--write` 应当执行，且因为 ledger 已是最新所以是 no-op」。
当缺陷 1 使 emit ≠ ledger 时，这次写**不是 no-op**，于是门禁把工作树里的 ledger
改成了运行者那一版 json 的渲染形式。

一个只读门禁在失败路径上写入受版本控制的文件，会让后续步骤、后续门禁与
`git status` 检查读到一棵被污染的树。这与 [DD-155](../design_doc/orchestrator/155-fixture-target-drift.md)
「fixture 必须证明自己应用了变异」是同一类问题的另一面：这里是**门禁在不该写的时候写了**。

## 为什么一直没暴露

`test-core-boundary.sh` 与 `test-persistence-api-boundary.sh` 都以 `set -euo pipefail`
开头，而 Case 2 的失败诊断是：

```bash
diff "$REPO_ROOT/$LEDGER" "$WORK/emitted.json" | sed -n '1,20p' >&2
```

`diff` 在文件不同时返回 1 —— 那是这行唯一会被执行到的场景 —— pipeline 状态因此为 1，
`set -e` **当场终止脚本**。结果是：诊断不打印、Case 3–12 不运行、summary 不出现。
一次截断的运行，对任何读退出码的东西来说和一次完整的失败无法区分。

这正是那行上方注释里声称已修的 FR-146 缺陷：去掉 `| head` 修好了一半，`diff` 自身的
返回码仍然打到 `set -e`。**该缺陷已在 `f4e93f8c` 修复**（捕获状态而非 `|| true`，
0/1 放行、≥2 具名），Case 7 的问题是修好截断后第一次可见的。

## 复现

```bash
# 1. 看两种渲染
ruby -rjson -e 'puts Gem.loaded_specs["json"]&.version; p JSON.pretty_generate({"k"=>{}})'
#   json 2.1.0  -> "{\n  \"k\": {\n  }\n}"
#   json 2.21.2 -> "{\n  \"k\": {}\n}"

# 2. 看 ledger 用的是哪种
sed -n '9,13p' config/governance/core-boundary-ledger.json

# 3. 在 json >= 2.21 的环境上跑，Case 2 会失败，Case 7 会写掉 ledger
bash scripts/qa/test-core-boundary.sh; git status --porcelain
```

## 期望

- 两个门禁在任何受支持的 ruby / json 版本上给出相同判定；或者门禁显式声明并检查它所
  要求的 json 版本，而不是沉默地依赖它
- 门禁在任何路径上都不写入受版本控制的文件，除非那是它被明确要求做的事
- 修复不应是「用新版 json 重新生成 ledger 并提交」—— 那只是把分歧掉个方向，
  开发机上会立刻反过来红

## 建议的修法方向（未验证，供实现者取舍）

1. **让 `ledger_json` 版本稳定**：不要依赖 `JSON.pretty_generate` 对空容器的渲染，
   自己规范化（例如把两种形式都收成 `{}` / `[]`，即在现有 gsub 之外再收 `{\n\s*\}`）。
   这样两边都产出紧凑形式，然后**一次性重新生成两个 ledger 并提交**。
2. **比对语义而非字节**：解析后比较数据结构，字节形式只作为「建议的写法」。
   代价是失去「emit 可直接提交」这条性质，而 Case 2 的注释说那正是它存在的理由。
3. 缺陷 2 独立于以上：`--write` 的失败路径无论如何都不该留下改动。

## 未解释的部分（明确标注）

**同样的输入下，main 绿而 PR #131 红，这一点我没能解释。** 逐项核对过：

| 项 | 结论 |
|---|---|
| `core-boundary-ledger.json` | `origin/main..HEAD` 无差异（md5 `7b968e88…`） |
| `scripts/lib/rust_source.rb`（`ledger_json` 所在） | 无改动 |
| `core/`、`crates/` | 无改动 |
| `scripts/qa/core-boundary.rb` | 无改动 |
| 解释器 | 两边均 ruby 3.2.3 / json 2.21.2（实测打印） |
| main 今天重跑（run 32099510921，`e6081c6d`） | **14 passed, 0 failed** |
| PR #131（run 32372530924，`f4e93f8c`） | **12 passed, 2 failed** |

相同输入产生稳定相反的结果，逻辑上要求还存在一个未被发现的输入差异。上面缺陷 1 的
机制是**实测确证**的（版本与渲染形式都打印了出来），但它**不足以解释这个分叉** ——
如果它是全部原因，main 也该红。实现者应当把「找出那个差异」当作第一步，
而不是接受本文档已给出的机制就动手。

## 影响

- FR-174 的 PR #131 因此为红，且该红与 FR-174 的改动无关（PR 未合并，等待本 ticket）
- 任何在 json ≥ 2.21 环境上运行这两个门禁的人都会遇到，并会得到一棵被改写的工作树
