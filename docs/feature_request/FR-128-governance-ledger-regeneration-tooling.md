# FR-128: 治理台账再生与审阅工具 — 消除手工 SHA256 维护摩擦

## 优先级: P1

## 状态: Proposed

## 背景

`config/governance/coordination-collapse-ledger.json`（510 行）承载 FR-124/125/126 的协调坍缩与执行退役治理状态，其中包含一份 **20 条手工维护的生产 Agent 清单**，每条带 `manifestFingerprint`（对 `kind`/`metadata.name`/`spec` 规范化后的 SHA256）：

```json
{ "file": "...", "name": "...", "classification": "...",
  "migrationTarget": "...", "manifestFingerprint": "<sha256>" }
```

`scripts/qa/coordination-governance.rb` 对其做严格相等比对，不一致即报 `production Agent execution inventory differs from the reviewed ledger`。

**该脚本没有任何 `--fix` / `--emit` / regenerate 模式。**

后果：任何一次生产 Agent 的 spec 变更（哪怕只是改一个 `args` 元素）都会让门禁失败，而恢复绿灯的唯一途径是人手重算 SHA256 并编辑 JSON。摩擦是刻意设计的——它强制每次 Agent 变更经过 review。但**零工具化的强摩擦不会带来更好的 review，只会带来绕过**：实践中要么有人跳过门禁，要么把台账更新退化成"看到红就复制粘贴新哈希"的橡皮图章，两者都使 `reviewed ledger` 这个概念失效。

`sourceBaseline` 的四个单调棘轮计数（`capturesOrJsonPath: 55`、`pipelineVariables: 39`、`celInterpreter: 9`、`legacyRunnerSelection: 0`）同样为手工维护，存在相同问题。

## 目标

- 让台账更新从"人手算哈希"变成"工具生成候选 + 人审 diff + 显式接受"。
- 保留 review 语义：工具**只输出候选，不直接写入**被门禁读取的文件，接受动作必须是人的显式提交。
- 让 diff 可读——审阅者应看到"哪个 Agent 的 spec 变了、变在哪里"，而不是一串哈希差异。

## 非目标

- **不**放松门禁：比对仍是严格相等，工具的存在不得使不一致自动通过。
- **不**引入自动提交或 CI 自动修复台账——那会把 review gate 变成装饰。
- **不**改变 `manifestFingerprint` 的计算口径（规范化规则保持不变，否则历史指纹全部失效）。
- **不**扩大台账的治理范围（不新增被追踪的对象类别）。

## 需求

### 1. 清单再生子命令

- 为 `scripts/qa/coordination-governance.rb` 增加 `--emit-inventory` 模式：扫描生产 manifest，输出与台账 `productionAgents` 结构完全一致的 JSON 片段到 stdout。
- 输出必须与门禁比对时使用的排序、字段裁剪、规范化逻辑**共用同一段代码**，避免"生成的和校验的不是同一个东西"。
- 默认不写文件；写入由使用者通过 shell 重定向或显式 `--write` 完成，且 `--write` 在 CI 环境下（检测 `CI` 环境变量）拒绝执行。

### 2. 可读的失配报告

- 门禁失败时，除现有错误消息外，输出结构化 diff：新增的 Agent、消失的 Agent、指纹变化的 Agent 及其变化字段（`classification` / `migrationTarget` / spec 内容）。
- 对指纹变化，至少指出是 spec 的哪个顶层键发生变化，使审阅者不必自行重算。

### 3. 基线棘轮的再生与校验

- 为 `sourceBaseline` 的四个计数提供同样的 `--emit-baseline` 输出。
- 明确并在台账中记录每个计数的扫描口径（现有 `scope` 字段已描述，需保证工具实现与该描述一致），并由测试证明二者一致。

### 4. 审阅工作流文档化

- 在 `docs/guide/` 或治理 DD 中记录标准更新流程：改 Agent → 门禁失败 → `--emit-inventory` → 审阅 diff → 提交台账变更（与 Agent 变更同一 commit）。
- 明确"同一 commit"要求：台账更新与其对应的 spec 变更不得分离提交，否则中间态的历史修订处于门禁失败状态。

## 验收标准

- [ ] `--emit-inventory` 输出与门禁比对逻辑共用同一实现，且存在测试证明二者对同一输入产出一致
- [ ] `--write` 在 `CI` 环境变量存在时拒绝执行，并有测试覆盖
- [ ] 门禁失败时输出可读 diff（新增/消失/指纹变化 + 变化字段），以一次真实 spec 变更为证
- [ ] `--emit-baseline` 输出四个棘轮计数，且与台账 `sourceBaseline.scope` 描述的口径一致（有测试）
- [ ] 审阅工作流已文档化，包含"台账与 spec 同 commit"约束
- [ ] 负向验证：工具的存在不会使一个未经审阅的 Agent 变更自动通过门禁
- [ ] `cargo test --workspace`、strict Clippy、`scripts/qa/test-coordination-strangler.sh` 通过

## QA 计划

- **一致性测试**：对同一份 manifest 集合，分别运行比对路径与 `--emit-inventory`，断言输出逐字节相同。
- **CI 写保护测试**：以 `CI=1` 调用 `--write`，断言非零退出且未修改文件。
- **真实变更演练**：修改一个生产 Agent 的 spec，记录门禁失败输出、`--emit-inventory` 输出、以及恢复绿灯所需的最小编辑；验证 diff 报告准确指出了变化字段。恢复原状后门禁重新通过。
- **绕过防护**：构造一个"指纹更新了但 classification 未随之更新"的不一致台账，确认门禁仍然失败——证明工具没有削弱语义校验。
