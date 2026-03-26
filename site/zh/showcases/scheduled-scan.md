# Scheduled Scan 模板

> **模板用途**：定时触发的安全/合规审计 — 展示 Trigger 资源和 cron 调度。

## 适用场景

- 定期安全扫描：依赖漏洞检测、硬编码凭证排查
- 合规审计：定期检查配置规范和安全策略
- 任何需要周期性自动执行的审计任务

## 前置条件

- `orchestratord` 运行中（cron trigger 需要 `--webhook-bind` 参数）
- 已执行 `orchestrator init`

## 使用步骤

### 1. 部署资源

```bash
orchestrator apply -f docs/workflow/scheduled-scan.yaml --project scan
```

### 2. 手动触发一次（测试）

```bash
orchestrator task create \
  --name "scan-1" \
  --goal "Run security audit" \
  --workflow scheduled_scan \
  --project scan
```

### 3. 查看结果

```bash
orchestrator task list --project scan
orchestrator task logs <task_id>
```

### 4. 验证 Trigger 已注册

```bash
orchestrator get triggers --project scan
```

Cron trigger `weekly-scan` 会在每周一凌晨 3:00 UTC 自动创建新任务。

## 工作流步骤

```
audit (scan-agent) — 单步骤，fixed loop 1 cycle
```

### 核心特性：Trigger

```yaml
kind: Trigger
metadata:
  name: weekly-scan
spec:
  cron:
    schedule: "0 3 * * 1"    # 每周一凌晨 3:00
    timezone: "UTC"
  action:
    workflow: scheduled_scan
    workspace: default
    goal: "Weekly automated security audit"
    start: true              # 创建后自动启动
  concurrency_policy: Forbid  # 防止重叠执行
```

- `cron.schedule` — 标准 cron 表达式
- `cron.timezone` — 时区控制
- `action.start: true` — 任务创建后自动开始执行
- `concurrency_policy: Forbid` — 若上一次任务尚未完成，跳过本次触发

## 自定义指南

### 调整执行频率

```yaml
# 每天凌晨 2:00
schedule: "0 2 * * *"

# 每 6 小时
schedule: "0 */6 * * *"

# 每月 1 日
schedule: "0 0 1 * *"
```

### 替换为真实扫描 Agent

将 echo command 替换为真实 agent，并自定义 StepTemplate prompt：

```yaml
# Agent
command: claude -p "{prompt}" --verbose --output-format stream-json

# StepTemplate prompt — 根据项目技术栈定制
prompt: >-
  Run `cargo audit` to check for known vulnerabilities.
  Run `rg -n 'password|secret|api_key' --type rust` to find hardcoded secrets.
  Report all findings with severity levels.
```

### 添加 Webhook Trigger

除了 cron，还可以通过 webhook 事件触发：

```yaml
kind: Trigger
metadata:
  name: on-push-scan
spec:
  source: webhook
  filter: "payload.ref == 'refs/heads/main'"
  action:
    workflow: scheduled_scan
    workspace: default
    goal: "Post-push security scan"
    start: true
```

## 进阶参考

- [密钥轮替](/zh/showcases/secret-rotation-workflow) — 生产级 cron trigger 示例（密钥轮替）
- [内容推广](/zh/showcases/promotion-execution) — 另一个 cron 触发的 workflow（内容推广）
- [高级特性](/zh/guide/advanced-features) — Trigger 资源详解
