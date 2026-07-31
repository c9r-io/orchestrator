# FR-152: 首跑路径现代化 — quickstart、fixture 与错误码可读性

## 优先级: P1

## 状态: Proposed

## 背景

2026-08-01 技术负债审计（at `9bcfaa96`）发现新用户首次运行的每一步都会撞上废弃警告或断裂引用：

1. **README quickstart 不可复制执行**。`README.md:35-47` 的第 3 步是 `orchestrator apply -f manifest.yaml`，但仓库内不存在任何 `manifest.yaml`（子代理扫描，单一方法）；`task create --goal` 无 `--workflow` 的用法也与 `docs/guide/01-quickstart.md`（始终传 `--workflow simple_qa`）不一致。
2. **quickstart 教的是运行时已在废弃的写法**。`docs/guide/01-quickstart.md` Step 4 的 Agent manifest 使用 `spec.command` 且无 `spec.driver`，apply 时触发 `core/src/resource/agent.rs:99` 的 `[legacy_agent_command_deprecated]` 警告（已由 FR-126/DD-138 确立 typed driver 为正道）。
3. **fixture 语料复现同一废弃形态**。85 个声明 `kind: Agent` 的 fixture 中仅 5 个含 `driver:`（子代理 grep 统计，单一方法，未二次推导——实施时须按 fr-governance Phase 2 步骤 0 重新推导，并区分"step 级 command"与"Agent 级 command 模板"两类，避免类别混淆）。
4. **10 种带方括号的机器错误码直达终端且零文档**。`[driver_config_invalid]`、`[legacy_agent_command_deprecated]`、`[legacy_coordination_removed]` 等 10 个前缀（子代理 grep `"\[[a-z][a-z0-9_]+\]"` 统计）经 anyhow 链路原样打印到用户终端，`docs/guide/` 中 0 次出现，无任何查询入口。它们恰好集中在 `apply` 与首次执行这两条首跑路径上。
5. **install.sh 向用户当前目录静默解包**。`install.sh:154-162` 将 skills tarball `tar -xzf ... -C "."` 解入 CWD，`curl | sh` 从 `$HOME` 执行时即向 `$HOME` 写入 `.claude/skills/`，无提示、无确认（已复核该代码路径存在；行号来自子代理扫描）。

## 需求

### 1. README quickstart 可复制执行
- 仓库内提供 quickstart 引用的最小 manifest（如 `docs/guide/examples/quickstart-manifest.yaml` 或 `fixtures/quickstart/`），README 与 guide 指向同一文件；
- README 与 `01-quickstart.md` 的命令序列一致（同一 workflow 名、同一参数形态）。

### 2. quickstart 与 guide 全量改用 typed driver
- `01-quickstart.md` 及 EN/ZH 两侧全部示例 Agent 使用 `spec.driver`（`shell/cli` 起步）；
- 干净环境走完 quickstart 全程,终端零 `[legacy_*]` 警告。

### 3. fixture 语料去废弃化
- 重新推导 driverless Agent fixture 清单（区分两类 command，逐个标注）；
- 除**专门测试废弃路径**的 fixture（显式注释标明）外全部补 `driver:`；
- 在 fixture 校验门禁中加入"新增 Agent fixture 必须含 driver 或豁免注释"的检查，防止回潮。

### 4. 错误码词汇表
- 新增 `docs/guide/error-codes.md`（EN/ZH），逐条解释 10 个方括号错误码的含义、触发条件与处置动作；
- 词汇表条目集合由 grep 从源码派生比对（不允许手抄清单——见 fr-governance §4.4 shape 2）；
- CLI 错误输出附带指向词汇表的短提示（或 `orchestrator guide error-codes` 入口）。

### 5. install.sh 写入行为显式化
- skills 解包目标改为显式约定目录（如 `$HOME/.claude/skills`，或 `--skills-dir` 参数），解包前输出目标路径；
- 不再无条件写 CWD。

## 验收标准

- [ ] 从干净环境按 README 逐行执行 quickstart 到 `task logs` 全程成功，无 404 引用、无 `[legacy_*]` 输出（记录执行日志）
- [ ] `grep -rL "driver:" fixtures/**/agent*.yaml` 派生的 driverless 清单为空或全部带豁免注释
- [ ] 负向验证：新增一个无 driver 无豁免的 Agent fixture 能使门禁失败
- [ ] `docs/guide/error-codes.md` 条目集合与源码 grep 派生集合一致（脚本比对，进 ci-required 或 qa-doc-lint）
- [ ] `install.sh` 在任意 CWD 执行后,CWD 无新增文件（除非显式指定）

## 依赖与关联

- 与 FR-155（AGENTS.md 重写）同主题不同文件面，可并行；AGENTS.md 的示例修正划归 FR-155。
- 关联 `docs/design_doc/orchestrator/127-agent-driver-abstraction.md`、`138-agent-driver-execution-migration.md`。
