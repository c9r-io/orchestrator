# Deployment Pipeline 模板

> **模板用途**：构建→测试→部署顺序流水线 — 展示 ExecutionProfile 隔离和 safety 配置。

## 适用场景

- CI/CD 自动化：构建、测试、部署三阶段流水线
- 需要对不同步骤施加不同文件系统/网络隔离策略
- 需要 safety 熔断机制（连续失败时自动停止）

## 前置条件

- `orchestratord` 运行中
- 已执行 `orchestrator init`

## 使用步骤

### 1. 部署资源

```bash
orchestrator apply -f docs/workflow/deployment-pipeline.yaml --project deploy
```

### 2. 创建并运行任务

```bash
orchestrator task create \
  --name "deploy-v1" \
  --goal "Deploy version 1.0" \
  --workflow deployment_pipeline \
  --project deploy
```

### 3. 查看结果

```bash
orchestrator task list --project deploy
orchestrator task logs <task_id>
```

## 工作流步骤

```
build (sandbox) → test (host) → deploy (host)
```

1. **build** — 沙箱模式执行，仅 `build/` 和 `dist/` 可写
2. **test** — host 模式，完整访问权限运行测试套件
3. **deploy** — host 模式，执行部署和健康检查

### 核心特性：ExecutionProfile

不同步骤使用不同的执行隔离级别：

```yaml
kind: ExecutionProfile
metadata:
  name: sandbox_build
spec:
  mode: sandbox
  fs_mode: workspace_rw_scoped
  writable_paths:
    - build
    - dist
  network_mode: inherit
```

- `sandbox` 模式：限制文件系统写入范围，保护源代码
- `host` 模式：完整权限，适合需要系统工具的步骤
- `network_mode: inherit`：继承宿主机网络（agent 需要 API 访问）

### 核心特性：Safety 配置

```yaml
safety:
  max_consecutive_failures: 1
  auto_rollback: false
```

连续失败 1 次即停止 workflow，防止在构建失败时继续部署。

## 自定义指南

### 添加审批步骤

在 test 和 deploy 之间插入人工审批：

```yaml
- id: approval
  type: approval
  scope: task
  required_capability: review
  enabled: true
```

### 调整沙箱权限

根据项目结构修改 `writable_paths`：

```yaml
writable_paths:
  - target          # Rust 项目
  - node_modules    # Node.js 项目
  - dist
```

### 启用自动回滚

```yaml
safety:
  max_consecutive_failures: 1
  auto_rollback: true
```

## 进阶参考

- [自举引导执行](/zh/showcases/self-bootstrap-execution-template) — 含 ExecutionProfile 的生产级 workflow
- [高级特性](/zh/guide/advanced-features) — ExecutionProfile 和 safety 详解
- [工作流配置](/zh/guide/workflow-configuration) — 步骤执行模型
