# FR-158: 治理体系自省 — 门禁的门禁、成本与新鲜度

## 优先级: P3

## 状态: Proposed

## 背景

FR-127→149 的 6 天 23 个 FR（含 fixture 有效性 4 轮返工、FR-126 四轮审计重闭环）表明治理体系已成为最大的需求来源。本 FR 不是"再加一层门禁",而是把审计发现的三个结构性弱点收敛,并为"门禁产生需求"的元问题建立止损边界。at `9bcfaa96`,除注明外为子代理扫描（单一方法）：

1. **`OUTCOMES` 手抄清单是仓库自己警告过的反模式**。governance job 有 36 处 `continue-on-error` 步骤,聚合步骤靠 `ci.yml:383-437` 中手工维护的 35 个 step id 清单;新增步骤忘登记则门禁静默失效。这正是 §4.4 shape 2（enumeration standing in for coverage）,且技能 §4.4 第 6 条已实测记录过该 job 的观测陷阱。
2. **manual-runbook 门禁零新鲜度信号**。`qa-gate-surface.json` 分类:ci-required 46、manual-runbook 33（42%）;`ci-job-liveness.json` 只追踪 workflow job,没有任何机制记录 33 个人工门禁上次真实执行时间;`staleClaimExemptions: []` 意味着连豁免登记都是空的。
3. **棘轮仍是行数正则**。DD-140 自记 "The four ratchets remain line-count regexes"、DD-148 自记 "The ratchet counts the driver's name in prose"——已知会把散文计入消费者;两次实测缺陷（monotonic 方向错误、fixture 泄入生产计数）都源于此。
4. **成本**：governance job 1139s 是 CI 关键路径（19 分钟）,前 5 个治理步骤合计 689s;预算门禁只守 12 个 job 中的 2 个,而全 job 总和 2709s 已超那个 2700s 预算数字本身的语义边界（`config/governance/ci-step-cost.json`,已复核数字;"预算只守 2 个 job"为对 workflow 的解读,实施时核对）。
5. **26/113 个脚本不在治理面**,其中包括所有 ci-required 门禁 source 的共享库 `scripts/lib/*.rb|sh`——门禁被治理,门禁的引擎不被治理。`scripts/watchdog.sh` 被 `docs/architecture.md` §7 描述为生存机制 Layer 4,实际零调用、零治理、且不会如文档所称重启服务。

## 需求

### 1. OUTCOMES 清单派生化
- 从 workflow YAML 解析 governance job 全部 `continue-on-error: true` 步骤 id,与聚合步骤读取的集合比对,不一致即失败（用既有 `scripts/lib/workflow_model.rb` 解析,不再手抄;负向 fixture:新增未登记步骤能被发现）。

### 2. manual-runbook 新鲜度账本
- 33 个人工门禁增加最近执行记录（执行日期、revision、日志路径）,由 certify 类脚本写入;
- 超过阈值（建议 90 天）未执行的门禁在 governance 汇总中显式列出——只报告,不阻断（避免制造新的必须喂养的门禁）。

### 3. 棘轮正则 → 结构化度量（选点试点,不全面铺开）
- 选 1 个已知误差最大的棘轮（DD-148 点名的"数散文中驱动名"者）,改为基于 `rust_source.rb`/AST 的结构化计数;其余棘轮仅在其数字下次需要变动时逐个迁移——明确"不为迁移而迁移"。

### 4. 脚本治理面收口
- `scripts/lib/*` 以 `supportFiles` 身份入 `qa-gate-surface.json`;
- `watchdog.sh` 三选一:实现文档声称的行为并纳入治理、降级文档描述与现实一致、或删除并修订 `docs/architecture.md` §7 为三层机制。

### 5. 元边界（本 FR 的真正目的）
- 在治理 README/DD 中写下扩张预算:治理步骤总时长上限、新增门禁必须先回答"§4.4 的哪个 shape 要求它存在"、以及"一个门禁连续 N 个审计周期零捕获即降级为 manual-runbook"的退役规则——给治理体系装上它给别人装的棘轮。

## 验收标准

- [ ] 负向验证:向 governance job 添加一个 `continue-on-error` 步骤而不登记 OUTCOMES,派生比对门禁失败
- [ ] 33 个 manual-runbook 门禁各有至少一条执行记录或显式 stale 列报
- [ ] 试点棘轮改造后,DD-148 记录的"散文计数"误差场景有 fixture 证明不再复现
- [ ] `qa-gate-surface.json` 的 scripts+supportFiles 集合与 `git ls-files scripts/` 派生集合的 diff 为空或逐项豁免
- [ ] `docs/architecture.md` §7 与 watchdog 现实一致
- [ ] 治理扩张预算与退役规则成文并被 DD 索引

## 依赖与关联

- 最后执行:它依赖前序 FR 的实施经验来校准"元边界"的数值;且其本身是治理工作,在产品债（FR-150~154）清偿前继续扩张治理面就是重复病灶。
- 关联 DD-140（账本再生）、DD-145（gate surface execution truth）、技能 `fr-governance` §4.4（多个 shape 的第一手案例库）。
