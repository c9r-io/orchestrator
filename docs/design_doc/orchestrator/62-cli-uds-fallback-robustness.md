# Design Doc 62: CLI UDS 连接回退鲁棒性 (FR-050)

## Overview

改进 CLI `connect()` 函数的连接优先级，使本地开发场景下无需显式设置 `ORCHESTRATOR_SOCKET` 环境变量即可自动通过 UDS 连接 daemon。

## Motivation

当 daemon 通过 UDS 运行于本地时，若 `~/.orchestrator/control-plane/config.yaml` 残留了旧的 TCP 配置，CLI 会优先尝试 TCP 连接并报出不直观的错误（`failed to connect to https://127.0.0.1:51052`），迫使用户手动设置 `ORCHESTRATOR_SOCKET=data/orchestrator.sock`。

## Design

### 连接优先级（方案 B）

`connect()` 的发现顺序从三级扩展为五级：

```
1. ORCHESTRATOR_SOCKET 环境变量已设置（无显式 config）→ UDS
2. 显式 control-plane config（--control-plane-config 或 ORCHESTRATOR_CONTROL_PLANE_CONFIG）→ TCP/TLS
3. 本地 socket 文件存在（discover_socket_path()）→ UDS          ← 新增
4. 自动发现 ~/.orchestrator/control-plane/config.yaml → TCP/TLS
5. 回退 → UDS
```

核心变化：步骤 3 在自动发现 home-dir 配置之前检查本地 socket 文件是否存在，避免在 daemon 明确在本地运行时还去尝试 TCP。

### 实现细节

将原 `discover_control_plane_config()` 拆分为两个内部帮助函数：

- `discover_explicit_control_plane_config(explicit)` — 仅检查显式路径和 `ORCHESTRATOR_CONTROL_PLANE_CONFIG` 环境变量
- `discover_home_control_plane_config()` — 仅检查 `~/.orchestrator/control-plane/config.yaml`

原 `discover_control_plane_config()` 保留为测试辅助函数（`#[cfg(test)]`），内部依次调用上述两个函数。

### 受影响范围

- `crates/cli/src/client.rs` — `connect()`、`discover_explicit_control_plane_config()`、`discover_home_control_plane_config()`
- 不影响任何现有 RPC 行为
- 不影响 `--control-plane-config` 显式指定场景
- 不影响 `ORCHESTRATOR_SOCKET` 已设置的场景

## 测试

- `discover_explicit_config_returns_none_when_no_explicit` — 验证无显式配置时返回 None
- `discover_explicit_config_uses_explicit_path` — 验证显式路径被正确发现
- `local_socket_probe_precedes_home_config_in_connect_priority` — 验证本地 socket 路径在 home-dir 发现之前可被探测
- 已有 `connect_prefers_socket_when_env_is_present_and_no_explicit_config` 保持不变
