# FR-154: CLI 输出正确性与三文档面一致性

## 优先级: P2（需求 1 为 P1——是正确性 bug 而非风格问题）

## 状态: Proposed

## 背景

2026-08-01 审计（at `9bcfaa96`,本节数字均来自子代理对 `crates/cli/src` 的扫描,单一方法,实施时按 Phase 2 步骤 0 重建）：

1. **同一命令不同格式返回不同数据（脚本化正确性 bug）**：
   - `crates/cli/src/output/task_list.rs:7-39` — `task list -o json` 含 `total_items`/`finished_items`/`failed_items`,`-o yaml` 静默丢弃全部三项；
   - `crates/cli/src/output/mod.rs:29-64` — `task items -o yaml` 丢弃 json 中的 `fix_required`/`fixed`/`last_error`/`started_at`/`completed_at` 五项；
   - `crates/cli/src/output/attention.rs:78-86` — `attention get`/`follow` 的 `-o table` 分支实际打印 JSON。
2. **四种输出机制并存**：`-o {table,json,yaml}`（约 49 个命令）、`--json` bool（`version`、`task trace`）、`--chunks-json`（`agent session read`）、`--format {markdown,json}`（`guide`）；同语义命令默认值不一（`get` 默认 table、`describe` 默认 yaml）。
3. **约 26 个叶子命令零文档**：`handoff`（2）、`resume`（3）、`metrics`（3）、`source connection`（17）、`source template` 在 `docs/guide/07-cli-reference.md`、ZH 指南、内置 `orchestrator guide` 三个文档面全部缺席；三个面之间对 `attention`/`source`/`agent session` 的覆盖也互相矛盾。
4. **内置 guide 是 1780 行手抄件**：`crates/cli/src/commands/guide.rs` 手工维护 90 个 `GuideEntry`,与 clap 定义无派生关系,是三文档面漂移的结构性根源。
5. **输出序列化失败静默吞掉**：output 模块普遍 `serde_json::to_string(...).unwrap_or_default()`,失败时打印空字符串而非报错。

## 需求

### 1. 修复格式间数据等价性（P1）
- 同一命令的 json/yaml/table 呈现同一数据集（table 可省列,但 json 与 yaml 必须字段等价）；
- `attention` 的 table 分支要么实现真表格,要么显式移除该选项;不允许静默改打 JSON；
- `unwrap_or_default()` 的序列化失败路径改为向 stderr 报错并非零退出。

### 2. 输出机制统一
- 制定单一约定（建议全量 `-o`,`--json` 等保留为隐藏别名一个版本周期后移除,进 CHANGELOG Compatibility 节）；
- 同语义命令统一默认格式。

### 3. 三文档面收敛为"一源两投影"
- 以 clap 定义为唯一事实源,`orchestrator guide` 的命令清单与 `07-cli-reference.md` 骨架由 `--help` 树生成或由门禁比对（`guide-alignment` 技能已有编译驱动比对思路,固化为 ci-required 脚本）；
- 补齐 26 个无文档命令的 EN/ZH 参考条目（集合由 clap 树派生,非手抄——§4.4 shape 2）。

## 验收标准

- [ ] 对全部支持 `-o` 的命令,json 与 yaml 输出经 key 集合比对相等（脚本遍历验证,进 QA）
- [ ] 负向验证：在任一 yaml 投影中删除一个字段,比对脚本能失败
- [ ] `orchestrator guide` 与 clap 树的命令集合 diff 为空（ci-required 门禁）
- [ ] `docs/guide/07-cli-reference.md` 与 ZH 版覆盖全部顶层命令族,无"三面互相矛盾"项
- [ ] CHANGELOG 记录输出机制统一的兼容性影响

## 依赖与关联

- 建议在 FR-151 发版**之后**执行:输出统一含用户可见破坏性变化,应进入下一个版本周期而非阻塞 0.4.0。
- 关联 `.claude/skills/guide-alignment`（其比对逻辑是需求 3 的雏形）。
