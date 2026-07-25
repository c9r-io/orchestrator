# FR-076: GUI 正式发布 — Tauri App 打包分发

## 优先级: P1（需求 1）/ P3（需求 2–4）

## 状态: Deferred（需求 1 建议提前独立落地）

## 背景

GUI crate (`crates/gui`) 已实现 Tauri 2.x + gRPC 架构（FR-063 至 FR-069），包含许愿池 UI、实时状态推送、主题切换、i18n 等功能。但目前被排除在 CI 之外（`--exclude orchestrator-gui`），未面向用户发布。

**2026-07-25 技术负债深挖补充**：CI 的排除是双重的——

```yaml
cargo clippy --workspace --exclude orchestrator-gui --all-targets -- -D warnings
cargo test   --workspace --exclude orchestrator-gui
```

即 5592 行 / 22 个文件的 crate 同时脱离了 lint 与测试，是工作区中唯一完全无 CI 覆盖的 crate。这构成一个反向优先级：**唯一待发布的 crate 恰是唯一无人验证的 crate**，且随时间推移，恢复 CI 的成本只增不减（累积的 lint 债与 API 漂移需要在恢复时一次性偿还）。

因此本 FR 的需求 1（CI 集成）与需求 2–4（打包分发）**优先级分离**：需求 1 是持续性的质量护栏，不依赖发布决策，建议提前独立落地；需求 2–4 保持 Deferred，等待发布意愿明确。

## 需求

### 1. CI 集成（P1，可独立于发布落地）
- 在 CI 中恢复 GUI crate 的构建验证（至少 `cargo check`，理想为 `clippy -D warnings` + `cargo test`）
- 需安装 Tauri 构建依赖（Linux: `webkit2gtk-4.1`、macOS: Xcode CLI tools）
- 可作为独立 job 或 matrix entry，避免阻塞其他 crate 的 CI
- 恢复时若存在累积的 lint/编译问题，逐项修复而非新增 `--exclude` 或 `allow`；确实无法立即修复的需在 FR 中显式记录
- 恢复后从 `ci.yml` 的 clippy 与 test 两处删除 `--exclude orchestrator-gui`，并以负向验证证明 GUI 的编译错误确实能使 CI 失败

### 2. 桌面应用打包
- macOS: `.dmg` 安装包（Universal Binary: x86_64 + aarch64）
- Linux: `.AppImage` 或 `.deb` 包
- Windows: `.msi` 安装包（如支持）
- 使用 Tauri 内置的 `tauri build` 命令

### 3. Release Workflow 集成
- 在 release workflow 中新增 GUI 构建 job
- 产物上传到 GitHub Releases（与 CLI/daemon 二进制并列）
- 独立的 asset 命名: `orchestrator-gui-{version}-{platform}.{ext}`

### 4. 应用签名（macOS）
- Apple Developer 签名以避免 Gatekeeper 阻止
- 或提供 `xattr -d com.apple.quarantine` 的安装说明

## 验收标准

- [ ] `cargo check -p orchestrator-gui` 在 CI 中通过
- [ ] `ci.yml` 的 clippy 与 test 两处均已移除 `--exclude orchestrator-gui`
- [ ] 负向验证：在 GUI crate 中故意引入编译错误会使 CI 失败（证明覆盖真实生效，而非 job 空转）
- [ ] macOS `.dmg` 可正常安装和启动
- [ ] Linux `.AppImage` 可正常运行
- [ ] GUI 能通过 UDS 或 mTLS 连接到 `orchestratord`
- [ ] Release 页面包含 GUI 安装包
