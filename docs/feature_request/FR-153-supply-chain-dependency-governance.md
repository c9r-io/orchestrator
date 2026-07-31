# FR-153: 供应链与依赖面治理

## 优先级: P1

## 状态: Proposed

## 背景

2026-08-01 审计（at `9bcfaa96`）发现依赖治理存在覆盖空洞与已过期的自述：

1. **npm 双树完全脱离 Dependabot**。`.github/dependabot.yml` 仅覆盖 `cargo` 与 `github-actions` 两个 ecosystem（已复核文件），而 `gui/package.json`（React/Vitest/Playwright，lock 112KB）与 `site/`（VitePress）两棵 npm 树无任何自动更新。GUI 又同时被 CI 排除（见 FR-076），形成"无更新+无验证"叠加。
2. **Action 版本漂移**。同一 `ci.yml` 内 `actions/setup-node` 同时存在 `@v7`（4 处）与 `@v6`（2 处）；`actions/upload-artifact` 在 release.yml 为 `@v4`、ci.yml 为 `@v6`（子代理扫描，单一方法）。漂移无人察觉的原因正是上一条：github-actions ecosystem 的 Dependabot 已启用却未能收敛它们，需查明原因（分组配置/忽略规则）。
3. **`cargo deny check bans` 状态存疑**。`Cargo.lock` 中 `schemars` 解析出 3 个版本（0.8.22/0.9.0/1.2.1）、`bit-vec` 2 个版本，而 `deny.toml` 的 71 条 skip 中无对应条目；在 `multiple-versions = "deny"` 且刻意无 `skip-tree` 的策略下，要么 `security.yml` 的 bans 检查当前已红，要么 skip 表过期（子代理扫描，单一方法，未运行 `cargo deny` 复核——实施时第一步就是跑一次拿到事实）。`deny.toml` 头部自述"48 个多版本 crate / 653 个外部包"与 lock 现状（50 / 668）也已不符。
4. **17 个 unmaintained advisory 以缺省方式被接受**。`deny.toml` 仅以一句散文说明不 deny unmaintained 的理由，无逐项 advisory ID、无退出条件；对照 `.cargo/audit.toml` 中 RUSTSEC-2024-0429 的规范记录（有理由、有退出条件），这 17 项是最大的未入账依赖债（子代理扫描；数字 17 来自 deny.toml 自述，本身可能已漂移）。
5. **`.cargo/config.toml` 把本地硬件妥协提交进了仓库**。`[build] jobs = 4` + `incremental = false` 的注释写明是"减轻 USB 外置硬盘 I/O 压力"，但该文件被 track,同样约束 CI runner（已复核文件存在;对 CI 时长的影响量化为推测,未测量）。

## 需求

### 1. Dependabot 覆盖 npm
- `dependabot.yml` 新增 `npm` ecosystem × 2 目录（`/gui`、`/site`），合理分组降噪。

### 2. 收敛 action 版本
- 统一 `setup-node`、`upload-artifact` 至单一版本；查明既有 Dependabot 为何未收敛它们并修正配置。

### 3. 依赖账本对齐现实
- 运行 `cargo deny check bans` 取得事实；红则修复（升级收敛或补 skip+理由），绿则查明 skip 语义为何放过 schemars/bit-vec（注意 fr-governance §4.4 shape 8 的教训：`unmatched-skip` 断言的是"crate 在图中"而非"仍有重复"）；
- 修正 `deny.toml` 头部散文中的过期数字，或改为由脚本生成防再漂移；
- 17 项 unmaintained advisory 逐条入账（ID、接受理由、退出条件），形式对齐 `.cargo/audit.toml` 现有条目；根因（Tauri 2 锁 gtk-rs 0.18）在账本中显式记录并关联 FR-076。

### 4. 构建配置去本地化
- `[build] jobs = 4` 移出被 track 的 `.cargo/config.toml`（迁往开发者本地 `config.toml` 或环境变量说明），CI 恢复默认并行度；
- 迁移前后各测一次 `test` job 时长，把影响从推测变成测量。

## 验收标准

- [ ] Dependabot 对 `gui/`、`site/` 产生过至少一个真实 PR（或 dry-run 日志证明配置生效）
- [ ] `grep -o "actions/[a-z-]*@v[0-9]*" .github/workflows/*.yml | sort -u` 中每个 action 仅一个版本
- [ ] `cargo deny check bans` 与 `cargo audit` 在 CI 绿，且账本（skip 表 + unmaintained 清单）与 lock 现实一致（比对脚本派生,非手抄）
- [ ] 被 track 的 `.cargo/config.toml` 不再包含面向本地硬件的构建限速;CI 时长前后对比数据进 `config/governance/ci-step-cost.json` 的下一次测量

## 依赖与关联

- 与 FR-150 第 4 项（action 钉扎）同属供应链面,分属两个 FR 因发布链路有独立的 P0 时限。
- 关联 `docs/design_doc/orchestrator/156-dependency-policy-gate.md`（FR-133 的依赖策略门禁设计,本 FR 是其账本的对齐与补全）。
