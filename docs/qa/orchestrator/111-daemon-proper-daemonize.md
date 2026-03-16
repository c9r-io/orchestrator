# QA-111 — orchestratord 真正 Daemon 化验证

> FR-057 | 设计文档: `docs/design_doc/orchestrator/69-daemon-proper-daemonize.md`

## 验证场景

### 场景 1: 默认模式后台运行

**前提:** 无 daemon 实例在运行

1. 执行 `orchestratord --workers 1`
2. 验证终端立即返回控制权（命令不阻塞）
3. 验证终端输出包含 `orchestratord daemonized (PID <N>)`
4. 执行 `orchestrator daemon status`
5. 验证输出 `orchestratord is running (PID <N>)`

**预期:** daemon 在后台运行，PID 文件已写入。

### 场景 2: 终端关闭后 daemon 存活

**前提:** 场景 1 已完成，daemon 在后台运行

1. 记录 daemon PID
2. 关闭启动终端（或发送 `kill -HUP <PID>`）
3. 等待 2 秒
4. 在新终端执行 `orchestrator daemon status`

**预期:** daemon 继续运行，不因 SIGHUP 退出。

### 场景 3: daemon stop 优雅停止

**前提:** daemon 在后台运行

1. 执行 `orchestrator daemon stop`
2. 验证输出 `stopping orchestratord (PID <N>)...`
3. 验证输出 `orchestratord stopped`
4. 执行 `orchestrator daemon status`
5. 验证输出 `orchestratord is not running`
6. 验证 `data/daemon.pid` 文件已删除

**预期:** daemon 优雅关闭，PID 文件被清理。

### 场景 4: --foreground 模式保持前台

**前提:** 无 daemon 实例在运行

1. 执行 `orchestratord --foreground --workers 1`
2. 验证命令不返回控制权（前台运行）
3. 验证日志输出到 stdout（带 ANSI 颜色）
4. 按 Ctrl+C 停止

**预期:** 行为与改动前一致。

### 场景 5: daemon 模式日志写入文件

**前提:** 无 daemon 实例在运行

1. 删除 `data/daemon.log`（如存在）
2. 执行 `orchestratord --workers 1`
3. 等待 3 秒
4. 检查 `data/daemon.log` 文件存在且有内容
5. 验证日志不包含 ANSI 转义序列

**预期:** daemon 模式日志正确写入文件。

### 场景 6: daemon status 对非运行状态的处理

1. 确保无 daemon 运行
2. 删除 `data/daemon.pid`（如存在）
3. 执行 `orchestrator daemon status`
4. 验证输出 `orchestratord is not running`

**预期:** 无 PID 文件时正确报告未运行。

### 场景 7: daemon stop 对已停止 daemon 的处理

1. 确保无 daemon 运行
2. 执行 `orchestrator daemon stop`
3. 验证输出包含 `not running`

**预期:** 不报错，正确提示未运行。

### 场景 8: 陈旧 PID 文件清理

1. 手动写入一个不存在进程的 PID 到 `data/daemon.pid`
2. 执行 `orchestrator daemon stop`
3. 验证输出包含 `stale PID file removed`
4. 验证 `data/daemon.pid` 已被删除

**预期:** stop 命令识别并清理陈旧 PID 文件。

## 自动化测试

- `cargo test -p orchestratord` — 16 个现有单元测试全部通过
- `cargo test -p orchestrator-cli` — CLI 解析测试通过
- `cargo clippy -p orchestratord -p orchestrator-cli --no-deps` — 无警告
