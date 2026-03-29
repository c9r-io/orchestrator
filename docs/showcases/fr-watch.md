# FR Watch 模板

> **Harness Engineering 模板**：这个 showcase 展示 orchestrator 作为 agent-first 软件交付控制面的一个能力切片，把 agent、workflow、policy 和反馈闭环固化为可复用的工程资产。
>
> **模板用途**：监控 FR 文档创建，原生文件系统触发 FR 治理流程 — 展示 filesystem Trigger 和 CEL payload 过滤。

## 适用场景

- 新 Feature Request 文档落入 `docs/feature_request/` 时自动触发分诊和规划
- 任何需要"文件变更 → 自动任务"的事件驱动场景
- 无需外部工具（fswatch / curl）— daemon 原生监控文件系统

## 前置条件

- `orchestratord` 运行中
- 已执行 `orchestrator init`
- 项目目录下有 `docs/feature_request/`

## 使用步骤

### 1. 部署资源

```bash
orchestrator apply -f docs/workflow/fr-watch.yaml --project fr-watch
```

### 2. 触发：创建一个 FR 文件

```bash
echo "# FR-099: New Feature" > docs/feature_request/FR-099-new-feature.md
```

Daemon 检测到文件创建，自动创建并启动 task。

### 3. 查看结果

```bash
orchestrator task list --project fr-watch
orchestrator task logs <task_id>
```

## 工作流步骤

```
fr_triage (fr-governance-agent) → fr_plan (fr-governance-agent)
```

1. **fr_triage** — 分诊新 FR：评估优先级、复杂度、依赖关系
2. **fr_plan** — 草拟实现计划：拆解任务、识别模块、定义验收标准

### 核心特性：Filesystem Trigger

```yaml
kind: Trigger
metadata:
  name: fr-file-created
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - docs/feature_request/
      events:
        - create
      debounce_ms: 500
    filter:
      condition: "payload_filename.endsWith('.md') && payload_filename.startsWith('FR-')"
  action:
    workflow: fr_governance
    workspace: default
    start: true
  concurrencyPolicy: Forbid
```

- `source: filesystem` — daemon 原生文件监控（macOS FSEvents / Linux inotify）
- `filesystem.paths` — 声明监控的目录（相对于 Workspace root_path）
- `filesystem.events` — 只监听 `create` 事件，忽略 modify/delete
- `debounce_ms: 500` — 500ms 防抖窗口，合并重复事件
- `filter.condition` — CEL 精确过滤：只有 `FR-*.md` 文件才触发

### CEL 变量（自动注入）

| 变量 | 说明 |
|------|------|
| `payload_path` | 变更文件完整路径 |
| `payload_filename` | 文件名（如 `FR-099-new-feature.md`） |
| `payload_dir` | 所在目录 |
| `payload_event_type` | `create` / `modify` / `delete` |
| `payload_timestamp` | 事件时间戳 |

### 与其他 Trigger 类型对比

| 特性 | Filesystem | Cron | Webhook |
|------|-----------|------|---------|
| 触发方式 | 文件变更 | 定时 | HTTP POST |
| 延迟 | 实时（~500ms 防抖） | 取决于调度间隔 | 实时 |
| 外部依赖 | 无 | 无 | 需要事件源 |
| 典型场景 | 文件监控 | 周期性任务 | CI/CD 集成 |

## 自定义指南

### 修改 CEL 过滤条件

```yaml
# 监控所有 markdown 文件创建
condition: "payload_filename.endsWith('.md')"

# 仅监控特定前缀
condition: "payload_filename.startsWith('ISSUE-')"

# 同时监控 create 和 modify
events:
  - create
  - modify
```

### 替换为真实 Agent

参见 [Hello World 自定义指南](hello-world.md#替换为真实-agent)。使用真实 agent 后，fr_triage 步骤将实际读取 FR 文档并产出分诊报告。

## 进阶参考

- [Scheduled Scan 模板](scheduled-scan.md) — Cron Trigger 示例
- [Secret Rotation Workflow](secret-rotation-workflow.md) — Webhook Trigger 示例
- [Advanced Features](../guide/05-advanced-features.md) — Trigger 资源详解
