# FR-068: GUI 连接韧性与系统通知

**状态**: Proposed

## 背景

当前 GUI 的连接管理和通知能力不足：

1. **连接失败只显示一行文字**：DD-073 设计了"连接向导"UI（引导确认 daemon 状态、选择连接方式），但未实现。
2. **无自动重连**：daemon 重启或网络中断后，GUI 保持断开状态，用户必须手动重启 GUI。
3. **无系统通知**：DD-074 设计了任务完成/失败/草稿就绪时的系统通知推送，未实现。
4. **错误提示过于技术化**：gRPC status message 直接暴露给非技术用户。

## 目标

1. 实现连接向导 UI，引导用户排查连接问题
2. 实现自动重连机制（3 次，1s 间隔），断开时顶栏显示"连接中断"横幅
3. 通过 Tauri Notification API 推送关键状态变更通知
4. 将 gRPC 错误信息转为非技术化的中文提示

## 设计方案

### 1. 连接向导

连接失败时显示引导式 UI：

```
┌─────────────────────────────────────┐
│  无法连接到 orchestratord           │
│                                     │
│  可能的原因：                        │
│  ● daemon 未启动                    │
│    → orchestratord --foreground     │
│  ● 连接地址不正确                    │
│    → 检查 ORCHESTRATOR_SOCKET 环境变量│
│  ● 远程连接证书问题                   │
│    → 检查 ~/.orchestrator/control-plane/ │
│                                     │
│  [重试连接]  [手动配置]               │
└─────────────────────────────────────┘
```

### 2. 自动重连

- 检测到 gRPC 调用失败（`Unavailable` / `ConnectionRefused`）时触发重连
- 重连策略：最多 3 次，间隔 1s
- 重连期间顶栏显示黄色横幅："连接中断，正在重连..."
- 重连成功：横幅消失，自动刷新当前页面数据
- 重连失败：横幅变红，显示"连接失败"+ 重试按钮

### 3. 系统通知

利用 Tauri `tauri-plugin-notification`：

| 事件 | 通知内容 | 触发条件 |
|------|---------|---------|
| 任务完成 | "「用户登录功能」开发完成" | TaskWatch snapshot.status → completed |
| 任务失败 | "「自动发送周报」执行失败" | TaskWatch snapshot.status → failed |
| FR 草稿就绪 | "你的需求方案已生成，等待确认" | 许愿任务 status → completed |

### 4. 错误信息人性化

建立 gRPC error code → 中文提示映射：

| gRPC Code | 用户提示 |
|-----------|---------|
| `Unavailable` | "无法连接到服务器，请检查 daemon 是否运行" |
| `PermissionDenied` | "权限不足，需要更高级别的访问权限" |
| `NotFound` | "未找到请求的资源" |
| `InvalidArgument` | "输入内容不符合要求，请检查后重试" |
| `DeadlineExceeded` | "操作超时，请稍后重试" |
| 其他 | "操作失败，请稍后重试" |

## 成功标准

1. 连接失败时显示连接向导（非一行文字），引导用户排查
2. daemon 重启后 GUI 在 5s 内自动重连并恢复数据
3. 任务完成/失败时收到系统通知（GUI 在后台时也能看到）
4. 所有 gRPC 错误信息均以中文非技术语言展示

## 约束

1. 需要添加 `tauri-plugin-notification` 依赖
2. 不修改 proto 或 daemon 代码
3. 重连逻辑在 GUI Rust 侧实现（AppState 层），不在前端轮询

## 关联文档

- 连接向导设计：`docs/design_doc/orchestrator/73-gui-architecture-tauri-grpc.md` §3.1
- 通知设计：`docs/design_doc/orchestrator/74-gui-uiux-wish-pool-progress.md` §6.2
