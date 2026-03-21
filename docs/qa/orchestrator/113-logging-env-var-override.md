---
self_referential_safe: true
---

# QA-113: Daemon 日志环境变量覆盖

**关联**: FR-061 / DD-071
**Scope**: 验证 `ORCHESTRATOR_LOG`、`RUST_LOG`、`ORCHESTRATOR_LOG_FORMAT` 环境变量覆盖机制

## 场景 1: ORCHESTRATOR_LOG 设置日志级别

**步骤**:
1. **Code review** — 确认 `main.rs` 读取 `ORCHESTRATOR_LOG` 并通过 `LogLevel::parse()` 解析：
   ```bash
   rg -n "ORCHESTRATOR_LOG" crates/daemon/src/main.rs
   ```
2. **Unit test** — 验证 `LogLevel::parse()` 接受各级别：
   ```bash
   cargo test -p orchestrator-config -- level_parse_accepts_common_variants
   ```

**预期**:
- `ORCHESTRATOR_LOG` 被读取并解析为 `LogLevel`
- `LogLevel::parse()` 测试通过（trace/debug/info/warn/error 均可解析）

## 场景 2: RUST_LOG 作为回退

**步骤**:
1. **Code review** — 确认 `RUST_LOG` 通过 `EnvFilter::try_from_default_env()` 读取：
   ```bash
   rg -n "try_from_default_env|RUST_LOG" crates/daemon/src/main.rs
   ```

**预期**:
- 当 `ORCHESTRATOR_LOG` 未设置时，使用 `RUST_LOG`（标准 tracing 行为）
- 当两者均未设置时，默认 `info` 级别

## 场景 3: ORCHESTRATOR_LOG_FORMAT=json 输出 JSON

**步骤**:
1. **Code review** — 确认 `main.rs` 根据 format 选择 `.json()` 或 `.pretty()` subscriber：
   ```bash
   rg -n "ORCHESTRATOR_LOG_FORMAT|LoggingFormat|\.json\(\)" crates/daemon/src/main.rs
   ```
2. **Unit test** — 验证 `LoggingFormat::parse()` 接受 json/text/pretty：
   ```bash
   cargo test -p orchestrator-config -- format_parse_accepts_common_variants
   ```

**预期**:
- `json` 模式使用 `tracing_subscriber::fmt().json()` 并关闭 ANSI
- `pretty`/`text` 模式使用默认 fmt subscriber

## 场景 4: 未设置环境变量时向后兼容

**步骤**:
1. **Code review** — 确认默认值链：
   ```bash
   rg -n "unwrap_or_default|unwrap_or_else|EnvFilter::new" crates/daemon/src/main.rs
   ```

**预期**:
- 无环境变量时：info 级别、Pretty 文本格式、ANSI 由 foreground 模式决定
- 行为与 FR-061 之前完全一致

## 场景 5: tracing-subscriber features 配置正确

**步骤**:
1. **Code review** — 确认 Cargo.toml 包含 `env-filter` 和 `json` features：
   ```bash
   rg -n "tracing-subscriber" crates/daemon/Cargo.toml
   ```
2. **Workspace test** — 确认编译和测试通过：
   ```bash
   cargo test --workspace --lib
   ```

**预期**:
- features 包含 `fmt`, `ansi`, `env-filter`, `json`
- 全部 409+ 单元测试通过

---

## Checklist

| # | Scenario | Status | Notes |
|---|----------|--------|-------|
| 1 | ORCHESTRATOR_LOG 设置日志级别 | ✅ | Code review + unit test passed |
| 2 | RUST_LOG 作为回退 | ✅ | Code review confirmed: try_from_default_env fallback |
| 3 | ORCHESTRATOR_LOG_FORMAT=json 输出 JSON | ✅ | Code review + unit test passed |
| 4 | 未设置环境变量时向后兼容 | ✅ | Code review confirmed: info default + pretty fmt |
| 5 | tracing-subscriber features 配置正确 | ✅ | 409 workspace tests passed |
