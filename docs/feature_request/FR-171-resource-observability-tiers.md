# FR-171: 资源可观测面的三级断层 —— 12 种可 apply，8 种可 list，5 种可 describe

## 优先级: P1

## 状态: Proposed

## 背景

计数 at `c6dbc5d7`，方法：从 `ResourceKind` 枚举逐项对照三个读取入口的 match 臂。
单一派生，未经 step 0 重建。

`ResourceKind`（`crates/orchestrator-config/src/cli_types.rs:146-171`）有 12 个成员，
全部可以 `apply`。读取面则逐级收窄：

| Kind | `apply` | `get`（列举） | `describe` | GUI Expert→Resources |
|---|---|---|---|---|
| Workspace / Agent / Workflow / StepTemplate / ExecutionProfile | ✓ | ✓ | ✓ | ✓ |
| Trigger / SourceTaskTemplate / SourceTaskBinding | ✓ | ✓ | ✗ | ✗ |
| **Project / RuntimePolicy / EnvStore / SecretStore** | ✓ | ✗ | ✗ | ✗ |

坐标：`get` 的臂在 `core/src/service/resource/query.rs:219-246`（8 种）；
`describe_builtin_resource` 的臂为 5 种；GUI 的 `ResourceSummaryPage` 目录
（同文件 `:415-441`）同样是 5 种，且对第 6 种**硬失败**并返回
`unsupported expert resource catalog type`。

四个不可列举的 kind 里，**Project 的后果最重**，因为它是资源模型的第一层：
`02-resource-model.md:228-239` 定义 Project 为隔离域，所有资源命令接受
`--project`，但没有任何入口能回答「这台机器上现在有哪些 project」。操作员
只能从别处反推 —— 而 `CLAUDE.md` 的删库禁令正把「看起来只能删库」判定为
隔离机制缺失的信号。可枚举性是隔离机制可用性的下限。

SecretStore 有 `orchestrator secret key` 六个叶命令，但那是**密钥**操作，
不列举 store 本身。`manifest export` 是四者共同的绕行路径，它导出全部资源
而非按 kind 查询。

本 FR 不主张四者都该补 —— 逐项裁决，**书面理由即为合法产出**（FR-160 需求 4 先例）。

## 需求

### 1. 逐 kind 裁决读取面

七个缺口（4 个不可列举 + 3 个不可 describe）各给一条裁决：补齐、或写明为什么
这个 kind 不需要该入口。预期分歧最大的是 SecretStore —— 列举 store 名与
「不泄露值」并不冲突（`config_load/persist.rs:40` 已有占位符替换的先例），
所以「因为敏感所以不列举」不是充分理由，需要更强的论据或者补齐。

Project 建议按补齐处理，除非有 apply 之外的枚举来源被证明存在。

### 2. 派生式门禁，而非清单式

三个入口的支持集合必须**从 `ResourceKind` 枚举派生**后再与裁决集合比对，
新增 kind 若未落进任一裁决则失败并点名自己。理由与
`check_resource_kind_catalog`（DD-182）相同：手写清单只守住写它的那天。

裁决集合写死、支持集合派生，组合失败关闭 —— 与 FR-168 的
block-and-report 同形。

### 3. GUI 目录的硬失败改为可解释

`ResourceSummaryPage` 目前对第 6 种 kind 返回 `unsupported ... catalog type`，
这是把「产品尚未裁决」呈现为「输入错误」。裁决落地后，未支持的 kind 应当
要么不出现在 GUI 的可选集合里（由派生集合驱动），要么给出裁决理由。

## 验收标准

- [ ] 七个缺口各有书面裁决；补齐的部分三个入口行为一致（列举与 describe 不得
      出现「能列举但 describe 报不存在」）
- [ ] 门禁从枚举派生支持集合，两个方向各有 fixture（枚举新增 kind / 裁决集合
      漏项），两种诊断可区分
- [ ] `02-resource-model.md` 的 Project 一节写明如何列举（或写明为何不能）
- [ ] GUI 目录不再对已裁决的 kind 硬失败
- [ ] 概念预算净增为零：不新增 kind，不新增顶级命令组（`get`/`describe` 扩容
      是既有命令的参数扩展，按 DD-182 的规则无需自证）

## 依赖与关联

- DD-182（概念预算与 `check_resource_kind_catalog` 的派生式先例）
- FR-168 / DD-184（block-and-report 形状）
- `CLAUDE.md` 删库禁令 —— 本 FR 是它所指「应当提供的隔离机制」的一部分，
  但**不是全部**：可枚举性不等于可清理性，project 级删除是独立问题。

## 未核验项（明确标注）

- 上表由单次派生得出，未经 step 0 重建。三个 match 臂的别名集合（如
  `"ws"`/`"workspace"`/`"workspaces"`）未逐一核对是否存在只在某一入口
  可用的别名。
- 四个不可列举 kind 是否存在**除 apply 外**的其他枚举来源（gRPC 直连、
  `debug` 子命令、`manifest export` 之外）未穷举。若存在，缺口小于本文所述。
- Project 补齐后的删除语义未在本 FR 范围内 —— 删除一个 project 意味着什么
  （级联？拒绝？）是 FR-168 同类的裁决问题，另起。
- 无遥测，四个 kind 的实际使用频率未知；优先级排序以模型层级而非用量为据。
