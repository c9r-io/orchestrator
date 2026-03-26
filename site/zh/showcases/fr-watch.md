# FR Watch 模板

> **模板用途**：监控 FR 文档创建，webhook 触发 FR 治理流程 — 展示 webhook Trigger 和 CEL payload 过滤。

## 适用场景

- 新 Feature Request 文档落入 `docs/feature_request/` 时自动触发分诊和规划
- 将文件系统事件（fswatch / inotifywait / GitHub Action）接入 orchestrator 自动化
- 任何需要"文件变更 → webhook → 任务"链路的事件驱动场景

## 前置条件

- `orchestratord` 运行中，且启用 `--webhook-bind 127.0.0.1:9090`
- 已执行 `orchestrator init`

## 使用步骤

### 1. 部署资源

```bash
orchestrator apply -f docs/workflow/fr-watch.yaml --project fr-watch
```

### 2. 模拟新 FR 创建事件

```bash
# 方式 A：CLI 直接触发
orchestrator trigger fire fr-file-created --project fr-watch \
  --payload '{"file":"docs/feature_request/FR-099-new-feature.md"}'

# 方式 B：curl 模拟 webhook
curl -X POST http://127.0.0.1:9090/webhook/fr-file-created \
  -H "Content-Type: application/json" \
  -d '{"file":"docs/feature_request/FR-099-new-feature.md"}'
```

### 3. 查看结果

```bash
orchestrator task list --project fr-watch
orchestrator task logs <task_id>
```

### 4. 接入真实文件监控（可选）

```bash
# macOS: fswatch
fswatch -0 docs/feature_request/ | while read -d '' file; do
  [[ "$file" == *FR-*.md ]] && \
  curl -X POST http://127.0.0.1:9090/webhook/fr-file-created \
    -H "Content-Type: application/json" \
    -d "{\"file\":\"$file\"}"
done

# Linux: inotifywait
inotifywait -m docs/feature_request/ -e create --format '%w%f' | while read file; do
  [[ "$file" == *FR-*.md ]] && \
  curl -X POST http://127.0.0.1:9090/webhook/fr-file-created \
    -H "Content-Type: application/json" \
    -d "{\"file\":\"$file\"}"
done
```

## 工作流步骤

```
fr_triage (fr-governance-agent) → fr_plan (fr-governance-agent)
```

1. **fr_triage** — 分诊新 FR：评估优先级、复杂度、依赖关系
2. **fr_plan** — 草拟实现计划：拆解任务、识别模块、定义验收标准

### 核心特性：Webhook Trigger + CEL 过滤

```yaml
kind: Trigger
metadata:
  name: fr-file-created
spec:
  event:
    source: webhook
    filter: "has(payload.file) && payload.file.startsWith('docs/feature_request/FR-')"
  action:
    workflow: fr_governance
    workspace: default
    goal: "Triage and plan newly created feature request"
    start: true
  concurrency_policy: Forbid
```

- `event.source: webhook` — 接受 HTTP POST 事件
- `filter` — CEL 表达式过滤 payload，只有 FR 文件路径才触发
- `concurrency_policy: Forbid` — 防止多个 FR 同时治理时冲突

### 与 Cron Trigger 的区别

| 特性 | Cron (scheduled-scan) | Webhook (fr-watch) |
|------|----------------------|-------------------|
| 触发方式 | 定时 | 事件驱动 |
| 适用场景 | 周期性任务 | 响应性任务 |
| 延迟 | 取决于调度间隔 | 实时 |
| 外部依赖 | 无 | 需要事件源（fswatch/CI/API） |

## 自定义指南

### 修改 CEL 过滤条件

```yaml
# 只监控 P0 优先级的 FR（需 payload 包含 priority 字段）
filter: "has(payload.priority) && payload.priority == 'P0'"

# 监控任意 markdown 文件创建
filter: "has(payload.file) && payload.file.endsWith('.md')"
```

### 替换为真实 Agent

参见 [Hello World 自定义指南](/zh/showcases/hello-world#替换为真实-agent)。使用真实 agent 后，fr_triage 步骤将实际读取 FR 文档并产出分诊报告。

### 添加 HMAC 签名验证

生产环境建议启用 webhook 签名验证：

```yaml
spec:
  event:
    source: webhook
    webhook:
      secret:
        fromRef: webhook-signing-keys
      signatureHeader: X-Webhook-Signature
    filter: "has(payload.file) && payload.file.startsWith('docs/feature_request/FR-')"
```

## 进阶参考

- [密钥轮替](/zh/showcases/secret-rotation-workflow) — 另一个 Trigger 驱动的 workflow
- [Scheduled Scan 模板](/zh/showcases/scheduled-scan) — Cron Trigger 示例
- [高级特性](/zh/guide/advanced-features) — Trigger 资源详解
