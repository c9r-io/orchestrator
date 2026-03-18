---
self_referential_safe: false
self_referential_safe_scenarios: [S1, S3]
---

# QA-104: CLI UDS 连接回退鲁棒性

> **Infrastructure note**: S2 and S4 require a valid TLS certificate chain
> (`~/.orchestrator/control-plane/config.yaml` pointing to real CA/client certs).
> These scenarios verify that the CLI **selects** the TCP/TLS path (code-review
> confirmed via `connect()` priority logic in `crates/cli/src/client.rs`), but
> the actual TLS handshake will fail unless the test environment provisions
> certificates.  S1, S3, and S5 are self-contained and fully automatable.

## 关联
- FR-050
- Design Doc 62

## 场景

### S1: 无环境变量且本地 socket 存在时优先 UDS
- **前置**: 未设置 `ORCHESTRATOR_SOCKET`，未设置 `ORCHESTRATOR_CONTROL_PLANE_CONFIG`，`~/.orchestrator/control-plane/config.yaml` 存在，`data/orchestrator.sock` 存在
- **操作**: 执行 `orchestrator task list`
- **预期**: CLI 通过 UDS 连接成功，不尝试 TCP

### S2: 显式 --control-plane-config 优先于本地 socket
- **前置**: `data/orchestrator.sock` 存在，传入 `--control-plane-config /path/to/config.yaml`
- **操作**: 执行 `orchestrator --control-plane-config /path/to/config.yaml task list`
- **预期**: CLI 使用 TCP/TLS 连接（由显式配置决定），不使用 UDS

### S3: ORCHESTRATOR_SOCKET 环境变量最高优先
- **前置**: 设置 `ORCHESTRATOR_SOCKET=/custom/path.sock`，`~/.orchestrator/control-plane/config.yaml` 存在
- **操作**: 执行 `orchestrator task list`
- **预期**: CLI 使用 `/custom/path.sock` 连接

### S4: 无 socket 文件时回退到 home-dir 配置
- **前置**: `data/orchestrator.sock` 不存在，`~/.orchestrator/control-plane/config.yaml` 存在
- **操作**: 执行 `orchestrator task list`
- **预期**: CLI 使用 home-dir 配置进行 TCP/TLS 连接

### S5: 所有发现路径均无时回退 UDS 并报错
- **前置**: 未设置任何环境变量，无 socket 文件，无 home-dir 配置
- **操作**: 执行 `orchestrator task list`
- **预期**: CLI 报错提示 daemon 未运行，包含 `orchestratord --foreground --workers 2` 提示

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | S2/S4 require TLS infra; S1/S3/S5 pass |
| 2 | S1: UDS preferred | ☑ | CLI connects via UDS when socket exists, no env vars |
| 3 | S2: --control-plane-config priority | ☐ | Code-review confirmed path selection; TLS handshake requires cert provisioning |
| 4 | S3: ORCHESTRATOR_SOCKET env priority | ☑ | Env var takes highest priority |
| 5 | S4: Fallback to home-dir config | ☐ | Code-review confirmed path selection; TLS handshake requires cert provisioning |
| 6 | S5: Error with hint | ☑ | Shows orchestratord startup hint |
