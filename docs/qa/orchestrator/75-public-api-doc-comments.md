# QA: Public API Doc Comments (FR-022)

## 验证范围

验证 workspace 级公共 API 文档注释治理已经闭环，并且未来新增公开接口会被 lint 阻断。

## 前置条件

- 工作目录为项目根目录
- Rust 工具链与依赖已安装完成

## 场景 1: workspace 编译检查无 `missing_docs`

**步骤**:
```bash
cargo check --workspace --all-targets
```

**预期**:
- 命令执行成功。
- 输出中不出现 `missing documentation`。

## 场景 2: rustdoc 生成无文档告警

**步骤**:
```bash
cargo doc --workspace --no-deps
```

**预期**:
- 命令执行成功。
- 输出中不出现 `missing documentation`。

## 场景 3: doc tests 全部通过

**步骤**:
```bash
cargo test --doc --workspace
```

**预期**:
- 命令执行成功。
- `agent_orchestrator` 的 rustdoc 示例测试全部通过。

## 场景 4: clippy 对公开 API 文档缺口零容忍

**步骤**:
```bash
cargo clippy --workspace --all-targets -- -D warnings
```

**预期**:
- 命令执行成功。
- 不出现 `missing_docs` 或其他 warning。

## 场景 5: crate root 已进入 `deny(missing_docs)` 收口态

**步骤**:
```bash
rg -n '#!\\[deny\\(missing_docs\\)\\]' core/src/lib.rs crates/cli/src/main.rs crates/daemon/src/main.rs
```

**预期**:
- 三个文件都能命中。
- 说明 `core`、`orchestrator-cli`、`orchestratord` 已进入强制文档门禁。

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |

See also: `docs/qa/orchestrator/75b-public-api-doc-comments-legacy.md` for legacy exemption cleanup verification.
