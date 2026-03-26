# Design Doc 91: Filesystem Trigger

## Origin

FR-085 — Filesystem Trigger：文件系统变更原生触发器

## Problem

文件系统变更是 SDLC 自动化中最常见的事件源（新增 FR 文档、配置文件修改、代码提交后的文件变动），但 orchestrator 只能通过外部 fswatch + curl → webhook 的胶水层桥接，增加部署复杂度和故障面。

## Design Decisions

### 1. `event.source: filesystem` 作为一等事件源

与 `cron`、`task_completed`、`webhook` 同级，复用已有的 trigger 引擎匹配逻辑和 CEL filter 评估。不引入新的引擎分支——filesystem 事件通过 `TriggerEventPayload` 广播到同一条事件通道。

### 2. 核心只提供原子事件变量，过滤逻辑由 CEL 承载

不在 `filesystem` 配置中引入 glob 参数。Glob 匹配等价于 `payload_filename.matches(regex)`，复用 CEL filter 即可。核心只暴露 5 个原子变量：

| 变量 | 说明 |
|------|------|
| `payload_path` | 变更文件完整路径 |
| `payload_filename` | 文件名（不含目录） |
| `payload_dir` | 所在目录 |
| `payload_event_type` | `create` / `modify` / `delete` |
| `payload_timestamp` | ISO 时间戳 |

### 3. 按需启停（Lazy Watcher Lifecycle）

`FsWatcher` 仅在存在至少一个有效（未 suspend）的 `source: filesystem` trigger 时才创建 `notify::RecommendedWatcher`。零 filesystem trigger = 零 fd/线程/内存开销。

生命周期：
- Apply 首个 fs trigger → 初始化 watcher
- Apply 后续 trigger → 增量添加路径
- Suspend/delete 最后一个 fs trigger → 释放 watcher
- Config reload → 差量更新 watched paths

### 4. 分层事件过滤

四层防护避免 agent 批量写文件时的事件风暴：

1. **路径白名单**（manifest `filesystem.paths`）：仅监听声明的目录
2. **事件类型收窄**（manifest `filesystem.events`）：如仅 `create`，忽略 `modify`
3. **防抖**（manifest `filesystem.debounce_ms` + notify 内置去重）
4. **CEL 精确过滤**（manifest `filter.condition`）：最终决定是否触发 task

### 5. 安全约束（硬编码不变量）

- 路径必须在 Workspace `root_path` 内（canonicalize 比较）
- 自动排除 `.git/` 和 `ORCHESTRATORD_DATA_DIR`
- 自动跳过隐藏文件（`.` 开头）

### 6. 跨平台兼容

使用 `notify` crate v7（macOS FSEvents、Linux inotify）。macOS 上 `/var` → `/private/var` 符号链接通过 `canonicalize()` 解决。

### 7. 复用现有基础设施

- **CEL 评估**：`evaluate_webhook_filter` 通用于所有 payload JSON，无需修改
- **触发引擎**：`handle_event_trigger` 按 source 字符串匹配，天然支持新 source
- **数据库**：复用 `trigger_state` 表，无 schema 变更
- **Config reload**：`notify_trigger_reload` 同时通知 trigger engine 和 fs_watcher

## Files

### New
- `crates/daemon/src/fs_watcher.rs` — FsWatcher 模块（~250 行）

### Modified
- `crates/daemon/Cargo.toml` — 添加 `notify` + `notify-debouncer-full` + `chrono`
- `crates/daemon/src/main.rs` — FsWatcher 初始化
- `crates/orchestrator-config/src/cli_types.rs` — `TriggerFilesystemSpec`
- `crates/orchestrator-config/src/config/trigger.rs` — `TriggerFilesystemConfig`
- `core/src/resource/trigger.rs` — 验证 + Spec↔Config 转换 + 5 个单元测试
- `core/src/state.rs` — `fs_watcher_reload_tx`
- `core/src/trigger_engine.rs` — `notify_trigger_reload` 扩展 + 文档更新
