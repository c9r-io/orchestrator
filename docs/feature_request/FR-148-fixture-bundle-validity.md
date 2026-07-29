# FR-148: 没有任何东西检查 fixture 是否还能被产品接受

## 优先级: P2

## 状态: Proposed

## 背景

2026-07-29，`scripts/qa/test-coordination-collapse.sh` 被发现自 07-25 起就跑不完：
它 apply 的 fixture 里有一个用 `behavior.captures` 的 workflow，而那个构造在 07-25
（`1b0937ca`，DD-137）按设计被移除，守护进程从此拒绝它。

**这个缺陷不需要门禁跑起来才能发现。** fixture 里写着 `behavior.captures`，校验器
拒绝 `behavior.captures`——只是没有任何东西把两者对上。

四天里没有人看见，因为那道门禁是 `manual-runbook`，没有 CI job 看着它，而它唯一的
症状是**汇总行不打印**（`apply` 对整个 bundle 全有或全无，一个被拒的 workflow 把
整份 fixture 带走，十二条断言只跑了三条）。修票时还顺带查出**同一个 commit 退役的
第二条断言**：`normalize_preserved_channels` 把四个残留通道搬进了类型化载体，断言
还在查旧形状。

## 目标

- 让「fixture 声明的东西产品是否还接受」成为一个每次 CI 都回答的问题。
- 让"故意无效"的 fixture 成为一个**写明理由的声明集合**，而不是没人认识的孤儿。

## 非目标

- **不**试图在 CI 里跑那 33 道 `manual-runbook` 门禁。它们之所以 manual，是因为要
  守护进程、端口、浏览器、provider；治理预算余量只有 **9%（247 秒 / 2700 秒）**，
  而这些门禁单个就是分钟级。
- **不**做"上次跑绿时间"的存活性台账。没人跑就没人写记录，33 条会同时长期过期，
  警报常亮然后被关掉——这个仓库已经反复记录过这种失效方式。
- **不**处理门禁 shell 本体里的断言腐烂。见下方「本 FR 盖不住的那一半」。

## 事实复核（`1b6628eb`，2026-07-30，治理 Phase 2 step 0）

下面的「测量」小节写于 `8a3ee0d9`。逐条重建后，**第 1、2、3 条成立，第 4 条不成立**，
而整份文档最重要的遗漏是：它把语料的腐烂规模说小了一个数量级。

### 已确认成立

- 93 个 bundle（`git ls-files 'fixtures/manifests/bundles/*.yaml' | wc -l`，`1b6628eb`）。
- `manifest validate` 走 gRPC 需要守护进程（`crates/cli/src/commands/manifest.rs:13-21`）。
- `validate_manifests`（`core/src/service/system.rs:250`）同步、公开，正是
  `crates/daemon/src/server/system.rs:317-327` 调用的那一个。
- 孤儿 49 / 被引用 44（`8a3ee0d9`，逐 basename `git grep -l -F` 重算，与原文一致）。
  在 `1b6628eb` 读作 46/47，差额三个是 **本 FR 文档自己点名了它们**；本 FR 闭环删除后回到 49。
- `scripts/qa-doc-lint.sh` 的通配消费属实，行号是 **64–66**（原文写 67）。

### 不成立：「四个故意无效的 bundle」

以空 base（见下）逐个调用 `validate_manifests`，`1b6628eb`：

| 原文断言 | 实测 |
|---|---|
| `qa105-s1-capture-wrong-level.yaml` 无效 | **通过校验**。step 级的 `capture:` 是未知键，被静默忽略——产品根本不拒绝它，它不是无效 fixture |
| `crd-test-invalid.yaml` 无效（理由：名字这么说） | 无效，但**理由不是它想测的那个**：单独校验命中 `no CustomResourceDefinition found for kind 'PromptLibrary'`，它设计上的「缺 `prompts` 必填字段」要先 apply `crd-test.yaml` 才够得着。它是**依赖型** fixture，不是自足的无效 fixture |
| （未提及） | `coordination-strangler-parity.yaml` 是**第五个**故意无效的 bundle，而且是四个里唯一有活消费者主张这一点的：`scripts/qa/test-coordination-strangler.sh:126-133`（**ci-required**）断言它被拒且诊断为 `[legacy_(coordination\|json_path)_removed]` |

### 遗漏：93 个里有 31 个当前无法通过校验

不是 4 个，是 **31 个**（62 通过 / 31 失败，空 base，`1b6628eb`）。按病因：

| 类 | 数量 | 说明 |
|---|---|---|
| **DD-137 已退役的构造** | **19** | `behavior.captures` 9 个（stagger×4、qa105-s3、qa105-s5、qa107-s1、s5-pipeline-var、coordination-legacy-baseline）；`generate_items` JSONPath post-action 10 个（qa83×5、wp05×3、cycle-overflow-test、generate-items-narrow-test） |
| 片段 bundle（只给 Workflow，不给 Agent） | 5 | fr052×2、template-test、simple-prehook-test、s4-invalid-cel |
| 环境/基线依赖 | 4 | crd-test、test-workspace、test-workspace-dryrun（work_dir 不存在）、non-code-workspace-fixture（需 `fileSharing.shareableRoots`） |
| 依赖型 | 1 | crd-test-invalid（需先 apply crd-test） |
| schema 腐烂 | 1 | prehook-test：`missing field \`when\`` |
| 故意无效且有消费者主张 | 1 | coordination-strangler-parity |

### 新发现：坏掉的门禁不止一道

`scripts/qa/test-wp05-integration.sh:250,282,312` 对 **三个** bundle 执行整份
`orchestrator apply -f`，而这三个都被 `[legacy_json_path_removed]` 拒绝。也就是说
它和触发本 FR 的 `test-coordination-collapse.sh` 是**同一个病、同一个 commit
（`1b0937ca`，DD-137，07-25）**，只是没人发现。它同样是 `manual-runbook`。

`GenerateItems` / `SpawnTasks` post-action 在 `core/src/config_load/validate/workflow_steps.rs:63-74`
被整类拒绝——所以这不是「fixture 写错了」，是**这些 fixture 测的功能已经不存在**。
连带四份 QA 文档至今 `lifecycle: active` 却在描述被移除的机制：
`docs/qa/orchestrator/51-primitive-composition.md`、`83-generate-items-mixed-text-extraction.md`、
`84-generate-items-regression-narrowing.md`、`92-dynamic-items-cycle-overflow.md`。

### 必须写进设计的：校验结果依赖 base，也依赖环境

`validate_manifests` 把 bundle 合并进**当前 config** 再 `build_active_config_for_project`。
所以「产品是否接受这个 fixture」不是 fixture 单独的属性。实测三种 base：

| base | 通过/失败 | 假象 |
|---|---|---|
| `TestState::new()` 原样（含 `default` workspace + `echo` agent + `basic` workflow） | 60/33 | **5 个假失败**：bundle 引入 self-referential workspace 后，被拖下水的是**测试脚手架自己的 `basic` workflow**（`[SELF_REF_POLICY_VIOLATION] ... workflow 'basic'`），与 fixture 无关 |
| 清空 agents+workflows（保留 `default` workspace） | 62/31 | self-ref 假失败消失；5 个片段 bundle 转为失败——这是**真实属性**（空守护进程确实不接受它们） |

选后者，并把 base 写进设计记录。另：同一个 bundle 有多个错误时，`errors[0]` 取决于
HashMap 迭代顺序（实测 `cycle-overflow-test.yaml` 两次跑出不同的 workflow 名），
断言必须匹配**诊断子串**，不能依赖第一条错误。

单个 `TestState` 复用给 93 次校验后总耗时 **1.77s**（每个 bundle 新建一个是 24.34s）——
`validate_manifests` 只读 state，可以共享。

## 测量（全部在 `8a3ee0d9`）

### 1. CLI 不能作为机制——它是控制面调用

```
$ ./target/debug/orchestrator manifest validate -f <任意 bundle>
Error: daemon socket not found at ~/.orchestratord/orchestrator.sock
```

**93 个 tracked bundle 全部如此**（方法：`git ls-files 'fixtures/manifests/bundles/*.yaml'`
逐个调用）。`manifest validate` 走 gRPC（`crates/cli/src/commands/manifest.rs:16` →
`ManifestValidateRequest`），需要守护进程。这不是 fixture 的问题，是"为什么机制不能是 CLI"。

### 2. 但离线入口是现成的，而且已经有人这么用

`core/src/service/system.rs:250`：

```rust
pub fn validate_manifests(state: &InnerState, content: &str, project_id: Option<&str>)
    -> Result<ManifestValidationReport>
```

**同步、公开，正是守护进程收到 `ManifestValidate` 时调用的那一个**
（`crates/daemon/src/server/system.rs:323`）。而 `core/src/service/system.rs:670` 的既有单测
`validate_manifests_handles_parse_valid_and_invalid_config` 已经用
`TestState::new()` + `fixture.build()` 构造出 `InnerState` 并直接调用它——**机制已验证，不是假设**。
`core/tests/integration_test.rs:560` 也已经在从磁盘读 bundle。

因此这条检查跑在**既有的 `Rust test` job** 里：不新增治理步骤，不占那 9% 余量。

### 3. 93 个 bundle 里 49 个没有任何消费者

方法：把 93 个 basename 做成模式表，`git grep -o -F -f` 扫全部 tracked 文件（排除
`fixtures/` 自身），`8a3ee0d9`。44 个被引用，**49 个没有**。

> **单一路径判断。** 用 basename 精确匹配，所以以路径片段或通配符引用的 bundle 会被
> 误判成孤儿。已知一个反例：`scripts/qa-doc-lint.sh:67` 用
> `fixtures/manifests/bundles/*.yaml` 通配来推导已知 workflow ID 集合，**所有 93 个都在
> 那条通配之下**。也就是说它们不是作为 fixture 被消费，但确实喂着一个交叉引用检查——
> 增删 bundle 会改变那个集合，这一点在实现时必须顾及。

**超过一半的 fixture 语料没有消费者**，意味着它们腐烂时没有任何东西会注意到，而校验是
唯一可能注意到的东西。

### 4. 四个故意无效的 bundle，其中三个是孤儿

| 文件 | 为何无效 | 谁引用 |
|---|---|---|
| `coordination-legacy-baseline.yaml` | `behavior.captures`，本次拆分出来 | `test-coordination-collapse.sh`（读文本 + 断言其被拒） |
| `crd-test-invalid.yaml` | 名字说它无效 | **无** |
| `s4-invalid-cel.yaml` | 名字说它无效 | **无** |
| `qa105-s1-capture-wrong-level.yaml` | 名字说它无效 | **无** |

后三个没有任何脚本或文档引用，也没有任何地方写明它们**应当**无效。这正是本 FR 要
把它们变成声明集合的原因——和 FR-133 的 `deny.toml` 里 70 条"每条写明来源"是同一个形状。

## 需求

### 1. 一条走 tracked 集合的校验检查

一个 cargo test（或 `core/tests/` 下的集成测试）遍历
`git ls-files 'fixtures/manifests/bundles/*.yaml'`，逐个读入并调用
`validate_manifests`。范围**从 git 派生，不写清单**：明天加的 bundle 明天就在范围内。

### 2. 无效必须是被声明的，且带理由

不能通过校验的 bundle 必须出现在一份声明里，**每条带一句为什么它必须无效**。
声明缺失 → 失败；声明里的条目实际上能通过校验 → 也失败（否则理由会烂在那儿，
就像 FR-133 的 `unmatched-skip` 那半边）。

### 3. 负向 fixture

- 往一个正常 bundle 里塞一个被退役的构造（如 `behavior.captures`）→ 检查必须失败并
  **点名那个诊断**，不能只判非零。理由与 `test-coordination-collapse.sh` 里那条相同：
  能力校验发生在 captures 校验**之前**，缺 agent 的清单会以 `no agent supports capability`
  失败，光看退出码分不开这两者（该点已在本次修票中实测）。
- 一个声明为无效、实际却能通过校验的条目 → 必须失败。
- 对照：未改动的树必须通过。

### 4. 顾及 `qa-doc-lint.sh` 的通配消费

`scripts/qa-doc-lint.sh:67` 从 `bundles/*.yaml` 推导已知 workflow ID。实现时必须确认
增删 bundle 不会让那道检查产生假阳性或假阴性。

## 验收标准

- [ ] 校验检查存在，范围由 `git ls-files` 派生，跑在既有 `Rust test` job 内（新增治理
      步骤数 = 0，预算占用 = 0）
- [ ] 93 个 bundle 全部要么通过校验，要么在声明里带一句理由
- [ ] 四个已知无效的 bundle 各有写明的理由
- [ ] 三条负向 fixture 各能被触发，且"塞入退役构造"那条断言的是**诊断**而非退出码
- [ ] `scripts/qa-doc-lint.sh` 的 workflow ID 交叉引用不受影响（通过数不减少）
- [ ] `cargo test --workspace`、strict Clippy、既有 CI job 全部通过

## 本 FR 盖不住的那一半

**只抓 fixture 腐烂。** 门禁 shell 本体里的断言腐烂抓不到——本次修票找到的第二条就是
例子：`normalize_preserved_channels` 把 `goal` 与三个 sandbox 信号搬进类型化载体，
断言还在查通用变量表里的旧形状。那不是任何静态检查能看出来的。

这一条明写在这里，而不是留给读者推断，因为把 FR 起名成「manual 门禁存活性」会让人
以为两半都盖住了。贵的那一半仍然没有答案，本 FR 不假装有。

## 备注

发现于 `docs/ticket/coordination_collapse_scenario_legacy_apply_260729_000000.md` 的
修复（该票分类为误报，已闭环删除；票文件按 `.gitignore` 惯例不入库，实质记录在
`docs/qa/orchestrator/168-coordination-collapse-mcp-tools.md` 与提交 `8a3ee0d9`）。

同期立项的还有 FR-146（`| head` 在 pipefail 下的同类机制）与 FR-147（三个由 `ci.yml`
执行却不在执行面清单里的脚本）。三者互不依赖。
