# FR-076: GUI 正式发布 — Tauri App 打包分发

## 优先级: P3

## 状态: Deferred

## 背景

GUI crate (`crates/gui`) 已实现 Tauri 2.x + gRPC 架构（FR-063 至 FR-069），包含许愿池 UI、实时状态推送、主题切换、i18n 等功能。但目前被排除在 CI 之外（`--exclude orchestrator-gui`），未面向用户发布。

## 需求

### 1. CI 集成
- 在 CI 中恢复 GUI crate 的构建验证（至少 `cargo check`）
- 需安装 Tauri 构建依赖（Linux: `webkit2gtk-4.1`、macOS: Xcode CLI tools）
- 可作为独立 job 或 matrix entry，避免阻塞其他 crate 的 CI

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
- [ ] macOS `.dmg` 可正常安装和启动
- [ ] Linux `.AppImage` 可正常运行
- [ ] GUI 能通过 UDS 或 mTLS 连接到 `orchestratord`
- [ ] Release 页面包含 GUI 安装包
