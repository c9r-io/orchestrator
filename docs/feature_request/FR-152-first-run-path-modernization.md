# FR-152: 首跑路径现代化 — quickstart、fixture 与错误码可读性

## 优先级: P1

## 状态: Proposed

## 背景

2026-08-01 技术负债审计（at `9bcfaa96`）发现新用户首次运行的每一步都会撞上废弃警告或断裂引用：

1. **README quickstart 不可复制执行**。`README.md:35-47` 的第 3 步是 `orchestrator apply -f manifest.yaml`，但仓库内不存在任何 `manifest.yaml`（子代理扫描，单一方法）；`task create --goal` 无 `--workflow` 的用法也与 `docs/guide/01-quickstart.md`（始终传 `--workflow simple_qa`）不一致。
2. **quickstart 教的是运行时已在废弃的写法**。`docs/guide/01-quickstart.md` Step 4 的 Agent manifest 使用 `spec.command` 且无 `spec.driver`，apply 时触发 `core/src/resource/agent.rs:99` 的 `[legacy_agent_command_deprecated]` 警告（已由 FR-126/DD-138 确立 typed driver 为正道）。
3. **fixture 语料复现同一废弃形态**。document 级重推导（per-document census：按 `^---$` 分割 + `kind: Agent` 判定，at `7d3abb8f`）：**147 个 Agent document 中 137 个 driverless、仅 10 个 typed**（file 级为 85 文件/5 文件，即初稿的计数单位）。分布：`fixtures/manifests/bundles/` 63 文件/114 docs、`fixtures/` 顶层 14 文件/18 docs（其中 13 个文件全仓库零引用、7 个是 bundles 同名文件的字节级重复）、`fixtures/benchmarks/` 5 docs、`fixtures/workflow/` 10 docs；另有 bundles glob 之外的 `crates/integration-tests/tests/common/manifests/` 6 docs（0 typed）与 `test-yaml-warnings/` 5 docs（FR-155 将删除该目录）。step 级 `spec.steps[].command`（10 文件 52 处）不属本类，实施须 document-aware 处理。
4. **16 种带方括号的机器错误码集中于首跑路径，且无集中词汇表**。二次推导（at `7d3abb8f`）：字面量 8 个（`driver_config_invalid`、`driver_raw_args_unsafe_mode_required`、`legacy_agent_command_deprecated`、`legacy_agent_execution_removed`、`legacy_coordination_removed`、`legacy_json_path_removed`、`legacy_runner_executor_removed`、`empty_change_check`）+ 经 `driver_error("[{code}] …")` 插值 7 个（`driver_multi_turn_required` 等 requirement 码，`core/src/config_load/validate/workflow_steps.rs:180-190`）+ 常量插值 `[FILE_SHARING_GLOBAL_SKILL_UNTRUSTED]`。初稿"10 个、docs/guide 0 次出现"两者皆误：字面量 grep 漏掉插值形态（fr-governance §4.4 shape 4），且 `docs/guide/agent-driver-model.md:148` 已有 6 码修复表、`02-resource-model.md`（EN/ZH）已提及 2 个 legacy 码。真实缺口：无集中词汇表、词汇表集合无派生比对、6 个码 guide 零覆盖、CLI 错误输出无查询入口指引。
5. **install.sh 向用户当前目录静默解包**。`install.sh:175-183`（初稿行号 154-162 已漂移）将 skills tarball `tar -xzf ... -C "."` 解入 CWD，`curl | sh` 从 `$HOME` 执行时即向 `$HOME` 写入 `.claude/skills/`，无提示、无确认。

## 需求

### 1. README quickstart 可复制执行
- 仓库内提供 quickstart 引用的最小 manifest（如 `docs/guide/examples/quickstart-manifest.yaml` 或 `fixtures/quickstart/`），README 与 guide 指向同一文件；
- README 与 `01-quickstart.md` 的命令序列一致（同一 workflow 名、同一参数形态）。

### 2. quickstart 与 guide 全量改用 typed driver
- `01-quickstart.md` 及 EN/ZH 两侧全部示例 Agent 使用 `spec.driver`（`shell/cli` 起步）；
- 干净环境走完 quickstart 全程,终端零 `[legacy_*]` 警告。

### 3. fixture 语料去废弃化
- 按 document 级清单（见背景 3）除**专门测试废弃路径**的 document（显式豁免注释标明）外全部补 `driver:`；零引用的顶层重复文件删除而非迁移；
- 豁免须为机器可解析的 per-document 注释（如 `# fixture-driverless-exempt: <reason>`），注意有活门禁按**精确计数**断言 legacy 警告（`test-agent-driver-production-parity.sh` 恰好 3 次）；
- 在 fixture 校验门禁中加入"新增 Agent fixture 必须含 driver 或豁免注释"的检查（scope 派生自 `git ls-files`，覆盖 bundles 之外的语料），防止回潮。

### 4. 错误码词汇表
- 新增 `docs/guide/error-codes.md`（EN/ZH），逐条解释 16 个方括号错误码（见背景 4）的含义、触发条件与处置动作；
- 词汇表条目集合由脚本从源码派生比对（不允许手抄清单——见 fr-governance §4.4 shape 2；提取规则须覆盖插值形态，排除项须带理由且防失稳）；
- CLI 错误输出附带指向词汇表的短提示（或 `orchestrator guide error-codes` 入口）。

### 5. install.sh 写入行为显式化
- skills 解包目标改为显式约定目录：默认 `$HOME/.claude/skills`，以 `INSTALL_ORCHESTRATOR_SKILLS_DIR` 环境变量覆盖（`curl | sh` 形态无法接收 flag，沿用既有 `INSTALL_ORCHESTRATOR_*` 约定），解包前输出目标路径；
- 不再无条件写 CWD。

## 验收标准

- [ ] 从干净环境按 README 逐行执行 quickstart 到 `task logs` 全程成功，无 404 引用、无 `[legacy_*]` 输出（记录执行日志）
- [ ] document 级派生（`git ls-files '*.yaml'` + `kind: Agent` 解析）的 driverless 清单为空或全部带豁免注释
- [ ] 负向验证：一个无 driver 无豁免的 Agent fixture document 能使门禁失败（变异形态为注释掉 driver 块，非删除）
- [ ] `docs/guide/error-codes.md` 条目集合与源码派生集合双向一致，且 ZH 与 EN 集合一致（脚本比对，进 ci-required 或 qa-doc-lint）
- [ ] `install.sh` 在任意 CWD 执行后,CWD 无新增文件（除非显式指定）

## 依赖与关联

- 与 FR-155（AGENTS.md 重写）同主题不同文件面，可并行；AGENTS.md 的示例修正划归 FR-155。
- 关联 `docs/design_doc/orchestrator/127-agent-driver-abstraction.md`、`138-agent-driver-execution-migration.md`。
