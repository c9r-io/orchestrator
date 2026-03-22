# DD-073: GUI 架构 — Tauri 2.x + gRPC 安全客户端

## 概述

为 orchestrator 新增 GUI 客户端，采用 Tauri 2.x 框架，Rust 后端直连 daemon gRPC 服务（mTLS），前端使用 React + Vite + TypeScript。本文档是 FR-063 的技术架构详细设计。

## 设计决策

### 1. 技术栈选型理由

选择 Tauri 2.x 基于以下关键因素：

**Rust 原生集成**：Tauri 后端是 Rust，可直接：
- 依赖 `orchestrator-proto` crate，获得全部 48 个 RPC 的类型安全客户端
- 使用 `tonic` 构建 gRPC channel，原生支持 mTLS
- 复用 `crates/cli/src/client.rs` 中的连接发现逻辑

**不选 Electron 的理由**：
- 需要 Node.js gRPC 绑定（`@grpc/grpc-js`），无法复用 Rust proto crate
- mTLS 配置需要通过 Node.js TLS API 重新实现
- 二进制体积 150MB+ 对桌面应用不合理

**不选纯 Web SPA 的理由**：
- gRPC-Web 不支持 server-streaming 的全部语义（无法复用 `TaskFollow` 等流式接口）
- 需要部署 envoy proxy 做 gRPC → gRPC-Web 转译，违反"不引入代理进程"约束
- 用户需要额外部署 Web 服务器

### 2. 通信架构详细设计

#### 2.1 Tauri Command 层

每个 Tauri command 是一个 async Rust 函数，负责：
1. 从 `AppState` 获取 gRPC client
2. 构建 tonic `Request`
3. 调用 RPC
4. 将 `Response` 序列化为前端可消费的 JSON

```rust
// 示例：task_list command
#[tauri::command]
async fn task_list(
    state: tauri::State<'_, AppState>,
    filter: Option<String>,
) -> Result<Vec<TaskSummary>, String> {
    let mut client = state.client().await?;
    let request = tonic::Request::new(TaskListRequest {
        filter: filter.unwrap_or_default(),
        ..Default::default()
    });
    let response = client.task_list(request).await.map_err(|e| e.to_string())?;
    Ok(response.into_inner().tasks.into_iter().map(TaskSummary::from).collect())
}
```

#### 2.2 Streaming 架构

Server-streaming RPC 通过 Tauri event system 桥接：

```rust
#[tauri::command]
async fn start_task_follow(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let mut client = state.client().await?;
    let request = tonic::Request::new(TaskFollowRequest {
        task_id: task_id.clone(),
        ..Default::default()
    });
    let stream = client.task_follow(request).await.map_err(|e| e.to_string())?;
    let mut stream = stream.into_inner();

    // 注册 cancel token
    let cancel = state.register_stream(&task_id);

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                msg = stream.message() => {
                    match msg {
                        Ok(Some(log_entry)) => {
                            let _ = app.emit(&format!("task-follow-{}", task_id), log_entry);
                        }
                        Ok(None) => break,  // stream ended
                        Err(_) => break,    // stream error
                    }
                }
                _ = cancel.cancelled() => break,
            }
        }
    });

    Ok(())
}
```

前端订阅：
```typescript
import { listen } from "@tauri-apps/api/event";

const unlisten = await listen<LogEntry>(`task-follow-${taskId}`, (event) => {
  appendLog(event.payload);
});

// 清理
onCleanup(() => unlisten());
```

#### 2.3 AppState 设计

```rust
pub struct AppState {
    /// gRPC channel（lazy init, 支持重连）
    channel: Arc<RwLock<Option<Channel>>>,
    /// 活跃 streaming 的 cancel token
    active_streams: Arc<RwLock<HashMap<String, CancellationToken>>>,
    /// 当前用户 RBAC 角色（连接后缓存）
    role: Arc<RwLock<Option<Role>>>,
}
```

### 3. 安全集成详细设计

#### 3.1 连接发现流程

与 CLI 完全一致的优先级链：

```
1. ORCHESTRATOR_SOCKET env → UDS 连接
2. --control-plane-config flag → TCP/TLS（Tauri 通过 tauri.conf.json 或启动参数传入）
3. data/orchestrator.sock 本地检测 → UDS
4. ~/.orchestrator/control-plane/config.yaml 自动发现 → TCP/TLS
5. fallback → UDS
```

GUI 专有增强：如果所有连接方式均失败，显示"连接向导"UI，引导用户：
- 确认 daemon 是否已启动
- 选择连接方式（本地 / 远程）
- 远程模式下输入 config.yaml 路径

#### 3.2 凭证管理

```
~/.orchestrator/control-plane/
├── config.yaml         # 连接配置（endpoint, cert paths）
├── ca.crt              # CA 证书
├── client.crt          # 客户端证书
└── client.key          # 客户端私钥
```

GUI Rust 侧解析 `config.yaml`，构建 TLS channel：

```rust
// 复用自 crates/cli/src/client.rs 的逻辑
let tls = ClientTlsConfig::new()
    .ca_certificate(Certificate::from_pem(&ca_pem))
    .identity(Identity::from_pem(&client_cert_pem, &client_key_pem))
    .domain_name(&server_name);

let channel = Channel::from_shared(endpoint)?
    .tls_config(tls)?
    .connect()
    .await?;
```

#### 3.3 RBAC 功能映射

GUI 启动后通过 `Ping` RPC 获取连接状态，通过尝试性调用或 daemon 扩展获取角色信息。角色缓存在 `AppState.role` 中，控制前端 UI 元素可见性：

| 角色 | 许愿池 | 进度观察 | 专家模式 |
|------|--------|---------|---------|
| read_only | 查看历史许愿（不可提交） | 查看列表/详情/日志 | 资源/Agent 只读 |
| operator | 提交/确认/取消许愿 | 暂停/恢复/重试/取消 | 资源编辑 |
| admin | 同 operator | 同 operator | Agent 管理（cordon/drain） |

### 4. Workspace 集成详细设计

#### 4.1 目录结构

```
crates/gui/
├── Cargo.toml
├── build.rs              # Tauri 构建脚本
├── tauri.conf.json       # Tauri 配置（窗口大小、标题、安全策略）
├── icons/                # 应用图标（各平台格式）
└── src/
    ├── main.rs           # Tauri app 初始化
    ├── state.rs          # AppState + gRPC channel 管理
    ├── client.rs         # gRPC 连接发现 + mTLS 配置
    └── commands/
        ├── mod.rs
        ├── task.rs       # TaskCreate/List/Info/Pause/Resume/Delete/Retry
        ├── resource.rs   # ResourceGet/Describe/Apply/Delete
        ├── agent.rs      # AgentList/Cordon/Uncordon/Drain
        ├── stream.rs     # TaskFollow/Watch/Logs streaming 管理
        ├── system.rs     # Ping/Check/WorkerStatus/ManifestValidate
        └── store.rs      # StoreGet/Put/Delete/List
```

#### 4.2 Cargo.toml 依赖

```toml
[package]
name = "orchestrator-gui"
version = "0.1.0"
edition = "2021"

[dependencies]
orchestrator-proto = { path = "../proto" }
tauri = { version = "2", features = ["rustls-tls"] }
tonic = { version = "0.14", features = ["tls-native-roots", "tls-ring"] }
tokio = { version = "1.44", features = ["full"] }
tokio-util = "0.7"          # CancellationToken
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[build-dependencies]
tauri-build = "2"
```

#### 4.3 前端目录结构

```
gui/
├── package.json
├── vite.config.ts
├── tsconfig.json
├── index.html
└── src/
    ├── main.tsx
    ├── App.tsx            # 顶级路由（Tab 切换）
    ├── pages/
    │   ├── WishPool.tsx       # 许愿池一级页面
    │   ├── WishDetail.tsx     # 许愿详情二级页面
    │   ├── ProgressList.tsx   # 进度观察一级页面
    │   ├── TaskDetail.tsx     # 任务详情二级页面
    │   └── ExpertPanel.tsx    # 专家模式面板
    ├── components/
    │   ├── GlassCard.tsx      # Liquid Glass 卡片
    │   ├── ProgressBar.tsx    # 进度条
    │   ├── LogStream.tsx      # 实时日志流
    │   ├── StatusBadge.tsx    # 状态标签
    │   ├── DagViewer.tsx      # 工作流 DAG 可视化
    │   └── YamlEditor.tsx     # 资源 YAML 编辑器
    ├── hooks/
    │   ├── useGrpc.ts         # invoke 封装
    │   ├── useStream.ts       # listen 封装（streaming）
    │   └── useRole.ts         # RBAC 角色上下文
    ├── lib/
    │   ├── tauri.ts           # Tauri invoke/listen 类型定义
    │   └── types.ts           # 数据类型（与 proto message 对应）
    └── styles/
        └── tokens.css         # design-system.md token 导入
```

### 5. 构建与分发

| 命令 | 用途 |
|------|------|
| `cargo tauri dev` | 开发模式：前端热重载 + Rust 增量编译 |
| `cargo tauri build` | 生产构建：生成 .dmg (macOS) / .msi (Windows) / .AppImage (Linux) |
| `cargo tauri build --ci` | CI 构建：无签名，用于测试 |

**CI 集成**：在现有 GitHub Actions workflow 中添加 GUI 构建步骤，生成各平台安装包作为 release artifact。

### 6. 分阶段实施

| 阶段 | 内容 | 验证点 |
|------|------|--------|
| Phase 1 | Tauri 脚手架 + gRPC 连接 + Ping/TaskList | mTLS 连通、TaskList 返回数据 |
| Phase 2 | 进度观察页（L1 列表 + L2 详情 + streaming） | TaskFollow 实时日志 |
| Phase 3 | 许愿池页（输入 → TaskCreate → FR 草稿预览） | 端到端许愿流程 |
| Phase 4 | 专家模式（资源/Agent/DAG） | ResourceDescribe、AgentList |
| Phase 5 | 打磨（动画、主题、可访问性、i18n 预留） | WCAG AA、Dark mode |

## 关键代码路径

| 文件 | 复用/参考 |
|------|----------|
| `crates/cli/src/client.rs` | gRPC 连接发现、ControlPlaneConfig、TLS 配置 |
| `crates/proto/src/lib.rs` | 48 个 RPC 的 Rust 类型定义 |
| `proto/orchestrator.proto` | RPC 接口完整定义 |
| `crates/daemon/src/control_plane.rs` | 理解 mTLS 验证逻辑（daemon 侧） |
| `crates/daemon/src/protection.rs` | 理解限流策略（GUI 作为新 subject） |
| `docs/design-system.md` | Liquid Glass 设计 token |
