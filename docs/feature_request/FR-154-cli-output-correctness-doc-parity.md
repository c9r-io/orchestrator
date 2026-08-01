# FR-154: CLI 输出正确性与三文档面一致性

## 优先级: P2（需求 1 为 P1——是正确性 bug 而非风格问题）

## 状态: In Progress

## 背景

2026-08-01 审计（at `9bcfaa96`）；2026-08-02 治理时按 Phase 2 步骤 0 由三个独立子代理在 `98f28a8a` 重建全部事实主张，本节数字均为重建后的值（重建方法逐条标注）：

1. **同一命令不同格式返回不同数据（脚本化正确性 bug）**：
   - `crates/cli/src/output/task_list.rs` — `task list -o json` 含 `total_items`/`finished_items`/`failed_items`（json 臂 7 键,yaml 臂 4 键,逐键比对,yaml 恰好丢弃这三项）；
   - `crates/cli/src/output/mod.rs` `print_task_items` — `-o yaml` 丢弃 json 中的 `fix_required`/`fixed`/`last_error`/`started_at`/`completed_at` 五项（json 9 键 vs yaml 4 键,严格子集）；
   - `crates/cli/src/output/attention.rs:78-86` — `attention get`/`follow` 的 `-o table` 分支实际打印**紧凑** JSON（`to_string` 而非 `to_string_pretty`,双重不一致）；
   - **（治理时新发现,比上述更严重）** `output/mod.rs` `print_event_list` — yaml 丢弃 `task_item_id` 且把 `payload`（解析后的对象）改名改型为 `payload_json`（原始字符串）——消费者无法在 json/yaml 间切换而不重写解析器；
   - **（治理时新发现）** `commands/agent.rs:302-309` — `agent session list/get/resolve` 的 yaml 臂是手写 `println!` 伪 YAML,丢弃 json 17 字段中的 **12 个**,值未转义（含 `: ` 即产出非法 YAML）；`agent list` yaml 臂用 `{:?}` 打印 capabilities 且按条件省略键；
   - 另有：6 个命令在 clap 层广告 `-o yaml` 但运行时 `bail!`（`db` ×2、`qa doctor`、`secret key` ×3）；`handoff`/`source` 的 table 分支静默打印 yaml；`attention claim/snooze/resolve/action` 与 `source.rs` 17 处硬编码格式、完全没有 `-o` 参数（其中 `source.rs:623` 独树一帜硬编码 json,其余为 yaml）。
2. **四种输出机制并存**（双路计数,`grep -c "output: OutputFormat"` 与 arg 属性计数均为 49）：`-o {table,json,yaml}`（49 个命令,默认值 26 table / 21 yaml / 2 json）、`--json` bool（恰为 `version`、`task trace` 两处）、`--chunks-json`（恰为 `agent session read` 一处）、`--format {markdown,json}`（恰为 `guide` 一处,独立的 `GuideFormat` 枚举）；同语义命令默认值不一（`get` 默认 table、`describe` 默认 yaml；`agent session get/resolve` 默认 table 而其它单对象 `get` 默认 yaml）。
3. **29 个叶子命令零文档**（原文写 26,漏计 `source binding` 3 个；集合由 clap 树对照三个文档面逐叶重建）：`source connection`（17）、`source template preview`（1）、`source binding simulate/suspend/resume`（3）、`metrics`（3）、`handoff`（2）、`resume`（3）在 `docs/guide/07-cli-reference.md`、ZH 指南、内置 `orchestrator guide` 三个文档面全部缺席。另有 **33 个叶子**存在于 guide.rs 但两个 markdown 参考均缺席（`attention` 7、`agent session` 9、`source` 顶层 7、`source automation` 7、`audit` 2、`task timeline` 1）——markdown 参考共缺约 62 叶。clap 树共 27 个顶层命令族、126 个可见叶子、2 个 hidden 叶子。
4. **内置 guide 是 1802 行手抄件**（原文写 1780；`wc -l` 实测）：`crates/cli/src/commands/guide.rs` 手工维护恰好 90 个 `GuideEntry`（结构字面量计数与 `command:` 字段计数双路一致）,与 clap 定义无派生关系,是三文档面漂移的结构性根源。另含一个非命令伪条目 `error-codes`。`clap_complete = "4.6"` 是已声明但零引用的依赖。
5. **输出序列化失败静默吞掉**：`serde_json/yaml::to_string*(...).unwrap_or_default()` 恰好 16 处（output/ 12 处、handoff.rs 2 处、agent.rs 2 处；另 tool.rs:76 为紧凑 JSON 变体）,失败时打印空字符串而非报错。crate 内其它序列化打印点正确使用 `?`——该模式是局部不一致而非全局风格。

已知活体消费者（约束设计）：`scripts/regression/scenarios/probe-low-output.sh` 使用 `task trace --json`（迁移须同 commit）；`scripts/qa/test-agent-session-control-plane.sh` 以 jq 解析 `agent session -o json` 的数组形状（json 形状不得变）。

## 需求

### 1. 修复格式间数据等价性（P1）
- 同一命令的 json/yaml/table 呈现同一数据集（table 可省列,但 json 与 yaml 必须字段等价）；实现方式：每载荷单一 `serde_json::Value` 投影 + 序列化唯一咽喉点（`output/render.rs`）,使分歧在结构上不可能；
- `attention` 的 table 分支要么实现真表格,要么显式移除该选项;不允许静默改打 JSON；同规则适用于全部 pattern-3（table 静默改打 yaml/json）与 pattern-4（广告 yaml 但 bail）站点：**广告的格式必须真实工作,不能工作的不广告**；
- `unwrap_or_default()` 的序列化失败路径改为向 stderr 报错并非零退出。

### 2. 输出机制统一
- 全量 `-o` 约定；`--json` 保留为隐藏别名一个版本周期后移除,进 CHANGELOG Compatibility 节；`--chunks-json`（内容模式而非编码）与 `guide --format`（文档渲染而非数据输出）作为记录在案的例外保留,理由进 DD；
- 同语义命令统一默认格式：集合/list → table,单对象 get/describe/info/status 与变更回执 → yaml,流式 follow/watch → json。

### 3. 三文档面收敛为"一源两投影"
- 以 clap 定义为唯一事实源：`Cli::command()`（CommandFactory）遍历生成落库的 `config/governance/cli-surface.json`,cargo test 门禁其新鲜度；`orchestrator guide` 的命令清单与 clap 树集合双向相等（cargo test + 治理门禁双保险,后者注册为 ci-required）；
- 补齐 29 个无文档命令的 EN/ZH 参考条目 + guide.rs 条目,以及 markdown 面缺失的另外 33 叶（集合由 cli-surface.json 派生,非手抄——§4.4 shape 2）。

## 验收标准

- [ ] 对全部支持 `-o` 的命令,json 与 yaml 输出经解析后深度值相等（cargo test `format_parity::*`,满填充 fixture,覆盖全部约 22 个载荷类型）
- [ ] 负向验证：**（治理时修订）** 共享投影使"在 yaml 投影中删除字段"在结构上不再可能——诚实的负向验证改为两条：比对器自身对人为构造的分歧对必须报 FAIL（`comparator_detects_divergence`）；任何在 render.rs 之外重新引入独立 yaml 序列化的代码使 `chokepoint_no_stray_serializers` 失败
- [ ] `orchestrator guide` 与 clap 树的命令集合 diff 为空（cargo test `guide_matches_clap_leaves` + ci-required 门禁 `test-cli-doc-parity.sh` check 4）
- [ ] `docs/guide/07-cli-reference.md` 与 ZH 版覆盖全部非 hidden 叶子命令,EN/ZH 覆盖集相等,无"三面互相矛盾"项（ci-required 门禁 checks 1-3）
- [ ] CHANGELOG 记录输出机制统一的兼容性影响

## 依赖与关联

- 建议在 FR-151 发版**之后**执行:输出统一含用户可见破坏性变化,应进入下一个版本周期而非阻塞 0.4.0。（已满足：0.5.0 于 2026-08-01 发出,本 FR 在 0.5.0 后周期执行。）
- 关联 `.claude/skills/guide-alignment`（其比对逻辑是需求 3 的雏形；其 Phase 1 嵌套父命令清单已过期——漏掉 `source` 全族,闭环时一并重写）。
