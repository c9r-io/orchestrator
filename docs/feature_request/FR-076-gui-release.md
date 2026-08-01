# FR-076: GUI 正式发布 — Tauri App 打包分发

## 优先级: P1（需求 1）/ P3（需求 2–4）

## 状态: In Progress（需求 1 已落地待验证闭环；需求 2–4 维持 Deferred）

## 背景

GUI crate (`crates/gui`) 已实现 Tauri 2.x + gRPC 架构（FR-063 至 FR-069），包含许愿池 UI、实时状态推送、主题切换、i18n 等功能。规模 5592 行 / 22 文件（`find`+`wc`，at 9e2c54f6）。长期被排除在 clippy 与 test 之外，未面向用户发布。

**2026-07-25 技术负债深挖补充**（2026-08-01 治理时按 Phase 2 步骤 0 逐条重建，三处修正）：

1. **排除是三重而非"双重"**：clippy、test 两处之外，cross-compile job 的 `cargo check --workspace --exclude orchestrator-gui --target …` 是第三处（at 9e2c54f6）。第三处经决策保留（外部 target 无 webkit 交叉工具链；宿主 target 缺 `gui/dist`），理由已写入 ci.yml 现场注释与 DD-165。
2. **"唯一完全无 CI 覆盖的 crate" 写下一天后即失效**：`c9ada747`（2026-07-26）起 `boundary-coverage` job 在 macOS 上经 `cargo llvm-cov --workspace --all-targets --all-features` 编译并运行其测试。真实缺口比原文窄：clippy lint 覆盖（任何地方都没跑过）与 Linux 构建（无 job 安装 webkit2gtk/gtk）。
3. **预期的"累积 lint 债"实测为零**（at 9e2c54f6，macOS，含 GUI 的 workspace clippy -D warnings 与 test 全绿）——boundary-coverage 持续编译该 crate 使债务未能累积。

## 需求

### 1. CI 集成（P1，已落地 — 2026-08-01，DD-165 / QA-203）
- [x] 在 CI 中恢复 GUI crate 的构建验证（落地为完整 `clippy -D warnings` + `cargo test`，非仅 `cargo check`）
- [x] 安装 Tauri 构建依赖（Linux: `libwebkit2gtk-4.1-dev libgtk-3-dev`；另需 Node 22 + `npm run build` 产出 `tauri::generate_context!` 编译期读取的 `gui/dist`）
- [x] 落地形态：就地拓宽 clippy 与 test job（而非独立 job）——`-p` 形态的独立 job 会在收窄的 feature unification 下 lint，与 workspace 图不一致（DD-165 记录了实测反例）
- [x] 恢复时无累积 lint/编译问题需修复（实测为零，见上）
- [x] 从 `ci.yml` 的 clippy 与 test 两处删除 `--exclude orchestrator-gui`；cross-compile 处保留并注释理由

### 2. 桌面应用打包（Deferred）
- macOS: `.dmg` 安装包（Universal Binary: x86_64 + aarch64）
- Linux: `.AppImage` 或 `.deb` 包
- Windows: `.msi` 安装包（如支持）
- 使用 Tauri 内置的 `tauri build` 命令

### 3. Release Workflow 集成（Deferred）
- 在 release workflow 中新增 GUI 构建 job
- 产物上传到 GitHub Releases（与 CLI/daemon 二进制并列）
- 独立的 asset 命名: `orchestrator-gui-{version}-{platform}.{ext}`

### 4. 应用签名（macOS）（Deferred）
- Apple Developer 签名以避免 Gatekeeper 阻止
- 或提供 `xattr -d com.apple.quarantine` 的安装说明

## 验收标准

需求 1（验证方法与证据见 QA-203）：

- [x] GUI crate 随 workspace 在 CI 中编译通过——run `30698178356`（cf891c9c）clippy 与 test job 双双 `success`，orchestrator-gui 首次在 Linux CI 构建
- [x] `ci.yml` 的 clippy 与 test 两处均已移除 `--exclude orchestrator-gui`（QA-203 场景 1 从解析后的 workflow 派生断言）
- [x] 负向验证：GUI crate 的编译错误使 CI 失败——run `30698184730`（throwaway 分支 fdc5c73a，`workflow_dispatch`，未触碰 main）clippy 与 test job conclusion 双双 `failure` 而 fmt `success`（§4.4 shape 6：以 job conclusion 为证据；QA-203 场景 3 含正负对照）

需求 2–4（Deferred）：

- [ ] macOS `.dmg` 可正常安装和启动
- [ ] Linux `.AppImage` 可正常运行
- [ ] GUI 能通过 UDS 或 mTLS 连接到 `orchestratord`
- [ ] Release 页面包含 GUI 安装包
