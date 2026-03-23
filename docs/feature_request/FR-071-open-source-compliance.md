# FR-071: 开源合规基础设施

## 优先级: P0

## 状态: Proposed

## 背景

项目即将进入产品化推广阶段，但缺少开源项目的基础合规文件。没有 LICENSE 文件意味着代码法律上不可被外部使用；缺少 CONTRIBUTING.md 和 issue 模板阻碍社区参与。

## 需求

### 1. LICENSE 文件
- 在仓库根目录添加 LICENSE 文件
- 推荐 MIT 或 Apache-2.0（与 `core/Cargo.toml` 中声明的 `license = "MIT"` 保持一致）

### 2. CHANGELOG.md
- 遵循 [Keep a Changelog](https://keepachangelog.com/) 格式
- 从 git history 整理已有版本的变更记录
- 后续每次 release 更新

### 3. CONTRIBUTING.md
- 开发环境搭建指南（Rust toolchain、protoc、cargo test）
- 代码风格约定（clippy、fmt、async lock governance）
- PR 提交流程和 review 标准
- Issue 提交指南

### 4. GitHub 模板
- `.github/ISSUE_TEMPLATE/bug_report.md` — Bug 报告模板
- `.github/ISSUE_TEMPLATE/feature_request.md` — 功能请求模板
- `.github/PULL_REQUEST_TEMPLATE.md` — PR 模板（含 checklist）

### 5. 首个正式 Release
- 基于当前代码发布 v0.1.0 tag
- 触发 release workflow 生成 4 平台二进制包
- 验证 install.sh 能正确下载和安装

## 验收标准

- [ ] LICENSE 文件存在且内容正确
- [ ] CHANGELOG.md 覆盖 v0.1.0 的主要功能
- [ ] CONTRIBUTING.md 包含完整的贡献指南
- [ ] GitHub 模板可用（创建 issue/PR 时出现模板选项）
- [ ] v0.1.0 release 在 GitHub Releases 页面可见
- [ ] `curl -fsSL .../install.sh | sh` 能成功安装
