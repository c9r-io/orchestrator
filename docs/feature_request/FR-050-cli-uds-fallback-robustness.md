# FR-050: CLI UDS 连接回退鲁棒性

- **优先级**: P2
- **状态**: Proposed

## 背景

当前 CLI 客户端在 `connect()` 函数中（`crates/cli/src/client.rs`），只有当 `ORCHESTRATOR_SOCKET` 环境变量被显式设置时，才会直接使用 UDS 连接。否则会先尝试 `discover_control_plane_config()`，若该函数意外发现了配置文件（例如残留的旧配置），CLI 会尝试 TCP 连接并报错：

```
Error: failed to connect to https://127.0.0.1:51052
```

这在本地开发场景中是最常见的连接方式，但用户需要额外知道设置 `ORCHESTRATOR_SOCKET=data/orchestrator.sock` 才能正常工作。

## 问题描述

1. **UDS 应该是本地开发的默认首选**：当 `data/orchestrator.sock` 存在时，CLI 应优先尝试 UDS 连接，而非依赖 `ORCHESTRATOR_SOCKET` 环境变量。
2. **错误信息不直观**：当 TCP 连接失败时，错误信息没有提示用户尝试 UDS（例如 "try setting ORCHESTRATOR_SOCKET=data/orchestrator.sock"）。
3. **连接发现优先级不明确**：`discover_control_plane_config()` 的发现逻辑可能返回意外结果，导致用户困惑。

## 建议改进

### 方案 A（最小改动）：改善错误提示

当 TCP 连接失败时，在错误信息中追加提示：

```
Error: failed to connect to https://127.0.0.1:51052
Hint: If running locally, try: export ORCHESTRATOR_SOCKET=data/orchestrator.sock
```

### 方案 B（推荐）：UDS 优先回退

修改 `connect()` 函数逻辑：

```
1. 如果设置了 ORCHESTRATOR_SOCKET → 使用 UDS
2. 如果有显式 control-plane config → 使用 TCP
3. 如果 data/orchestrator.sock 存在 → 使用 UDS（新增）
4. 尝试 discover_control_plane_config() → 使用 TCP
5. 回退到 UDS
```

核心变化是在步骤 3 中新增对本地 socket 文件的存在性检查，避免在 daemon 明确在本地运行时还去尝试 TCP。

## 受影响范围

- `crates/cli/src/client.rs`: `connect()` 和 `discover_socket_path()` 函数
- 不影响任何现有 RPC 行为
- 不影响显式指定 `--control-plane-config` 的场景
