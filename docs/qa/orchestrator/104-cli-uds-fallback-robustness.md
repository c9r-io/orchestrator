# QA-104: CLI UDS 连接回退鲁棒性

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
