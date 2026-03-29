# Scheduled Scan 模板

> **Harness Engineering 模板**：这个 showcase 展示 orchestrator 作为 agent-first 软件交付控制面的一个能力切片，把 agent、workflow、policy 和反馈闭环固化为可复用的工程资产。
>
> **模板用途**：定时触发的安全审计 — 展示 Trigger 资源、cron 调度和 agent 驱动的安全分析。

## 适用场景

- 定期安全审计：agent 驱动的威胁建模 + 静态工具扫描
- 合规检查：定期审查架构安全性和依赖健康度
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
agent_audit (scan-agent) → static_check (scan-agent)
```

1. **agent_audit** — Agent 驱动的安全分析：识别信任边界、审查认证授权逻辑、检查注入向量、评估密钥处理和错误暴露
2. **static_check** — 静态工具扫描：依赖审计（cargo audit / npm audit）、秘密扫描、已知漏洞模式检查

### 为什么 Agent 分析优先？

传统静态扫描只能发现已知模式（CVE、正则匹配），而 AI agent 能：
- 理解业务逻辑中的安全隐患（如权限绕过、TOCTOU 竞态）
- 进行威胁建模（识别信任边界和攻击面）
- 给出上下文感知的修复建议

静态扫描作为补充，覆盖 agent 可能遗漏的机械性检查（依赖 CVE、硬编码凭证正则匹配等）。

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

### 替换为真实 Agent

将 echo command 替换为真实 agent：

```yaml
command: claude -p "{prompt}" --verbose --output-format stream-json
```

替换后 agent 将实际执行威胁建模分析和静态扫描命令。

### 自定义 StepTemplate prompt

根据项目技术栈调整 static_check 的 prompt：

```yaml
prompt: >-
  Run static security checks:
  - `cargo audit` for Rust dependency CVEs
  - `rg -n 'password|secret|api_key' --type rust` for hardcoded secrets
  - Check for `unsafe` blocks without safety comments
```

### 添加 Webhook Trigger

除了 cron，还可以通过 webhook 事件触发（如 CI push 后自动扫描）：

```yaml
kind: Trigger
metadata:
  name: on-push-scan
spec:
  event:
    source: webhook
    filter: "payload.ref == 'refs/heads/main'"
  action:
    workflow: scheduled_scan
    workspace: default
    goal: "Post-push security scan"
    start: true
```

## 进阶参考

- [FR Watch 模板](fr-watch.md) — Webhook Trigger 示例（文件监控驱动）
- [Secret Rotation Workflow](secret-rotation-workflow.md) — 生产级 cron trigger 示例
- [Advanced Features](../guide/05-advanced-features.md) — Trigger 资源详解
