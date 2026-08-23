# FR-176: fixture 语料库的范围是列举出来的，不是导出来的 —— 四个目录 34 份清单没有任何测试解析过

## 优先级: P2

## 状态: Proposed

## 背景

FR-148 建立了一条正确的机制：`core/src/fixture_corpus_tests.rs` 把 fixture 语料库
逐个喂给 `validate_manifests`，与 `config/governance/fixture-bundle-validity.json`
台账比对，且**拒绝必须匹配其声明的诊断**，而不只是「拒绝了」。它的文件头注释自己
写着为什么范围要导出而非列举：

> **Scope is derived, never listed.** The corpus comes from `git ls-files`,
> so a bundle added tomorrow is in scope tomorrow.

但它导出的是**一个写死目录里的文件清单**，不是**清单文件本身**：

```rust
// core/src/fixture_corpus_tests.rs:31
const BUNDLE_GLOB: &str = "fixtures/manifests/bundles/*.yaml";
```

`git ls-files` 的这个 pathspec 使「明天新增的 bundle」进入范围，却使「明天新增的
**目录**」不进入。这不是实现偏离设计 —— DD-158 就是这么设计的。缺的是**范围本身
的导出规则**：一份 `apiVersion: orchestrator.dev/v2` 清单是否被治理，取决于它落在
哪个目录，而没有任何东西声明过哪些目录该被治理。

这正是 §4.4 shape 7（「列举式靶标会在靶标移动的那天变瞎」）应用在**范围**而不是
**靶标**上的形态。

## 实测清单（非枚举，方法与修订版本如下）

**方法**：`git ls-files '*.yaml' '*.yml'`，过滤出内容含 `apiVersion: orchestrator.dev/v2`
的文件，逐个跑 `orchestrator manifest validate -f <file>`。daemon 由
`scripts/lib/gate_daemon.sh` 起停，`ORCHESTRATORD_DATA_DIR` 指向 `mktemp -d`，
未触碰 `~/.orchestratord`，跑完 `pgrep orchestratord` 为空。

**修订版本**：`764b93de`（工单开具时的 HEAD）。

**复验（2026-08-23，`778b587a`）**：该基线在 33 个提交之后依然成立。目录分布未变
（`fixtures/manifests/bundles` 48 / `docs/workflow` 14 / `fixtures/benchmarks` 11 /
`crates/integration-tests/tests/common/manifests` 6 / `fixtures/workflow` 3），
治理外仍是 34 份；被拒数从 15 降到 **12**，差额正是下文「已完成的部分」修好的那三份
用户可见模板，它们现在都被接受。剩下 12 份的构成不变：10 份合法依赖形态、
2 份 `fixtures/workflow/` 的真腐烂。方法同上，隔离 daemon，跑完无泄漏。

全仓 82 个受追踪 v2 清单文件，分布在 5 个目录。语料库 glob 覆盖 1 个目录 / 48 个文件。
**34 个文件在治理之外，其中 15 个被产品拒绝。**

| 目录 | 文件数 | 被拒 |
|---|---|---|
| `fixtures/manifests/bundles`（已治理） | 48 | — |
| `docs/workflow` | 14 | 6 |
| `fixtures/benchmarks` | 11 | 6 |
| `fixtures/workflow` | 3 | 3 |
| `crates/integration-tests/tests/common/manifests` | 6 | 0 |

15 条拒绝里，**10 条是合法的依赖形态** —— 缺同目录的 `ExecutionProfile`、
`SecretStore`，或缺一个提供 capability 的 Agent。台账的 `Status` 枚举已经有现成词汇
（`fragment` / `environment` / `dependent`）表达它们，它们需要的只是**被声明**。

**5 条是真的烂掉了**：

| 文件 | 诊断 | 性质 |
|---|---|---|
| `docs/workflow/fr-watch.yaml` | `[parse_error] missing field 'type'` | 从未可用 |
| `docs/workflow/scheduled-scan.yaml` | `[parse_error] missing field 'type'` | 从未可用 |
| `docs/workflow/hello-world.yaml` | `[CODE_REPO_QA_TARGETS_REQUIRED]` | 从未可用 |
| `fixtures/workflow/self-bootstrap.yaml` | `[parse_error] unknown field 'captures'` | 曾可用，已退役 |
| `fixtures/workflow/self-evolution.yaml` | `[parse_error] unknown variant 'generate_items'` | 曾可用，已退役 |

前三条已在本 FR 的来源工单中修复（见「已完成的部分」），后两条留给本 FR 裁决。

## 三条被 parse error 掩盖的更深层腐烂

这一条值得单独写下，因为它决定了实施者该怎么估工。

`docs/workflow/fr-watch.yaml` 与 `scheduled-scan.yaml` 的 `missing field 'type'` 是
**第一层**。补上 `type:` 之后，产品报出了此前根本没机会报的下一层：

- `fr-watch.yaml`：`loop.guard enabled but no builtin loop_guard step or agent with
  loop_guard capability found`（`loop.mode: fixed` + `guard.enabled` 默认为真，
  见 `core/src/config_load/validate/loop_policy.rs:29-37`）
- `scheduled-scan.yaml`：`unknown field 'goal'`（`TriggerActionSpec` 只接受
  `workflow` / `workspace` / `args` / `start`，`cli_types.rs:755-769`，且带
  `deny_unknown_fields`）；此外 `concurrency_policy` 拼写错误，清单键是
  `concurrencyPolicy`（`cli_types.rs:566-571`）

**一个文件的第一个错误会把它后面的所有错误无限期地藏起来。** 这与 FR-131 的记录同形
（「一个 job 因某一原因常红，会把第二个原因无限期地藏起来」）。实施者给这 34 个文件
定状态时，不能假设「一条诊断 = 一个缺陷」—— 每修一条都要重跑。

## 谁本该发现，为什么没发现

`scripts/qa/test-agent-driver-production-parity.sh:100-102` **点名读取**
`docs/workflow/fr-watch.yaml` 与 `scheduled-scan.yaml`：

```ruby
"scheduled-scan" => ["docs/workflow/scheduled-scan.yaml", "scan-agent", "scheduled"],
"fr-watch" => ["docs/workflow/fr-watch.yaml", "fr-governance-agent", "fr-watch"]
```

它用 Ruby 的 `YAML.load_stream` 读，比对 `spec.command` 字符串与
`spec.driver.provider`。Ruby 能解析这两个文件 —— 它们是合法 YAML；**不合法的是清单**。
于是这道门禁在两个文件产品根本无法解析的整个期间**一直是绿的**。

这是 §4.4「代理不能是唯一的检查」的教科书形态：用「YAML 能读出这个字段」代理
「产品能接受这份清单」。加宽 glob 不会修好这道门禁，但会让它的盲区第一次可见 ——
语料库会独立地对同两个文件给出判定。

## 已完成的部分（来源工单中已修，本 FR 不重复）

来源工单的 ticket-fix 已修复三份**用户可见**的模板并逐个实测通过：

- `docs/workflow/fr-watch.yaml` —— 两个 step 补 `type:`；`loop` 改 `mode: once`
- `docs/workflow/scheduled-scan.yaml` —— 同上补 `type:`、改 `mode: once`；
  Trigger 去掉 `goal:`、`concurrency_policy` 改为 `concurrencyPolicy`；
  Workspace 补齐 `qa_targets` / `ticket_dir`
- `docs/workflow/hello-world.yaml` —— Workspace 补齐 `qa_targets` / `ticket_dir`，
  `root_path` 改为规范的 `work_dir`；`greet` step 显式声明 `scope: task`

三者现均 `Manifest is valid`。`hello-world.yaml` 还端到端跑通了它自己文件头印着的
两条命令（apply + task create），实测 items 解析为 **1 个**而非每份 QA 文档一个 ——
step 默认是 item scope，这正是 `scope: task` 那一行要挡住的。

**这三处修复目前没有任何东西守着。** 本 FR 的 glob 加宽就是它们的门禁。

## 需求

1. **语料库范围改为导出。** 把 `BUNDLE_GLOB` 换成对 `git ls-files` 全量 YAML 的扫描，
   按**文件内容**过滤而不是路径前缀。明天新增的目录因此明天就在范围内。既有的空扫描
   失败断言（`fixture_corpus_tests.rs:219-222`）必须保留并覆盖新范围。

   **谓词更正（治理 step 0，2026-08-23）。** 本条原写「内容含
   `apiVersion: orchestrator.dev/v2`」，那是错的，且错在本文档没预见的方向：
   `fixtures/manifests/bundles/crd-test-invalid.yaml` 用的是
   `apiVersion: extensions.orchestrator.dev/v1`（CRD 扩展资源 `PromptLibrary`），
   它**当前在**语料库内、且台账里有一条 `dependent` 声明。按原谓词它会**掉出**范围，
   声明随之变成孤儿，门禁报 `declaration names a path outside the corpus`。

   正确谓词是 `^apiVersion:` 且值含 **`orchestrator.dev/`**：

   | 谓词 | bundles | 新纳入 | 合计 |
   |---|---|---|---|
   | 原文 `orchestrator.dev/v2` | 48（**掉 1**） | 34 | 82 |
   | 更正后 `orchestrator.dev/` | **49** | 34 | **83** |

   全树 `apiVersion` 只有四种取值：`orchestrator.dev/v2`（449）、
   `extensions.orchestrator.dev/v1`（2）、`apps/v1`（2）、`v1`（2）。后两者是
   `project-bootstrap` 模板里的 Kubernetes 清单，必须排除 —— 这也是不能退而用
   「含 `apiVersion:`」的原因。**两端都要点名**：放宽到能接住 CRD 扩展，同时不接住
   Kubernetes。这是 §4.4 shape 10 的形状 —— 修好 under-reach 会开出 over-reach，
   只有同时写下两端才是对的。
2. **34 个文件各自获得一条判定**：被接受，或在台账里有一条带 `expect` 诊断与
   `reason` 的声明。`Status` 枚举已够用，不需要新增变体 —— 若实施者认为需要，
   要写下为什么现有五个不够。
3. **`rotted_count` 棘轮覆盖加宽后的集合。** 它当前是 0；加宽后必须等于实际
   `rotted` 条数，且只能向下走。
4. **不要照抄本文档的状态。** 每个文件的状态由实施者从测试自身的环境
   （`validate_manifests` + `TestState::without_seeded_agents_and_workflows`）
   重新测得。本文档的数字来自 CLI + 全新 daemon，两者不保证一致 —— 一条抄来而非
   测来的状态，正是台账要防的那种失效。

## 验收标准（由工单复现步骤导出）

1. 每一个受追踪的 `apiVersion: orchestrator.dev/v2` 文件要么被 `validate_manifests`
   接受，要么在 `fixture-bundle-validity.json` 中有声明。范围由索引导出，断言其为空
   时失败。
2. 被拒绝的文件，其真实诊断必须命中声明的 `expect` 之一 —— 沿用既有语义，
   退出码不算证据。
3. 三份已修模板（`fr-watch` / `scheduled-scan` / `hello-world`）在新范围内被判为
   **接受**。把它们改回破损形态会让语料库变红 —— 这是本 FR 唯一能证明门禁真的在守
   这三处修复的断言。
4. `rotted_count` 与实际 `rotted` 条数相等；把任一 `rotted` 条目删掉而不改计数会失败。
5. 一条负向 fixture：向**新纳入**的某个目录（而非 `fixtures/manifests/bundles/`）
   注入一份带退役构造的文档，语料库将其报为 `undeclared rejection`。既有的
   `an_injected_retired_construct_is_rejected_by_its_own_diagnostic` 从
   *accepted 且未声明* 的文件里导出靶标，加宽后要确认它挑中的仍是有意义的靶标。

## 待裁决：`fixtures/workflow/` 这三份分叉要不要留

来源工单问「这两份蓝图还要不要」。查清了，答案比工单假设的简单：

`fixtures/workflow/` 的三个文件是 `docs/workflow/` 同名文件的**分叉副本**，由
`71f8bf3b`（2026-03-15）出于一个很窄的理由创建 —— `ticket_dir: fixtures/ticket`
而非 `docs/ticket`，好让 QA 跑动时不往生产工单目录里写。此后生产版本历经迁移
（FR-156 的 `store_inputs`、FR-118/DD-137 的协调工具、FR-173 的退役），**分叉从未同步**。

工单把「step 如何在引擎不解析 stdout 的前提下发布 items 和分数」列为未决的设计决策。
**它已经决定并且已经上线了**，就在生产副本里：

- `docs/workflow/self-evolution.yaml:47` —— `Call mcp__orch__generate_items once with replace=true`
- 同文件 `:109` —— `Call mcp__orch__record_metric with name 'score' and the numeric total_score`
- 同文件 `:315` —— `metric_var: score` 原样保留

这条路径在代码里是通的，实测非推测：`item_executor/apply.rs:412-427` 把
`record_metric` 的回执写进 `pipeline_vars.signals.metrics`，
`loop_engine/segment.rs:717` 收集它，`item_select.rs:80` 按它排序。

因此两个选项，本 FR 倾向前者，但留给治理裁决：

- **删除三份分叉**，并为那两条引用它们的 QA 场景另找 `ticket_dir` 隔离手段
  （生产副本承载着流程本身，分叉已经数周不可用）。
- **保留并纳入加宽后的 glob**，使其不能再悄悄漂移 —— 代价是每次生产副本变更都要
  同步一次分叉，而过去五个月的记录表明这件事不会发生。

注：`fixtures/workflow/full-qa.yaml` 也在被拒之列（`unknown execution profile 'host'`）。
工单称它「不受影响」—— 对退役构造成立，对可用性不成立。这个目录是三个文件三个被拒，
不是工单说的两个。

## 未核验 / 开放问题

- **加宽的代价未测量。** 34 个文件里有 10 个需要新写声明（含 `expect` 与 `reason`）。
  这是实打实的工作量，本 FR 未估时，也未核验这 10 条声明写下来之后是否稳定 ——
  例如 `fixtures/benchmarks/` 那 5 个 Agent 依赖的 store 是否总是由同目录的
  `secrets-*.yaml` 提供。**单方法、未核验。**
- **`docs/workflow/self-evolution.yaml` 的 `environment` 判定依赖一个不在索引里的文件。**
  它引用 `fromRef: minimax`，由 `docs/workflow/minimax-secret.yaml` 提供，而后者被
  `.gitignore:13` 忽略 —— 是开发者本地创建的密钥文件。这使该文件天然是 `environment`
  而非腐烂，但也意味着**语料库永远无法接受它**。台账能表达这个状态；本 FR 未核验
  是否还有别的文件依赖被忽略的同类文件。
- **`crates/integration-tests/tests/common/manifests` 当前 6 个全绿**，纳入范围后
  没有立即成本。但它是测试夹具目录，纳入后新增夹具都要带声明，这个约束是否合意
  未与使用者确认。
- **本 FR 不修 `test-agent-driver-production-parity.sh` 的代理问题。** 加宽 glob 使其
  盲区可见，不使其消失。那道门禁是否该改成向产品要判定，是另一个议题。

## 来源

`docs/ticket/fixtures-workflow_ungoverned-dead-blueprints_260818_071500.md`
（2026-08-18 开于 FR-173 退役清扫，同日经 ticket-fix 实测复现后裁定为功能缺口并
转入本 FR；工单的三条前提在复现中被更正，见上文）。
