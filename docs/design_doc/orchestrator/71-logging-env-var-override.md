# DD-071: Daemon 日志环境变量覆盖

## 概述

为 `orchestratord` 添加环境变量覆盖机制，允许通过 `ORCHESTRATOR_LOG`、`RUST_LOG`、`ORCHESTRATOR_LOG_FORMAT` 动态配置日志级别和输出格式。

## 设计决策

### 优先级链

```
ORCHESTRATOR_LOG > RUST_LOG > 硬编码默认值 (info)
```

- `ORCHESTRATOR_LOG`: 简单级别覆盖（trace/debug/info/warn/error），复用 `LogLevel::parse()`
- `RUST_LOG`: 标准 `tracing_subscriber::EnvFilter` 兼容，支持模块级过滤（如 `agent_orchestrator=trace`）
- 未设置时默认 `info` 级别

### 输出格式

```
ORCHESTRATOR_LOG_FORMAT=json → JSON 格式（适合 daemon 模式日志采集）
ORCHESTRATOR_LOG_FORMAT=pretty|text → Pretty 文本格式（默认）
```

复用 `LoggingFormat::parse()` 进行格式解析。JSON 模式强制关闭 ANSI。

### 关键代码路径

| 文件 | 变更 |
|------|------|
| `crates/daemon/Cargo.toml` | 添加 `env-filter`、`json` features |
| `crates/daemon/src/main.rs` | 替换硬编码 subscriber 为 env-aware 版本 |

### 向后兼容

- 未设置任何环境变量时，行为与修改前完全一致（info 级别、pretty 文本、ANSI 由 foreground/daemon 模式决定）
- `--foreground` flag 不受影响
- 现有 `ObservabilityConfig` 类型被复用，无新增配置模型
