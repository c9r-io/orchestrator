# FR-171: 四种资源可以写入但读不出来

## 优先级: P1

## 状态: Proposed（step 0 已完成，2026-08-17）

## 背景

计数 at `c6dbc5d7`。step 0 已对下表逐条重建，方法与更正记录在 §step 0 一节。

`ResourceKind`（`crates/orchestrator-config/src/cli_types.rs:146-171`）有 12 个成员
（两条独立路径确认：枚举变体扫描，以及 `check_resource_kind_catalog` 已门禁化的
指南列表），全部可以 `apply`（`core/src/resource/{parse,apply}.rs` 出现全部 12 个
`ResourceKind::` 变体）。读取面则不是：

| Kind | `apply` | `get`（列举） | `get kind/name` | `describe` | GUI 目录 | `manifest export` |
|---|---|---|---|---|---|---|
| Workspace / Agent / Workflow / StepTemplate / ExecutionProfile | ✓ | ✓ | ✓ | ✓ 类型化 | ✓ | ✓ |
| Trigger / SourceTaskTemplate / SourceTaskBinding | ✓ | ✓ | ✓ | ✓ 通用回落 | ✗ | ✓ |
| **Project / RuntimePolicy / EnvStore / SecretStore** | ✓ | ✗ | ✗ | ✗ | ✗ | ✓ |

坐标：`get_list_resource`（`core/src/service/resource/query.rs:219-246`）8 种；
`get_single_resource`（同文件 `:~270`）8 种；`describe_builtin_resource`（`:357`）
5 种类型化臂，其余 `_ => return Ok(None)`，由 `describe_resource`（`:339`）回落到
`get_resource` → `get_single_resource`；GUI 的 `list_resource_summaries`（`:402`，
match 臂 `:415-441`）5 种且对第 6 种**硬失败**。

**缺口是四种 kind，不是七个。** 四者在 `get` 的两条路径上都被 `is_builtin_kind`
（`core/src/crd/resolve.rs:63-79`，含全部 12 种）挡在 CRD 回落之外，落到
`unknown list resource type` / `unknown resource type`。

三条相关事实，都改变了论据的强度：

- **词汇已经预留，只是没有消费者。** `is_builtin_alias`（同文件 `:83`）保留了
  `project`/`projects`、`runtimepolicy`/`runtime-policy`、`envstore`/`envstores`、
  `secretstore`/`secretstores` —— CRD 不得占用这些名字。系统已经为四者留好了
  复数名，只是没有任何读取入口消费它。这使「补齐」比「裁决不补」更接近既有意图。
- **同一个条件有三条不同诊断**：CLI 列举说 `unknown list resource type`，
  CLI 单查说 `unknown resource type`，GUI 说 `unsupported expert resource catalog type`。
- **`manifest export` 覆盖全部 12 种**（`core/src/resource/export.rs:241-251`，
  并有断言 Project/RuntimePolicy/EnvStore/SecretStore 在场的测试），但它没有
  kind 过滤参数（CLI 面只有 `--output`），所以它是转储而不是查询。

四者中 **Project 的后果最重**，因为它是资源模型的第一层：
`02-resource-model.md:228` 定义 Project 为隔离域，所有资源命令接受
`--project`，但没有任何查询入口能回答「这台机器上现在有哪些 project」。
而 `CLAUDE.md` 的删库禁令正把「看起来只能删库」判定为隔离机制缺失的信号。
可枚举性是隔离机制可用性的下限。

## step 0：未survive 的断言与新增事实

**一条更正，改变了工作范围。** 原表的 `describe` 列对
Trigger / SourceTaskTemplate / SourceTaskBinding 记为 ✗，依据是
`describe_builtin_resource` 只有 5 个臂。该函数对其余 kind 返回 `Ok(None)`
而非错误，调用方 `describe_resource` 随即回落到 `get_resource`，那里这三种
是支持的。**`describe` 实际覆盖 8 种。** 原文的「七个缺口（4 + 3）」应为
**四个缺口**，需求 1 的工作量相应缩小；三者的残留问题不是「不支持」，而是
渲染路径不同（见下）。这条错误的形状是把一个函数的臂数当成了一个命令的能力。

**三条 survive 但需要限定的**：五种类型化 kind 的 `describe` 走
`RegisteredResource::to_yaml()`，另三种走 `get_single_resource`，后者又先查
`resource_store`（带 labels/annotations）再回落到内存配置（不带）。所以同一
命令有三条渲染路径，选哪条取决于 kind 与该资源是否在 store 里。这影响全部
8 种而不只是那 3 种，是否属于本 FR 由需求 1 裁决。

**两条原「未核验项」已关闭**：

- *别名是否存在只在某一入口可用的形式* —— 否。`get_list_resource` 收复数与单数，
  `get_single_resource` 只收单数，这是列举/单查的刻意划分，不是缺陷。
- *是否存在 apply 之外的其他枚举来源* —— 除 `manifest export` 外没有。
  gRPC 122 个 RPC 中与四者相关的只有 workflow store 的 `StoreList`（不同概念）；
  `debug` 的 9 个叶命令全是 sandbox 探针；GUI 的 Tauri 命令只有 `secret_key_*`
  密钥操作。

**一处排除**：`get_single_resource` 第二个 match 的 `_ => unreachable!()` 经检查
确实不可达（第一个 match 已对全部非 8 种提前返回），不是 panic 风险。

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

### 1. 四个不可读 kind 各给一条裁决

Project / RuntimePolicy / EnvStore / SecretStore 各一条：补齐 `get` 的两条路径，
或写明为什么这个 kind 不需要被查询。预期分歧最大的是 SecretStore —— 列举 store 名
与「不泄露值」并不冲突（`config_load/persist.rs:39-44` 已有 SecretStore `data`
占位符替换的先例，导出路径上值本来就被替换掉），所以「因为敏感所以不可列举」
不是充分理由，需要更强的论据或者补齐。

Project 按补齐处理：step 0 已证明不存在 `manifest export` 之外的枚举来源，
而 `is_builtin_alias` 早已为它预留了 `project`/`projects`。

附带裁决（可以是「不做，理由 X」）：`describe` 的三条渲染路径是否收敛。
它影响全部 8 种可读 kind，不只是本 FR 的四个缺口，所以也可以判为另起 FR。

### 2. 派生式门禁，而非清单式

四个入口（`get_list_resource`、`get_single_resource`、`describe_builtin_resource`、
`list_resource_summaries`）的支持集合必须**从 `ResourceKind` 枚举派生**后再与裁决
集合比对，新增 kind 若未落进任一裁决则失败并点名自己。理由与
`check_resource_kind_catalog`（DD-182）相同：手写清单只守住写它的那天。

裁决集合写死、支持集合派生，组合失败关闭 —— 与 FR-168 的
block-and-report 同形。

门禁必须**读 match 臂而不是 grep kind 名**：step 0 的更正正是因为把函数臂数
当成命令能力，而一个数臂数的门禁会把同一个错误固化下来。支持集合的正确定义
是「从命令入口可达」，`describe` 的回落必须计入。

### 3. GUI 目录的硬失败改为可解释

`ResourceSummaryPage` 目前对第 6 种 kind 返回 `unsupported ... catalog type`，
这是把「产品尚未裁决」呈现为「输入错误」。裁决落地后，未支持的 kind 应当
要么不出现在 GUI 的可选集合里（由派生集合驱动），要么给出裁决理由。

## 验收标准

- [ ] 四个缺口各有书面裁决；补齐的部分，列举与单查行为一致（不得出现
      「能列举但单查报不存在」），且每个补齐的 kind 有一条行为断言而非只有计数
- [ ] 门禁从枚举派生支持集合，且支持集合按**入口可达**而非按 match 臂计算；
      两个方向各有 fixture（枚举新增 kind / 裁决集合漏项），两种诊断可区分
- [ ] 存在一个 fixture 直接钉住 step 0 更正的那条：把 `describe_builtin_resource`
      的 `_ => Ok(None)` 改成返回错误，门禁必须变红。若不红，门禁数的就是臂数
- [ ] `02-resource-model.md` 的 Project 一节写明如何列举（或写明为何不能）
- [ ] 三条诊断（`unknown list resource type` / `unknown resource type` /
      `unsupported expert resource catalog type`）收敛或各自写明为何不同
- [ ] GUI 目录不再对已裁决的 kind 硬失败
- [ ] 概念预算净增为零：不新增 kind，不新增顶级命令组（`get`/`describe` 扩容
      是既有命令的参数扩展，按 DD-182 的规则无需自证）

## 依赖与关联

- DD-182（概念预算与 `check_resource_kind_catalog` 的派生式先例）
- FR-168 / DD-184（block-and-report 形状）
- `CLAUDE.md` 删库禁令 —— 本 FR 是它所指「应当提供的隔离机制」的一部分，
  但**不是全部**：可枚举性不等于可清理性，project 级删除是独立问题。

## 未核验项（明确标注）

step 0 关闭了原先的前两条（别名无入口间不对称；除 `manifest export` 外无其他
枚举来源）。仍然未核验的：

- **四者的 `resource_store` 落盘形状未逐一确认。** 补齐 `get` 需要
  `resource_store.get_namespaced(kind, project, name)` 对这四种返回结果，
  而 step 0 只证明了 `apply` 会走到它们的分发臂，没有确认四者都以同样的
  namespaced key 落盘。若某一种不是，补齐它的成本高于其余三种。
- **Project 的「列举什么」尚未定义，且两个候选集合是否会发散未证实。**
  存在两个候选：`config.projects` 这个 map 的键，与已 apply 的 `kind: Project`
  资源。step 0 确认 `resource/helpers.rs:118-127` 在 apply 任意资源时只计算一个
  `project_id` 用于 namespaced 落盘，**没有**写入 `config.projects`；但没能在
  合理成本内定位 `config.projects` 的生产构建路径（`build.rs` 从
  `resolve_and_validate_projects` 取，其上游未追到），所以「用 `--project foo`
  apply 一个 workspace 会不会让 foo 出现在 `config.projects` 里」仍是未知。
  这个问题必须在实现前回答：若两集合可发散，`get projects` 返回哪一个是产品
  裁决而不是实现细节，两个答案给出不同的行数。
- Project 补齐后的删除语义不在本 FR 范围内 —— 删除一个 project 意味着什么
  （级联？拒绝？）是 FR-168 同类的裁决问题，另起。
- 无遥测，四个 kind 的实际使用频率未知；优先级排序以模型层级而非用量为据。
