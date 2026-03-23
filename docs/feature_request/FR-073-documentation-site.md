# FR-073: 文档站点与 Landing Page

## 优先级: P1

## 状态: Proposed

## 背景

项目拥有高质量的 `docs/guide/`（EN+ZH），但以 markdown 形式散落在仓库中，外部用户难以发现和浏览。需要一个独立的文档站点提升可发现性，并配合 landing page 传达核心价值主张。

## 需求

### 1. 文档站点
- 基于 mdBook（Rust 生态一致性）或 Docusaurus（功能更丰富）构建
- 部署到 GitHub Pages（`docs.c9r.io` 或 `c9r-io.github.io/orchestrator`）
- 内容来源: `docs/guide/` (EN) 和 `docs/guide/zh/` (ZH)
- 支持语言切换（EN ↔ ZH）
- 支持全文搜索
- 自动化: push to main 时自动重建和部署

### 2. Landing Page
- 单页式 marketing page，包含:
  - Elevator pitch（一句话价值主张）
  - 核心特性 highlights（DAG、CEL prehook、agent 调度、mTLS）
  - 架构图（CLI ↔ daemon ↔ workers）
  - 30 秒 quickstart 代码片段
  - 安装方式汇总（install.sh / brew / docker）
  - 指向文档站的链接
- 可集成在文档站首页或独立部署

### 3. README 精简
- 当前 README 信息量过大（300+ 行）
- 精简为: elevator pitch + 安装 + 30 秒 demo + 链接到文档站
- 详细内容全部迁移到文档站

### 4. 竞品差异化定位
- 文档站中新增 "Why orchestrator?" 页面
- 与 Airflow、Prefect、n8n、Dagger 的对比矩阵
- 强调差异化: AI-native SDLC 自动化、声明式 agent 调度、CEL 动态控制流、内置安全（mTLS + sandbox）

## 验收标准

- [ ] 文档站可访问且内容与 `docs/guide/` 同步
- [ ] EN/ZH 语言切换正常
- [ ] 全文搜索可用
- [ ] Landing page 包含核心价值主张和 quickstart
- [ ] GitHub Actions 实现自动部署
- [ ] README 精简至 100 行以内，指向文档站
