# FR-061: Daemon 日志环境变量覆盖

**状态**: Proposed
**优先级**: P2

## 背景

当前 daemon 的 tracing subscriber 使用硬编码配置初始化（`crates/daemon/src/main.rs:122-127`），不支持通过环境变量动态调整日志级别和输出格式。

`ObservabilityConfig` 已在 `crates/orchestrator-config/src/config/observability.rs` 中定义了完整的日志配置模型：

- `LogLevel` 枚举（trace/debug/info/warn/error）及 `FromStr` 实现
- `LoggingFormat` 枚举（text/json/pretty）及 `FromStr` 实现
- `ConsoleLoggingConfig`、`FileLoggingConfig` 配置结构

但 daemon bootstrap 路径未读取这些配置，也未支持环境变量覆盖。

### 来源

FR-060 迭代 10 full-QA 回归中发现 QA-36 S1/S3 引用了不存在的环境变量功能。

## 需求

1. daemon 启动时读取以下环境变量（优先级高于 YAML config）：
   - `ORCHESTRATOR_LOG` — 日志级别（对应 `LogLevel`）
   - `RUST_LOG` — 与 `tracing_subscriber::EnvFilter` 兼容的过滤表达式（优先级低于 `ORCHESTRATOR_LOG`）
   - `ORCHESTRATOR_LOG_FORMAT` — 输出格式（对应 `LoggingFormat`）

2. 优先级链：环境变量 > YAML config > 硬编码默认值

3. 已有 `ObservabilityConfig` parsing 逻辑应被复用，不重复实现

## 验收标准

1. `ORCHESTRATOR_LOG=debug orchestratord --foreground` 输出 debug 级别日志
2. `ORCHESTRATOR_LOG_FORMAT=json orchestratord --foreground` 输出 JSON 格式日志
3. `RUST_LOG=agent_orchestrator=trace orchestratord --foreground` 启用模块级 trace
4. 未设置环境变量时行为不变（向后兼容）
5. 4 个现有 observability 单元测试继续通过

## 实现提示

- 在 `crates/daemon/src/main.rs` 的 tracing subscriber 初始化路径中加入 `EnvFilter::try_from_default_env()` 或自定义 env 读取
- `LoggingFormat::Json` 对应 `tracing_subscriber::fmt().json()`
- 可参考 `tracing_subscriber::EnvFilter` 的标准 `RUST_LOG` 兼容实现

## 约束

1. 不改变现有 `--verbose` CLI flag 的行为
2. 不引入新的 crate 依赖（`tracing-subscriber` 已有 `env-filter` feature）
3. 变更范围限制在 daemon 启动路径
