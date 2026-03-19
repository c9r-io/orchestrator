---
self_referential_safe: true
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
- **前置**: Repository root is the current working directory. Rust toolchain available.
- **操作**:
  1. 代码审查: 确认 `crates/cli/src/client.rs` 中 `connect()` 函数的优先级逻辑:
     ```bash
     rg -n "fn connect\b|fn discover_control_plane_config\b|explicit.*Some" crates/cli/src/client.rs
     ```
  2. 运行 unit test 验证显式配置优先:
     ```bash
     cargo test -p orchestrator-cli --lib -- discover_control_plane_config_prefers_explicit_path --nocapture
     ```
- **预期**: unit test 通过, code review 确认显式 `--control-plane-config` 在优先级链中排第一

### S3: ORCHESTRATOR_SOCKET 环境变量最高优先
- **前置**: 设置 `ORCHESTRATOR_SOCKET=/custom/path.sock`，`~/.orchestrator/control-plane/config.yaml` 存在
- **操作**: 执行 `orchestrator task list`
- **预期**: CLI 使用 `/custom/path.sock` 连接

### S4: 无 socket 文件时回退到 home-dir 配置
- **前置**: Repository root is the current working directory. Rust toolchain available.
- **操作**:
  1. 代码审查: 确认 `connect()` 中 socket 不存在时的回退链:
     ```bash
     rg -n "local_socket_probe\|home.*config\|fallback" crates/cli/src/client.rs
     ```
  2. 运行 unit test 验证 socket → home-dir 回退:
     ```bash
     cargo test -p orchestrator-cli --lib -- local_socket_probe_precedes_home_config --nocapture
     ```
- **预期**: unit test 通过, code review 确认 socket 探测失败后回退到 `~/.orchestrator/control-plane/config.yaml`

### S5: 所有发现路径均无时报错
- **前置**: Repository root is the current working directory. Rust toolchain available.
- **操作**:
  1. 代码审查: 确认所有发现路径失败时的错误处理:
     ```bash
     rg -n "orchestratord.*foreground\|daemon.*not.*running\|No.*control.*plane" crates/cli/src/client.rs
     ```
  2. 运行 unit test 验证 env 变量优先级:
     ```bash
     cargo test -p orchestrator-cli --lib -- connect_prefers_socket_when_env_is_present --nocapture
     ```
- **预期**: code review 确认无配置时输出包含 `orchestratord --foreground --workers 2` 提示; unit test 通过

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | S2/S4/S5 rewritten for safe mode: code review + unit test |
| 2 | S1: UDS preferred | ☑ | CLI connects via UDS when socket exists, no env vars |
| 3 | S2: --control-plane-config priority | ☐ | Rewritten: unit test + code review of priority logic |
| 4 | S3: ORCHESTRATOR_SOCKET env priority | ☑ | Env var takes highest priority |
| 5 | S4: Fallback to home-dir config | ☐ | Rewritten: unit test + code review of fallback chain |
| 6 | S5: Error with hint | ☐ | Rewritten: unit test + code review of error path |
