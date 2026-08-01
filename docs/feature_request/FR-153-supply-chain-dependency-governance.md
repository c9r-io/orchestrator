# FR-153: 供应链与依赖面治理

## 优先级: P1

## 状态: In Progress

## 背景

2026-08-01 审计（at `9bcfaa96`）提出五项主张；治理 Phase 2 step 0 于 `a538d508` 全量重建后，两项成立、两项需改写、一项反转。以下为修正后的事实（每个数字带方法与 revision）：

1. **npm 三树（非双树）当前脱离 Dependabot，且这是一次无记录的主动移除，不是从未覆盖。** `.github/dependabot.yml` 在 `b16b9156`（2026-07-23）曾加入 npm × 3 目录（`/gui`、`/site`、`/.claude/skills/project-bootstrap/assets/template/portal`——原 FR 漏计第三棵树），数小时后在 `3446b652`（"chore: finish Dependabot cleanup"）被整体移除，commit message 无理由。移除前产生的 npm 升级 PR（#67–75，均为 breaking major）已于 2026-07-22 全部 close 未合（`gh pr list --author app/dependabot`）。GUI 同时被 CI 排除（见 FR-076），"无更新+无验证"叠加成立。
2. **Action 版本漂移成立，计数修正，根因已查明。** ci.yml 内 `actions/setup-node` @v7 ×2 + @v6 ×2（非 4/2；docs.yml 另有 @v7 ×1）；`actions/upload-artifact` release.yml @v4、ci.yml @v6（`grep -o … | sort | uniq -c`，at `a538d508`）。Dependabot 未收敛的机制：升级 PR #65（setup-node 6→7，2026-07-22 close）与 #15（upload-artifact 4→7，2026-03-10 close）被关闭后，Dependabot 不再重新提议同一版本；随后人工提交 `1c0b170d`（2026-07-25）又新增了 @v6 步骤。配置中无 ignore 规则——不是配置缺陷，是 closed-PR 抑制 + 人工混入。
3. **【反转】`cargo deny check bans --deny unmatched-skip` 在 `a538d508` 本地实测绿。** 原 FR 看到的 schemars ×3 / bit-vec ×2 仅存在于 `Cargo.lock`，不在 cargo-deny 检查的 feature-resolved graph 中：它们是从未启用的 optional 依赖（`serde_with` 的 schemars 0.9/1.2 feature、`yasna` 的 bit-vec 0.9.1），lock 是 graph 的超集。deny.toml 自述"48 个多版本 crate / 70 条接受"对 graph **精确成立**（第二派生：`cargo tree --all-features --target all --duplicates` → 48，at `a538d508`）；原 FR 的 50/668 量的是错误的宇宙（裸 lock grep）。仅"653 个外部包"一数方法不明（lock 外部 = 654，tree 派生 ≈ 630），实施时需二次派生或改为方法注记。副产物发现：`skip-is-live` 以 lock 判定重复，幻影 lock 条目可让一条过期 skip 同时躲过 `unmatched-skip` 与 `skip-is-live`（窄门，离线不可修，入 DD known limits）。
4. **17 项 unmaintained advisory 计数经第二途径确认（`cargo audit` at `a538d508`：恰 17 条 unmaintained warning），但根因需拆分：** 11/17 为 gtk-rs 0.18 世代（10 个 gtk 系 crate + proc-macro-error via glib-macros）← Tauri 2 / FR-076；5/17 为 unic-* via tauri-utils → urlpattern（属 Tauri 但非 gtk-rs）；**1/17 为 paste via cel-interpreter——daemon/CLI 侧依赖，与 Tauri 完全无关**。原 FR "根因（Tauri 2 锁 gtk-rs 0.18）"仅覆盖 11/17。
5. **`.cargo/config.toml` 把本地硬件妥协提交进了仓库——原样成立。** `[build] jobs = 4` + `[profile.dev] incremental = false`，注释自述为 USB 外置硬盘减压，被 track，CI 无覆盖（已复核文件；对 CI 时长的影响未测量——ubuntu runner 为 4 vCPU，预期 Δ≈0，测量把推测变数据）。

## 需求

### 1. Dependabot 覆盖 npm（三棵树）
- `dependabot.yml` 新增 `npm` ecosystem × 3 目录（`/gui`、`/site`、`/.claude/skills/project-bootstrap/assets/template/portal`），minor+patch 分组降噪，major 单独成 PR。
- 覆盖集须由仓库派生（所有 package.json 树）而非手抄清单，防再次"cleanup"静默移除（§4.4 shape 2）。

### 2. 收敛 action 版本
- 统一 `setup-node` 至 @v7；release.yml 的 `upload/download-artifact` 成对升至经核实的最新 major。
- closed-PR 抑制机制入 DD；配置无需修正（无 ignore 规则）。

### 3. 依赖账本对齐现实（改写后）
- bans 已绿；改为**门禁化 deny.toml 自述数字**（48/70 由 skip 表自派生，新规则守护），"653" 二次派生后修正或改为方法注记；
- lock-vs-graph 机制（serde_with/yasna 幻影条目）与 `skip-is-live` 窄门入 DD；
- 17 项 unmaintained advisory 逐条入账（ID、接受理由、退出条件），形式对齐 `.cargo/audit.toml` 现有条目，并以 `--deny unmaintained` 使账本具备约束力（第 18 条未入账即红）；根因按 11/5/1 拆分记录，gtk-rs 部分关联 FR-076。

### 4. 构建配置去本地化
- `[build] jobs = 4` 与 `[profile.dev] incremental = false` 移出被 track 的 `.cargo/config.toml`（本地配方：`~/.cargo/config.toml`），CI 恢复默认并行度；
- 迁移前后各测一次 `Rust test` job 时长（前值已取：run 30684584564 at `7d3abb8f`，259s），把影响从推测变成测量。

## 验收标准

- [ ] Dependabot 对三棵 npm 树产生过至少一个真实 PR（或推送配置后的更新运行证据）
- [ ] `grep -o "actions/[a-z-]*@v[0-9]*" .github/workflows/*.yml | sort -u` 中每个 action 仅一个版本（配对 workflow_model.rb 解析派生的清单，grep 仅作代理）
- [ ] `cargo deny check bans` 与 `cargo audit --deny unsound --deny unmaintained` 在 CI 绿，且账本（skip 表自述数字 + unmaintained 清单）由脚本派生守护
- [ ] 被 track 的 `.cargo/config.toml` 不再包含面向本地硬件的构建限速；`Rust test` job 前后时长对比数据入 DD 与 `config/governance/ci-step-cost.json` 注记

## 依赖与关联

- 与 FR-150 第 4 项（action 钉扎）同属供应链面，分属两个 FR 因发布链路有独立的 P0 时限。
- 关联 `docs/design_doc/orchestrator/156-dependency-policy-gate.md`（FR-133 的依赖策略门禁设计，本 FR 是其账本的门禁化与补全）。
- 关联 FR-076（GUI 发布）：11/17 unmaintained 的退出条件系于 Tauri 迁出 gtk-rs 0.18。
