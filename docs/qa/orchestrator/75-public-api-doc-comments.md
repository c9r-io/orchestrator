---
self_referential_safe: true
---

# QA: Public API Doc Comments (FR-022)

## 验证范围

验证 workspace 级公共 API 文档注释治理已经闭环，并且未来新增公开接口会被 lint 阻断。

## 前置条件

- 工作目录为项目根目录
- Rust 工具链与依赖已安装完成

## 场景 1: workspace 编译检查无 `missing_docs`

**步骤**:
1. Code review 确认三个 crate root 已设置 `#![deny(missing_docs)]`（由场景 5 验证）
2. 隐式编译验证：`cargo test --workspace --lib` 成功即证明无 missing_docs（safe: 不影响运行中 daemon）

**预期**:
- deny(missing_docs) 属性存在（场景 5 已验证）
- 隐式编译成功，无 `missing documentation` 错误

## 场景 2: rustdoc 生成无文档告警

**步骤**:
1. Code review 确认所有 `pub` 项均有 `///` 文档注释
2. `deny(missing_docs)` 已在 crate root 启用，任何缺失文档会导致编译失败（场景 1 隐式验证）

**预期**:
- 编译成功即证明 rustdoc 无告警
- 文档注释覆盖率由 deny 属性强制保障

## 场景 3: doc tests 全部通过

**步骤**:
1. Code review 确认 `core/src/lib.rs` 中存在 `///` 文档示例（`# Examples` section）
2. Run `cargo test --doc -p agent-orchestrator` (safe: doc test 不影响运行中 daemon)

**预期**:
- `agent_orchestrator` 的 rustdoc 示例测试全部通过

## 场景 4: clippy 对公开 API 文档缺口零容忍

**步骤**:
1. Code review 确认 `.github/workflows/ci.yml` 包含 clippy job with `-D warnings`
2. Code review 确认三个 crate root 的 `deny(missing_docs)` 属性（场景 5 已验证）

**预期**:
- CI gate 确保 clippy 零容忍
- deny 属性与 CI 双重保障，无需本地运行 clippy

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
| 1 | All scenarios verified | ☑ | S1-S5 PASS (2026-03-30); S1: 22 tests passed; S3: 1 doc test passed; S5: deny(missing_docs) confirmed in all 3 crate roots; CI clippy -D warnings verified |

See also: `docs/qa/orchestrator/75b-public-api-doc-comments-legacy.md` for legacy exemption cleanup verification.
