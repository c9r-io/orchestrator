# FR-085: Filesystem Trigger — 文件系统变更原生触发器

## 优先级: P1

## 状态: In Progress

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
    filter:
      condition: "payload_event_type == 'create' && payload_filename.matches('^FR-.*\\\\.md$')"
  action:
    workflow: fr_governance
    workspace: default
    start: true
  concurrencyPolicy: Forbid
```

> **设计原则：核心只提供原子事件变量，过滤逻辑统一由 CEL 承载。** 不在 `filesystem` 配置中引入 glob 参数 — glob 匹配等价于 `event.filename.matches(regex)`，复用已有的 CEL filter 通道即可，避免核心 API 面膨胀。

### 2. Daemon 内嵌文件监控（按需启停）

- 使用 `notify` crate（跨平台：macOS FSEvents、Linux inotify、Windows ReadDirectoryChanges）
- **零 filesystem trigger = 零开销**：daemon 仅在存在至少一个有效（未 suspend）的 `source: filesystem` trigger 时才创建 `notify::Watcher` 实例。无 filesystem trigger 时不占用任何 fd、线程或内存
- **动态生命周期管理**：
  - `apply` 首个 filesystem trigger → 初始化 watcher，注册监控路径
  - `apply` 后续 trigger → 增量添加路径到现有 watcher
  - `delete` / `suspend` 最后一个 filesystem trigger → 完全释放 watcher 资源
  - `resume` 已暂停的 trigger → 按需重建 watcher
- 监控路径相对于 Workspace 的 `root_path` 解析
- 多个 trigger 监控相同路径时共享 watcher，去重处理

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

### 5. 安全约束与监听范围控制

监听范围通过安全约束严格限定，防止 agent 批量文件操作时产生事件风暴：

- **路径白名单**：`filesystem.paths` 显式声明监控目录，仅监听指定路径（非递归到整个 `root_path`）
- **root_path 围栏**：所有 `paths` 必须在 Workspace `root_path` 内（或等于 `root_path`），拒绝任意系统路径
- **内置排除**：自动排除 `.git/`（高频变更，无业务意义）和 daemon 数据目录（`ORCHESTRATORD_DATA_DIR`）
- **事件类型收窄**：`filesystem.events` 只订阅需要的事件类型（如仅 `create`），忽略 agent 写文件产生的大量 `modify` 事件
- **防抖兜底**：即使上述约束未完全过滤，`debounce_ms` 保证同文件的密集事件合并为一次触发
- **CEL 精确过滤**：最终由 CEL filter 决定是否触发 task，如 `event.filename.matches('^FR-.*\\.md$') && event.event_type == 'create'`

这套分层防护确保：agent 在 `src/` 下批量修改 100 个文件时，仅监控 `docs/feature_request/` 的 trigger 不会产生任何事件。

## 验收标准

- [x] `event.source: filesystem` 通过 manifest validate
- [x] 无 filesystem trigger 时 daemon 不创建 watcher（零开销）
- [x] apply 首个 filesystem trigger 后 watcher 启动
- [ ] 在监控目录创建匹配文件时自动创建 task（需 daemon 集成测试验证）
- [x] CEL filter 可访问 `payload_path`、`payload_filename`、`payload_event_type`
- [ ] 防抖机制生效：500ms 内同文件多次事件只触发一次（需 daemon 集成测试验证）
- [x] 路径安全约束：拒绝 `root_path` 外的路径
- [x] `trigger suspend/resume` 正确暂停/恢复文件监控；suspend 最后一个 trigger 释放 watcher
- [x] Trigger 删除时清理 watcher 资源
- [x] Agent 在非监控目录批量写文件时不产生任何 filesystem 事件

## 风险

- **跨平台差异**：macOS FSEvents 和 Linux inotify 行为不完全一致（如递归监控、事件粒度）。`notify` crate 抽象了大部分差异，但需关注边界情况。
- **编辑器临时文件**：vim/emacs 等编辑器的 swap 文件和备份文件可能触发误报。CEL filter（如 `!event.filename.startsWith('.')`）可缓解。
