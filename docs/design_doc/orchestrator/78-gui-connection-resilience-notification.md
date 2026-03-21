# DD-078: GUI 连接韧性与系统通知

## 概述

实现 GUI 的连接弹性和用户通知能力，覆盖四个维度：连接向导引导 UI、自动重连机制、OS 级系统通知、gRPC 错误人性化映射。

## 设计决策

### 1. 错误人性化（Error Humanization）

**方案**: 新增 `crates/gui/src/errors.rs` 模块，提供 `humanize_grpc_error(status: &Status) -> String`，将 `tonic::Code` 映射为中文用户提示。

所有 `commands/*.rs` 中的 `.map_err(|e| e.message().to_string())` 统一替换为 `.map_err(|e| crate::errors::humanize_grpc_error(&e))`。

映射表：

| gRPC Code | 用户提示 |
|-----------|---------|
| `Unavailable` | 无法连接到服务器，请检查 daemon 是否运行 |
| `PermissionDenied` | 权限不足，需要更高级别的访问权限 |
| `NotFound` | 未找到请求的资源 |
| `InvalidArgument` | 输入内容不符合要求: {detail} |
| `DeadlineExceeded` | 操作超时，请稍后重试 |
| 其他 | 操作失败，请稍后重试 |

### 2. 连接状态机（Connection State Machine）

**方案**: 在 `AppState` 中引入 `ConnectionState` 枚举和后台心跳任务。

```
ConnectionState: Disconnected → Connecting → Connected ⇄ Reconnecting → Failed
```

- `connection_state: Arc<RwLock<ConnectionState>>` 存储当前状态
- `app_handle: Arc<RwLock<Option<AppHandle>>>` 用于事件发射
- 通过 `connection-state-changed` Tauri 事件通知前端

### 3. 自动重连（Auto-Reconnect Heartbeat）

**方案**: `start_heartbeat()` 在 `tauri::async_runtime::spawn` 中运行后台循环：

- 每 5 秒 ping 一次 daemon
- 检测到失败后尝试 3 次重连（1s 间隔）
- 重连期间发射 `Reconnecting { attempt, max_attempts }` 状态
- 重连成功发射 `Connected`，失败发射 `Failed`

### 4. 前端连接 UI

**ConnectionBanner 组件**: 固定在视口顶部的横幅
- 黄色（重连中）→ 绿色（已恢复，3s 自动消失）→ 红色（失败 + 重试按钮）

**ConnectionStatus 连接向导**: 替代原来的一行文字
- 三项诊断清单：守护进程未启动 / 连接地址不正确 / 远程证书问题
- [重试连接] + [手动配置] 按钮
- 手动配置支持输入 control-plane 配置文件路径

**useConnectionState Hook**: 监听 `connection-state-changed` 事件，暴露 `connectionState` 和 `reconnect()`

### 5. 系统通知（OS Notifications）

**方案**: 通过 `tauri-plugin-notification` 在 TaskWatch 流中检测任务状态转换并推送。

触发条件：
- 任务完成 → "任务完成：「{name}」已成功完成"
- 任务失败 → "任务失败：「{name}」执行失败"
- 许愿池 FR 草稿就绪（project_id == "wish-pool" && completed）→ "FR 草稿就绪"

前端启动时请求通知权限（`isPermissionGranted` / `requestPermission`）。

### 6. 流式连接韧性

**方案**: 流式 RPC 错误不再静默 `break`，改为发射 `stream-error-{key}` Tauri 事件传递人性化错误消息。前端 `useStream` Hook 新增 `error` 状态和 `errorEventName` 参数。

## 文件变更

| 文件 | 变更 |
|------|------|
| `crates/gui/src/errors.rs` | 新增 — gRPC 错误映射 |
| `crates/gui/src/state.rs` | ConnectionState 枚举、心跳任务、AppHandle |
| `crates/gui/src/lib.rs` | 注册 errors 模块、notification 插件、setup 心跳 |
| `crates/gui/src/commands/*.rs` | humanize_grpc_error 替换、Arc<AppState> 类型 |
| `crates/gui/src/commands/stream.rs` | 通知触发、流错误发射 |
| `crates/gui/Cargo.toml` | 添加 tauri-plugin-notification |
| `crates/gui/capabilities/default.json` | notification 权限 |
| `gui/package.json` | 添加 @tauri-apps/plugin-notification |
| `gui/src/App.tsx` | useConnectionState 集成、通知权限请求 |
| `gui/src/hooks/useConnectionState.ts` | 新增 — 连接状态 Hook |
| `gui/src/components/ConnectionBanner.tsx` | 新增 — 重连横幅 |
| `gui/src/pages/ConnectionStatus.tsx` | 重写为连接向导 |
| `gui/src/hooks/useStream.ts` | 流错误回调支持 |
| `gui/src/lib/types.ts` | ConnectionState 类型定义 |
| `gui/src/styles/tokens.css` | banner 样式 |

## 关联文档

- 连接向导设计原型：DD-073 §3.1
- 通知设计原型：DD-074 §6.2
- FR：FR-068
