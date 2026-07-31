# FR-151: 0.4.0 版本发布与 Unreleased 清算

## 优先级: P0

## 状态: Proposed（依赖 FR-150）

## 背景

`CHANGELOG.md` 的 `[Unreleased]` 块自 v0.3.1（2026-04-06）起累积了约 4 个月的已合并变更（at `9bcfaa96` 时约 130KB，涵盖 FR-126 至 FR-149 的全部工作；子代理扫描，单一方法），期间 0 次发版。

**2026-08-01 FR-150 治理中发现（已复核）：v0.3.1 本身是一次幻影发布。** tag `v0.3.1`（22eab222）存在于远端，CHANGELOG 记录 `[0.3.1] - 2026-04-06`，14 个 crate manifest 均为 0.3.1——但 `gh run list --workflow=release.yml` 显示 release 流水线最后一次运行是 v0.3.0（2026-04-04），GitHub Releases、crates.io（`agent-orchestrator`/`orchestrator-cli` 最新均为 0.3.0）与 Homebrew tap 全部停在 0.3.0。可能机制是 tag 在不触发 workflow 的上下文中被推送（如另一 workflow 内用 `GITHUB_TOKEN` 推 tag——GitHub 的防递归规则会吞掉触发）；确切原因由本 FR 查明。后果：

- 发布链路 4 个月未被执行，FR-150 列出的缺陷因此从未暴露；
- 全部治理门禁、依赖修复、agent driver 迁移、协调坍缩成果均未到达任何已发布产物；
- 发版间隔越长，`[Unreleased]` 中被后续工作**证伪**的条目越多（fr-governance 技能 Phase 5 明确要求 Unreleased 条目不是历史记录，过期条目会作为错误陈述随版本发出）。

发版节奏崩坏的时间点与治理 FR 流（FR-127→149，6 天 23 个 FR）的开始重合——这是"治理吞噬产品"的可量化症状，恢复发版节奏本身就是治理目标。

## 需求

### 1. 发布前清算 `[Unreleased]`
- 逐节重读,修正已被后续 FR 证伪的条目（技能 Phase 5.3 的要求，此处成批执行）；
- 确认 `### Removed` 与 `### Compatibility And Migrations` 完整覆盖 27→37 号迁移与全部 `[legacy_*_removed]` 拒绝语义。

### 2. 版本决策与打 tag
- 含 11 个 forward-only 迁移与多项 apply-time 拒绝语义变化，语义上不是 patch；建议 `0.4.0`；
- 全部 14 个 crate 版本号统一 bump（当前 `0.3.1` 均匀一致，已复核 `Cargo.toml`）；
- **幻影 v0.3.1 的清算**：查明 tag 未触发 workflow 的原因并修复触发路径（若根因是 `GITHUB_TOKEN` 推 tag，改用 PAT/deploy key 或手动 `workflow_dispatch`）；确认 0.4.0 的 tag 推送方式能真实触发 release.yml；在 CHANGELOG 的 `[0.3.1]` 节补注"该版本从未产出发布产物，其变更由 0.4.0 首次发布"；crates.io 从未见过 0.3.1，0.4.0 直接发布即可，无需补发。

### 3. 全链路验证发布
- GitHub Release 产物、crates.io 10+2 个 crate、Homebrew formula 占位符替换（`scripts/update-homebrew-formula.sh`）全部成功；
- 发布后从干净机器（或容器）执行 `install.sh` 与 `brew install` 各验证一次。

## 验收标准

- [ ] FR-150 全部 P0 项已闭环
- [ ] `CHANGELOG.md` 存在 `## [0.4.0] - <date>` 且 `[Unreleased]` 清空
- [ ] crates.io 上 `agent-orchestrator`、`orchestrator-persistence`、`orchestrator-slack-gateway` 等全部发布 crate 可见且版本一致
- [ ] Homebrew tap 更新成功，`brew install` 后 `orchestrator --version` 输出 0.4.0
- [ ] `install.sh` 在 Apple Silicon macOS 与 x86_64 Linux 各验证通过一次（记录执行环境与日志路径）

## 依赖与关联

- 阻塞性依赖 FR-150。
- 建议与 FR-153（供应链治理）同批或先后紧邻落地，使发布产物包含依赖修复。
