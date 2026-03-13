# Design Doc 51: Trigger Resource — Cron & Event-Driven Task Creation

**关联 FR**: FR-039
**状态**: Implemented
**日期**: 2026-03-14

---

## 1. 问题

orchestrator 缺乏被动触发机制：所有 task 必须手动通过 `orchestrator task create` 创建。定期任务依赖外部 crontab，任务间无法编排因果链（workflow A 完成后触发 workflow B）。

## 2. 设计决策

### 统一 Trigger 资源

Cron 和 Event 触发条件不同但动作端相同（创建 task），统一为一个 `Trigger` 资源类型，遵循 K8s CronJob 心智模型。

### TriggerEngine 作为 daemon 子任务

TriggerEngine 是 `orchestratord` 内的一个 long-running tokio task，与 daemon 同生命周期。使用 `tokio::select!` 多路复用四个事件源：

```
tokio::select! {
    cron_tick     => fire nearest cron trigger
    event_rx      => match event triggers
    reload_rx     => rebuild cron schedule from config
    shutdown_rx   => graceful exit
}
```

### 轻量事件广播

`TriggerEventPayload` 仅携带 `event_type` + `task_id`（不含 workflow/workspace），engine 在匹配时按需从 DB 查询。减少 broadcast channel 的内存开销。

### 同步 hot-reload

`TriggerEngineHandle` 提供 `reload_sync()` 方法（基于 `mpsc::try_send`），使 `apply_manifests` 和 `delete_resource` 等同步代码路径可以通知 engine 重新加载配置。

## 3. 关键模块

| 文件 | 职责 |
|------|------|
| `core/src/cli_types.rs` | `TriggerSpec`, `ConcurrencyPolicy`, `TriggerCronSpec`, `TriggerEventSpec` 等类型定义 |
| `core/src/config/trigger.rs` | 存储层 `TriggerConfig` 类型 |
| `core/src/resource/trigger.rs` | `TriggerResource` validate/apply/get/delete/to_yaml |
| `core/src/trigger_engine.rs` | `TriggerEngine` 核心：cron ticker + event listener + fire logic |
| `core/src/state.rs` | `trigger_event_tx` broadcast channel + `trigger_engine_handle` |
| `core/src/service/resource.rs` | `suspend_trigger()`, `resume_trigger()`, `fire_trigger()` |
| `core/src/scheduler/check/mod.rs` | preflight trigger reference integrity check |
| `core/src/persistence/migration_steps.rs` | `m0018_trigger_state` 表 |

## 4. ConcurrencyPolicy 实现

```rust
match trigger.concurrency_policy {
    Allow  => { /* 直接创建 */ }
    Forbid => { if has_active_task() { skip; return } }
    Replace => { cancel_active_tasks(); /* then create */ }
}
```

- `has_active_task()` 查询 trigger_state.last_task_id 的 tasks.status 是否为 active 状态
- `cancel_active_tasks()` 调用 `stop_task_runtime()` 终止旧 task

## 5. Cron 调度

- 使用 `cron` crate 解析标准 cron 表达式（6 段格式：秒 分 时 日 月 周）
- `chrono-tz` 支持 IANA 时区
- daemon 重启后从配置重建 cron schedule（`build_cron_schedule()`）

## 6. Event 触发

- `InnerState::emit_event()` 在 `task_completed` / `task_failed` 时广播 `TriggerEventPayload`
- `handle_event_trigger()` 匹配 event.source + filter.workflow
- CEL condition 评估为 Phase 2 保留（当前设置 condition 时 skip + 日志警告）

## 7. 历史清理

`cleanup_history()` 在每次 trigger_fired 后异步执行：
- 查询 tasks 表中 name = "trigger-{name}" 且 project_id 匹配的 completed/failed tasks
- 按 created_at DESC 排序，保留 historyLimit 内的，删除超出的

## 8. Preflight Check 集成

`run_checks()` 新增 trigger 引用完整性验证：
- `trigger_workflow_ref`: action.workflow 必须存在于当前 project
- `trigger_workspace_ref`: action.workspace 必须存在于当前 project

## 9. 未来扩展点

- Phase 2: Webhook 事件源 (`POST /api/v1/webhook/:trigger_name`)
- Phase 3: 文件系统监听 (`fs_watch` source via `notify` crate)
- CEL condition 评估（复用 `core/src/prehook/cel.rs`）
