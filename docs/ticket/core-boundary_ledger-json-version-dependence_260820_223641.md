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

## 曾经「未解释」的部分 —— 已查明，是我自己的测量错误

本文档初版写道：同样的输入下 main 绿而 PR #131 红，无法解释。**那个矛盾不存在**，
它来自我的一处误测，记录在此因为它正是 §4.4 shape 6 的形状。

我当时断言「两边解释器相同」，依据是两次 run 的 apt 输出都写着
`ruby is already the newest version (1:3.2~ubuntu1)`。但那是 **apt 包版本**，
而出问题的是 **json gem 版本** —— 一个随 runner 镜像滚动更新、apt 包版本却不变的量。
main 那次 PASS，所以诊断从未打印过它的 json 版本，我把「没测到」当成了「相同」。

时间戳挑明了这一点：

| run | 时间 | 结果 |
|---|---|---|
| main governance 重跑（job 95939493389） | 2026-08-19 **02:44** | 14 passed, 0 failed |
| PR #131（job 96436169466） | 2026-08-20 **13:07** | 12 passed, 2 failed |

相差 34 小时，期间 ubuntu-latest 镜像更新。**在当前镜像上重跑 main 的同一个 job：
core-boundary 与 persistence-api-boundary 同样失败，diff 逐字节相同（`11,12c11`）。**

所以这不是分支特有的问题，而是镜像更新触发的全仓缺陷，main 早已中招，只是在这次重跑
之前没有人碰过它。缺陷 1 的机制就是全部原因。

教训是可迁移的：**一个字段回答的不是你问的问题**。`1:3.2~ubuntu1` 回答「apt 包是哪个」，
被拿去当「`JSON.pretty_generate` 如何渲染空对象」的答案。判断两个环境「相同」之前，
要测的是那个真正决定行为的量，而不是它旁边那个容易读到的量。

## 修复（已实施）

1. **`ledger_json` 版本稳定化**（`scripts/lib/rust_source.rb`）：正则从
   `\{\n\s*\n\s*\}`（只收带空行的形式）放宽为 `\{\n\s*\}`，三种渲染
   （`{}`、`{\n  }`、`{\n\n  }`）统一收成紧凑形式，数组同理。
   **刻意不用 `\{\s*\}`** —— JSON 字符串字面量里不可能有真换行（会转义成 `\n` 两字符），
   要求换行保证它只改结构；宽松形式会静默改写 `reason` 散文里的 `{ }`。
2. **两个 ledger 重新生成**并提交为规范化后的形式。
3. **新增 Case 8b**（`test-core-boundary.sh`）直接断言这条性质：三种空容器渲染都规范化、
   且散文里的 `{ }` 不被改动。这条与 json 版本无关 —— Case 2 做不到，因为它拿本机的 emit
   和本机写的 ledger 比，在任何单台机器上都绿，只在跨版本时才红。
4. 该 case 经 `fixture_produce` 派生输入，`fixture-target-drift.rb` 的告警是对的：
   手写的 `if ...; then` 只断言了命令成功，没断言产出非空。

**缺陷 2（`--write` 改写 ledger）的触发条件随之消失** —— emit 恢复与 ledger 一致后
Case 7 重新是 no-op。但「只读门禁在失败路径写入受版本控制的文件」这个形状本身未加固：
若将来 emit 再次与 ledger 分歧，它还会写。留作后续项，见下。

## 遗留项（未做，非阻塞）

`test-core-boundary.sh` Case 7 在真实仓库上跑 `--write`，因此它的失败路径会改写
`config/governance/core-boundary-ledger.json`。当前 emit == ledger，所以是 no-op；
但正确的形状应当是在 scratch tree 上跑，或在失败路径上恢复。本次不做，因为它需要重排
Case 7 的断言语义（它现在测的正是「no-op 写入不改变字节」这一性质本身）。

## 影响

- FR-174 的 PR #131 因此为红，且该红与 FR-174 的改动无关（PR 未合并，等待本 ticket）
- 任何在 json ≥ 2.21 环境上运行这两个门禁的人都会遇到，并会得到一棵被改写的工作树
