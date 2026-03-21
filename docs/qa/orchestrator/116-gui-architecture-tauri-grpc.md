# QA-116: GUI 架构 — Tauri 2.x + gRPC 安全客户端

**关联设计**: `docs/design_doc/orchestrator/73-gui-architecture-tauri-grpc.md`
**关联 FR**: FR-063（已闭环）

## 验证场景

### 场景 1: Cargo Workspace 集成

**目标**: GUI crate 已正确集成到 Cargo workspace，编译通过。

**步骤**:
1. 确认 `Cargo.toml` workspace members 包含 `crates/gui`
2. 执行 `cargo build -p orchestrator-gui`
3. 确认编译成功，无 warning（proto 编译信息除外）

**期望**: 编译通过，生成 `orchestrator-gui` 二进制。

---

### 场景 2: gRPC 连接发现（mTLS）

**目标**: GUI 复用与 CLI 一致的连接发现优先级链。

**步骤**:
1. 确认 `crates/gui/src/client.rs` 包含 5 级连接发现：
   - `ORCHESTRATOR_SOCKET` env → UDS
   - 显式配置 → TCP/TLS
   - 本地 `data/orchestrator.sock` → UDS
   - `~/.orchestrator/control-plane/config.yaml` → TCP/TLS
   - 回退 → UDS
2. 确认 mTLS 配置使用 `ClientTlsConfig` + CA cert + client identity
3. 确认 `MAX_GRPC_DECODE_SIZE = 64 MB`

**期望**: 连接逻辑与 `crates/cli/src/client.rs` 一致，无需手动配置证书路径。

---

### 场景 3: Ping RPC 执行

**目标**: GUI 通过 gRPC 连接到 daemon 并执行 `Ping`。

**前置**: daemon 已运行。

**步骤**:
1. GUI 启动后自动调用 `connect` 命令
2. 调用 `ping` Tauri command
3. 返回 `PingInfo { version, git_hash, uptime_secs }`

**期望**: 返回正确的 daemon 版本信息。

---

### 场景 4: TaskList RPC 执行

**目标**: GUI 可列出所有任务。

**前置**: daemon 已运行，存在至少一个任务。

**步骤**:
1. 调用 `task_list` Tauri command
2. 返回 `Vec<TaskSummary>` 包含 id, name, status, total_items 等

**期望**: 返回与 `orchestrator task list` CLI 一致的任务列表。

---

### 场景 5: TaskFollow 流式日志

**目标**: Streaming RPC 实时推送日志，延迟 < 2s。

**前置**: daemon 已运行，有正在执行的任务。

**步骤**:
1. 调用 `start_task_follow` with `task_id`
2. 监听 Tauri 事件 `task-follow-{task_id}`
3. 确认每条日志 `LogLine { line, timestamp }` 通过事件推送
4. 调用 `stop_task_follow` 停止流

**期望**: 日志在 daemon 产出后 2s 内到达前端。

---

### 场景 6: RBAC 角色探测与 UI 可见性

**目标**: GUI 正确探测用户角色并控制功能可见性。

**步骤**:
1. UDS 连接：调用 `probe_role`，期望返回 `"admin"`
2. TLS 连接（admin 证书）：期望返回 `"admin"`
3. TLS 连接（operator 证书）：期望返回 `"operator"`
4. TLS 连接（read_only 证书）：期望返回 `"read_only"`
5. 确认前端根据角色条件渲染：
   - `read_only`: 无操作按钮
   - `operator`: 显示 Pause/Resume 按钮
   - `admin`: 显示 Delete 按钮

**期望**: 角色正确映射到 UI 功能可见性。

---

### 场景 7: 安全凭证自动发现

**目标**: 用户无需手动配置证书路径。

**步骤**:
1. 确保 `~/.orchestrator/control-plane/config.yaml` 存在且配置正确
2. 启动 GUI（不传任何配置参数）
3. GUI 自动发现配置并建立 mTLS 连接

**期望**: 零配置连接成功。

---

## 架构验证清单

| 验证点 | 方法 | 状态 |
|--------|------|------|
| Tauri 2.x 框架 | `crates/gui/Cargo.toml` 依赖 `tauri = "2"` | ✅ |
| Rust 后端原生 gRPC | `tonic` 0.14 直连 daemon | ✅ |
| Proto 复用 | 依赖 `orchestrator-proto` crate | ✅ |
| mTLS 支持 | `ClientTlsConfig` + CA + Identity | ✅ |
| Streaming → 事件桥接 | `app.emit()` + `CancellationToken` | ✅ |
| RBAC 探测 | 无 proto/daemon 修改，probe 策略 | ✅ |
| 前端 React + Vite + TS | `gui/` 目录 | ✅ |
| Liquid Glass 设计系统 | `gui/src/styles/tokens.css` | ✅ |
| Workspace 集成 | `cargo build -p orchestrator-gui` 编译通过 | ✅ |
