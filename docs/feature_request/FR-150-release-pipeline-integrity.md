# FR-150: 发布链路完整性修复

## 优先级: P0

## 状态: Proposed

## 背景

自 v0.3.1（2026-04-06）以来未再发版，发布链路 4 个月未被执行过。2026-08-01 技术负债审计（at `9bcfaa96`）发现下次打 tag 时必然失败的缺陷，且失败发生在 GitHub Release 与 Homebrew tap 推送**已成功之后**，会留下半发布状态。

已复核的事实（直接核对 `9bcfaa96` 工作树）：

1. **crates.io 发布列表缺 2 个 crate**。`.github/workflows/release.yml:183-192` 的 publish 循环包含 proto/config/collab/security/runner/core/scheduler/client/cli/daemon 共 10 项，**不含 `crates/orchestrator-persistence` 与 `crates/slack-gateway`**——而 `core/Cargo.toml` 以非 dev 依赖引用 `orchestrator-persistence = { version = "0.3.1" }`，`crates/daemon/Cargo.toml` 同样引用 `orchestrator-slack-gateway`。两个 crate 均未声明 `publish = false`（仅 gui 与 integration-tests 声明了）。`cargo publish core` 将因 crates.io 上无 `orchestrator-persistence` 匹配版本而失败；`release.yml:186-192` 的 `already exists` 兜底不匹配该错误（错误是 "no matching package"）。
2. **Intel Mac 双重损坏**。`release.yml:56-60` 构建矩阵仅 3 个 target（linux x86_64/aarch64 + macOS aarch64），无 `x86_64-apple-darwin`；但 `install.sh` 的 `detect_os`/`detect_arch` 会照常拼出该产物 URL → curl 404 → `set -eu` 静默中止。`homebrew/orchestrator.rb:11-16` 的 `on_macos` 块只有 `Hardware::CPU.arm?` 分支，Intel Mac 得到一个没有 `url`/`sha256` 的公式。
3. **License 声明错误**。`homebrew/orchestrator.rb:9` 声明 `license "Apache-2.0"`，而仓库 `LICENSE` 与全部 14 个 crate manifest 均为 MIT。
4. **可变分支 action 持有 secret**。`release.yml:147` 使用 `dmnemec/copy_file_to_another_repo_action@main`（钉在 moving branch），并传入 `secrets.TAP_GITHUB_TOKEN`。这是全仓库唯一非 tag 钉扎的第三方 action（子代理扫描，单一方法，未二次推导）。

关联发现（本 FR 范围内一并决策，不强制修复）：

- `ci.yml` cross-compile 门禁包含 `x86_64-unknown-linux-musl`，但 `release.yml` 从不发布 musl 产物——一个被门禁守护却不出货的 target（子代理扫描，单一方法）。
- `release.yml:196` 在 10 次 `cargo publish` 之间固定 `sleep 30`（合计约 5 分钟纯等待），无 index 传播轮询（子代理扫描，单一方法）。

## 需求

### 1. 补全 crates.io 发布列表（P0）
- 在 publish 循环中按依赖拓扑序插入 `crates/orchestrator-persistence`（core 之前）与 `crates/slack-gateway`（daemon 之前）。
- 以 `cargo package -p <crate>` 对全部待发布 crate 做 dry-run 验证依赖可解析，作为 CI 或本地预发布检查。

### 2. 修复 Intel Mac 安装路径（P0，三选一并全链路一致）
- 方案 A（推荐）：矩阵新增 `x86_64-apple-darwin`（macos runner 交叉编译或 macos-13 intel runner）；
- 方案 B：`install.sh` 在 detect 阶段对不支持的 `os/arch` 组合输出明确的不支持信息并以非零码退出（而非 404 静默中止）；formula `on_macos` 增加 intel 分支 `odie` 明确报错。
- 无论选哪个方案，`install.sh`、formula、release 矩阵三处支持集合必须一致，且验收需覆盖"不支持组合得到可读错误"。

### 3. 修正 formula license 为 MIT（P0）
- `homebrew/orchestrator.rb:9` 与 `scripts/update-homebrew-formula.sh` 模板同步修正。

### 4. 钉扎第三方 action（P1）
- `dmnemec/copy_file_to_another_repo_action@main` → 钉 commit SHA（或改用 `gh` CLI 推送 tap，消除第三方 action）。

### 5. musl target 决策（P2）
- 要么在 release 中发布 musl 产物，要么从 cross-compile 门禁移除该 target 并在 CHANGELOG 记录理由——门禁集合与出货集合应一致。

## 验收标准

- [ ] publish 循环包含全部非 `publish = false` 的 workspace crate，顺序满足依赖拓扑（以脚本从 `cargo metadata` 派生集合做比对，而非手抄清单）
- [ ] 负向验证：从循环中临时移除 `orchestrator-persistence` 后，预发布检查能失败（证明检查真实生效）
- [ ] Intel Mac 场景：按所选方案，either 产物存在且 `install.sh` 可安装，or `install.sh` 与 formula 均给出明确不支持信息（非 404 中止）
- [ ] `homebrew/orchestrator.rb` license 与 `LICENSE` 一致（MIT）
- [ ] `release.yml` 无 moving-branch 第三方 action
- [ ] musl：发布产物与 CI 门禁的 target 集合一致

## 依赖与关联

- 被 FR-151（发版）阻塞性依赖：本 FR 必须先落地。
- 关联 `docs/design_doc/orchestrator/82-homebrew-tap-distribution.md`（原分发设计）。
