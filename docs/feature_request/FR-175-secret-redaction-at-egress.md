# FR-175: 脱敏边界画在持久化时，而不是出网时 —— 两条已授权的读命令输出明文密钥

## 优先级: P2

## 状态: Proposed

## 背景

SecretStore 的值在静态存储上加密，在持久化快照里被替换为 `[ENCRYPTED]`。
但两条已授权的读命令把同一批值原样吐出来。

这不是实现偏离了设计。设计把脱敏范围写得很具体，而且是**有界的**：

- `docs/design_doc/orchestrator/17-envstore-secretstore-agent-env.md:132` ——
  值「encrypted at rest (AES-256-GCM-SIV) and redacted **in logs**」
- 同文档 `:140`、`:175` —— 「redacted **in task output logs**」
- 同文档 `:171` —— 「EnvStore and SecretStore resources can be applied,
  **exported**, and deleted via CLI」，export 是验收标准之一

没有任何一句说一条已授权的读命令要脱敏。实现忠实地照做了：加密写入、脱敏日志、
提供导出。缺的是**出网边界**这个概念本身。

`docs/design_doc/orchestrator/189-resource-observability-tiers.md:191-194`
（FR-171 的设计记录）已经把这条泄漏显式记为未处理的已知限制。本 FR 是它的去处。

## 泄漏清单（实测，非枚举）

**方法**：在 `e6081c6d` 上起一个隔离 daemon（`scripts/lib/gate_daemon.sh`，
`ORCHESTRATORD_DATA_DIR` 指向 `mktemp -d`，未触碰 `~/.orchestratord`），
apply 一个含真实值的 SecretStore，然后对每条读取路径的输出做 `grep` 实测。

| 出口 | 明文？ | 代码位置（`e6081c6d`） |
|---|---|---|
| `manifest export -o yaml` | **是** | `core/src/service/resource/mod.rs:527` |
| `manifest export -o json` | **是** | 同上，共用 `builtin_docs` |
| `debug --component config` | **是** | `core/src/service/system.rs:38` |
| GUI `manifest_export` Tauri 命令 | **是**（同一 RPC） | `crates/gui/src/commands/manifest.rs:73` |
| `get secretstore <name>` | 否 | `core/src/service/resource/query.rs:464` 脱敏 |
| `get secretstores` | 否 | 只回名字 |
| `debug --component state` / `--component dag` | 否 | 不含 config |
| 数据库静态存储 | 否 | AES-256-GCM-SIV |
| GUI `config_debug` 探测 | 否 | `crates/gui/src/commands/system.rs:66` 传 `component: None`，只回帮助文本 |

工单只记录了 `manifest export`。`debug --component config` 是本次补测发现的，
GUI 那条是第三个用户可达面。**只点名 export 会重复 §4.4 shape 2** —— 手列的清单
只守住写它时已知的那些，下一个落在清单外的实例会静默通过。

上表的「否」与「是」同等重要：一个把 SecretStore 整个从输出里漏掉的 bug，
会让「明文缺席」这条断言变绿。验收标准因此要求**占位符在场**，而不只是明文缺席。

## 两条泄漏同源，且仓库里已经有答案

两条路径都拿未脱敏的 `read_active_config(state).config` 直接序列化。

而写路径早就解决了同一个问题：`core/src/config_load/persist.rs:18`
的 `serialize_config_snapshot` 调用同文件 `:30` 的 `sanitized_config_snapshot`，
后者同时脱敏 typed 的 `project.secret_stores` **和** `resource_store` 里的
SecretStore 资源，然后跑 `export_manifest_resources(&sanitized)`。

也就是说，**持久化路径已经在产出一份形状完全相同的脱敏导出**。

实施者不要新写脱敏器。既有的那个被
`persist_raw_config_encrypts_secret_store_resources_and_redacts_snapshots`
（`core/src/config_load/persist.rs:399`）逐条守着，它当前是私有 `fn`，
需要放开可见性。新写一个意味着两份脱敏逻辑各自漂移，而只有一份有测试守着。

## 往返语义已裁决，不再是开放问题

工单把这个列为未决：脱敏后的导出还能 apply 回去吗？答案是**不能，而且是具名拒绝**。

`core/src/resource/secret_store.rs:43` 的 `SecretStoreResource::validate` 已在
FR-171 中合并：`spec.data` 的值等于 `[ENCRYPTED]` 时以
`[secret_value_placeholder_rejected]` 拒绝，并点名违规的 key。判定用等值而非
`contains`，所以一个真值里恰好嵌了这段文本不会被误伤。

因此脱敏导出**不会造成静默的数据损坏**，只会在 apply 时被拒。这是产品所有者
裁决「一律脱敏」的前提，也是这条 FR 得以只做减法的原因。

这条前提是**实测的，不是引用的** —— 在同一个隔离 daemon 上，把一份
`OPENAI_API_KEY: "[ENCRYPTED]"` 的清单 apply 回去：拒绝发生、诊断点名了
`OPENAI_API_KEY`、且已存储的值未被覆盖。一条 FR 的整个论证支在别人的改动上时，
那个改动值得亲自跑一遍。

**代价要写明**：一律脱敏后，没有任何路径能导出可还原的完整配置。
export 从「可还原的备份」变成「资源清单」。见下方开放问题。

## 需求

1. **`manifest export` 一律脱敏。** yaml 与 json 两种格式的 SecretStore
   `spec.data` 值均为 `[ENCRYPTED]`。无开关、无 flag —— 产品所有者已裁决不设
   `--reveal-secrets` 逃生口。
2. **`debug --component config` 一律脱敏。** 与需求 1 同源同解。
3. **复用 `sanitized_config_snapshot`，不新写。** 若因 crate 边界确实无法复用，
   FR 治理时要写下为什么，而不是默默复制一份。
4. **QA 场景覆盖导出的输出内容。** `docs/qa/orchestrator/37-envstore-secretstore-resources.md`
   的 Scope 已经写着 export，却没有任何场景检查导出输出里有什么 —— 这正是这条
   泄漏活了这么久还没人发现的原因。该文档已在 5 场景上限，所以要么替换一个场景，
   要么新建 QA 文档；这个取舍留给治理。

## 验收标准（由工单复现步骤导出）

每条都要求**占位符在场**，而不只是明文缺席：

1. apply 一个 `spec.data` 含真实值的 SecretStore 后，`manifest export -o yaml`
   的输出中该值缺席、`[ENCRYPTED]` 在场。
2. 同上，`-o json`。两种格式分别断言 —— 它们共用 `builtin_docs`，所以预期一致，
   但共用是实现细节，断言不该依赖它。
3. 同上，`debug --component config`。
4. `debug --component state` 与 `--component dag` 不因本次改动开始输出 config。
5. `get secretstore <name>` 与 `get secretstores` 保持现状（FR-171 已脱敏），
   不因本次改动回归。
6. 把脱敏后的导出 apply 回去，被 `[secret_value_placeholder_rejected]` 拒绝且
   点名 key —— 证明脱敏与拒绝这两半是配套的。
7. 端到端，非纯函数层：隔离 daemon + 真实 apply + 真实读取。工单最初就是因为
   只做了纯函数探测而把这一条列为「未核验」。

## 未核验 / 开放问题

- **备份能力的去向。** 一律脱敏后，`manifest export` 不再能还原一套完整配置。
  操作员是否需要一条独立的加密备份路径（导出密文而非占位符，配对一条恢复命令），
  本 FR 未评估，也未测量是否有人真的把 export 当备份用。
- **既有导出产物未评估。** 若此前有人把 `manifest export` 的输出存档、提交进
  git 或贴进 issue，那些文件里带着明文密钥。本 FR 不含任何补救建议的可行性评估，
  也没有清点这类产物是否存在。
- **非 CLI/GUI 出口未清点。** 上表覆盖 CLI 子命令、GUI Tauri 命令与数据库。
  事件负载、任务日志与 Slack/webhook 投影走的是另一套 `redaction_patterns`
  机制（DD-17:140、DD-127:142），本 FR 未验证其完整性。这是**单方法、未核验**。
- **EnvStore 未纳入。** EnvStore 按设计存放非敏感值，不加密也不脱敏。本 FR 不改
  这个边界，但也没有核验过是否有人在 EnvStore 里放了密钥 —— 若有，那是另一个问题。

## 来源

`docs/ticket/secretstore_manifest-export_scenario0_260817_224518.md`
（2026-08-17 开于 FR-171 step 0，2026-08-18 经 ticket-fix 端到端复现后裁定为
功能缺口并转入本 FR）。
