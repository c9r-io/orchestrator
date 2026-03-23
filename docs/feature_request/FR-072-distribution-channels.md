# FR-072: 分发渠道扩展 — Docker 镜像与 Homebrew

## 优先级: P1

## 状态: Proposed

## 背景

当前唯一的分发方式是 GitHub Releases 的静态二进制包 + install.sh。缺少 Docker 镜像限制了云原生场景的部署；缺少 Homebrew tap 让 macOS 用户安装不够便捷。

## 需求

### 1. Docker 镜像
- 基于 `debian:bookworm-slim` 或 `alpine` 构建最小镜像
- 包含 `orchestrator` 和 `orchestratord` 两个二进制
- 发布到 `ghcr.io/c9r-io/orchestrator`
- 支持 `linux/amd64` 和 `linux/arm64` 多架构
- 默认入口点为 `orchestratord --foreground`
- 数据卷挂载点: `/data`（映射到容器内的 `ORCHESTRATORD_DATA_DIR`）

### 2. Release Workflow 集成
- 在 `.github/workflows/release.yml` 中新增 docker build+push job
- 与二进制 release 并行构建
- Tag 策略: `v0.1.0`, `0.1`, `latest`

### 3. Homebrew Tap
- 创建 `c9r-io/homebrew-tap` 仓库
- Formula 自动从 GitHub Release 下载对应平台的 tarball
- 安装后 `orchestrator` 和 `orchestratord` 均可用
- Release workflow 自动更新 formula（通过 `dawidd6/action-homebrew-bump-formula` 或手动 dispatch）

### 4. Docker Compose 示例
- 提供 `docker-compose.yml` 示例，展示单节点部署
- 包含持久化卷配置和健康检查

## 验收标准

- [ ] `docker pull ghcr.io/c9r-io/orchestrator:latest` 成功
- [ ] `docker run ghcr.io/c9r-io/orchestrator orchestrator --version` 输出正确版本
- [ ] 多架构 manifest 验证: `docker manifest inspect` 显示 amd64 + arm64
- [ ] `brew install c9r-io/tap/orchestrator` 成功安装
- [ ] `orchestrator --version` 和 `orchestratord --version` 输出一致
- [ ] docker-compose 示例能成功启动并通过 `orchestrator init` 连接
