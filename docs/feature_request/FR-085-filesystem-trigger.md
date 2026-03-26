# FR-085: Filesystem Trigger — 文件系统变更原生触发器

## 优先级: P1

## 状态: Proposed

## 背景

当前 Trigger 资源支持 3 种 event source：`cron`、`task_completed`/`task_failed`、`webhook`。文件系统变更（如新增 FR 文档、配置文件修改、代码提交后的文件变动）是 SDLC 自动化中最常见的事件源之一，但目前只能通过外部 fswatch + curl → webhook 的胶水层桥接，增加了部署复杂度和故障面。

Filesystem trigger 作为 daemon 原生能力，可以消除外部依赖，让"文件变更 → workflow 触发"成为一等公民。

## 需求

### 1. 新增 `event.source: filesystem` 触发器类型

```yaml
apiVersion: orchestrator.dev/v2
kind: Trigger
metadata:
  name: fr-file-watch
spec:
  event:
    source: filesystem
    filesystem:
      paths:
        - docs/feature_request/
      events:
        - create            # 文件创建
        - modify            # 文件修改（可选）
        - delete            # 文件删除（可选）
      debounce_ms: 500      # 防抖窗口（默认 500ms）
    filter: "event.filename.matches('^FR-.*\\\\.md$')"  # CEL 过滤：文件名匹配
  action:
    workflow: fr_governance
    workspace: default
    goal: "Triage newly created feature request: {event.filename}"
    start: true
  concurrency_policy: Forbid
```

> **设计原则：核心只提供原子事件变量，过滤逻辑统一由 CEL 承载。** 不在 `filesystem` 配置中引入 glob 参数 — glob 匹配等价于 `event.filename.matches(regex)`，复用已有的 CEL filter 通道即可，避免核心 API 面膨胀。

### 2. Daemon 内嵌文件监控

- 使用 `notify` crate（跨平台：macOS FSEvents、Linux inotify、Windows ReadDirectoryChanges）
- Daemon 启动时根据已注册的 filesystem trigger 建立 watcher
- Trigger apply/delete 时动态增减 watcher
- 监控路径相对于 Workspace 的 `root_path` 解析

### 3. 事件上下文注入

filesystem 事件应将以下信息注入 CEL filter 和 action 模板变量：

| 变量 | 说明 |
|------|------|
| `event.path` | 变更文件的完整路径 |
| `event.filename` | 文件名（不含目录） |
| `event.event_type` | `create` / `modify` / `delete` |
| `event.dir` | 所在目录 |
| `event.timestamp` | 事件时间戳 |

### 4. 防抖（Debounce）

文件系统事件通常成批到达（编辑器保存会触发多次 write 事件）。`filesystem.debounce_ms` 提供可配置的防抖窗口（默认 500ms），合并同文件的重复事件为一次触发。

### 5. 安全约束

- 监控路径必须在 Workspace `root_path` 内（或等于 `root_path`），禁止监控任意系统路径
- 不监控 `.git/` 目录（高频变更，无业务意义）
- 不监控 daemon 自身的数据目录（`ORCHESTRATORD_DATA_DIR`）

## 验收标准

- [ ] `event.source: filesystem` 通过 manifest validate
- [ ] Daemon 启动后对 registered filesystem trigger 建立 watcher
- [ ] 在监控目录创建匹配文件时自动创建 task
- [ ] CEL filter 可访问 `event.path`、`event.filename`、`event.event_type`
- [ ] 防抖机制生效：500ms 内同文件多次事件只触发一次
- [ ] 路径安全约束：拒绝 `root_path` 外的路径
- [ ] `trigger suspend/resume` 正确暂停/恢复文件监控
- [ ] Trigger 删除时清理 watcher 资源

## 风险

- **跨平台差异**：macOS FSEvents 和 Linux inotify 行为不完全一致（如递归监控、事件粒度）。`notify` crate 抽象了大部分差异，但需关注边界情况。
- **大目录性能**：递归监控大型目录树可能消耗 fd/内存。应限制递归深度或单 trigger 最大监控路径数。
- **编辑器临时文件**：vim/emacs 等编辑器的 swap 文件和备份文件可能触发误报。CEL filter（如 `!event.filename.startsWith('.')`）可缓解。
