---
lifecycle: active
related_fr: FR-050
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
- **前置**: 未设置 `ORCHESTRATOR_SOCKET`，未设置 `ORCHESTRATOR_CONTROL_PLANE_CONFIG`，`data/orchestrator.sock` 存在（symlink 到 `~/.orchestratord/orchestrator.sock`）
- **操作**: 执行 `orchestrator task list`
- **预期**: CLI 通过 UDS 连接成功，不尝试 TCP

> **Note (FR-163 更新):** UDS 优先路径不依赖 control-plane 配置，这一点不变；但步骤 3
> 已从"检查 socket 文件是否存在"改为**连接探测**——存在但没人监听的陈旧 socket
> 不再截住发现链，会继续落到步骤 4。故 S1 的前置需要的是一个**活着的** daemon，
> 而不只是一个 socket 文件。
>
> 本注记原先写的是 `~/.orchestratord/control-plane/config.yaml`。那个路径从来没有
> 任何东西写过——daemon 写的是 `~/.orchestrator/control-plane/config.yaml`，
> 差一个字符，自动发现因此从未在 daemon 自己的产物上生效过。FR-163 已把两侧对齐，
> 详见 [DD-178](../../design_doc/orchestrator/178-runtime-layout-single-source.md)
> 与 [QA-215](215-connectivity-path-single-source.md)。

### S2: 显式 --control-plane-config 优先于本地 socket
- **前置**: Repository root is the current working directory. Rust toolchain available.
- **操作**:
  1. 代码审查: 确认 `crates/cli/src/client.rs` 中 `connect()` 函数的优先级逻辑:
     ```bash
     rg -n "fn connect\b|fn discover_control_plane_config\b|explicit.*Some" crates/cli/src/client.rs
     ```
  2. 运行 unit test 验证显式配置优先:
     ```bash
     cargo test -p orchestrator-client -- discover_control_plane_config_prefers_explicit_path --nocapture
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
     cargo test -p orchestrator-client -- local_socket_probe_precedes_home_config_in_connect_priority --nocapture
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
     cargo test -p orchestrator-client -- connect_prefers_socket_when_env_is_present_and_no_explicit_config --nocapture
     ```
- **预期**: code review 确认无配置时输出包含 `orchestratord --foreground --workers 2` 提示; unit test 通过

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S2/S4/S5: code review + unit test all passed |
| 2 | S1: UDS preferred | ☑ | CLI connects via UDS when socket exists, no env vars |
| 3 | S2: --control-plane-config priority | ☑ | Unit test passed; priority chain confirmed in code review |
| 4 | S3: ORCHESTRATOR_SOCKET env priority | ☑ | Env var takes highest priority |
| 5 | S4: Fallback to home-dir config | ☑ | Unit test passed; fallback chain confirmed in code review |
| 6 | S5: Error with hint | ☑ | Unit test passed; error path confirmed in code review |
