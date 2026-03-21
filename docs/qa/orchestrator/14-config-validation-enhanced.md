---
self_referential_safe: true
---

# Orchestrator - 增强配置校验系统

**Module**: orchestrator
**Scope**: 验证增强的配置校验系统（YAML语法预检、分层校验、错误聚合）
**Scenarios**: 5
**Priority**: High

---

## Background

测试 manifest 预检与语义校验能力，通过代码审查和现有 unit test 验证：
- YAML 语法预检（反序列化前检测）
- 分层校验（语法 + 资源语义）
- 错误/警告聚合报告
- 路径存在性检查（警告 vs 错误）
- 路径安全检查（逃逸检测）

所有场景使用代码审查和现有 unit test — 无需 `cargo build` 或 CLI binary。

> **Note**: The package name in this document is `orchestrator-core` but the actual workspace package is `agent-orchestrator`. All test commands should use `-p agent-orchestrator` instead of `-p orchestrator-core`.

### Verification Command

```bash
cargo test --workspace --lib -- \
  parse_resources_from_yaml \
  ensure_within_root \
  validate_workflow_config \
  normalize_config
```

> **Note**: The `--workspace` verification command above uses `orchestrator-core` as the package filter name, which doesn't match the actual package. Use the per-scenario commands below instead, which correctly reference `-p agent-orchestrator`.

---

## Scenario 1: YAML 语法错误预检 (Code Review + Unit Test)

### Goal

验证 YAML 语法错误能被提前检测，不会导致程序崩溃。

### Steps

1. Review `core/src/resource/parse.rs` — `parse_resources_from_yaml()` 函数使用 `serde_yml::Deserializer` 逐文档反序列化，无效 YAML 返回错误而非 panic
2. Run unit tests:
   ```bash
   cargo test -p agent-orchestrator -- parse_resources_from_yaml
   ```

### Expected

- [ ] `parse_resources_from_yaml()` 对无效 YAML 返回 `Err`（不 panic、不崩溃）
- [ ] 单文档解析测试 `parse_resources_from_yaml_single_document` 通过
- [ ] 多文档解析测试 `parse_resources_from_yaml_multi_document` 通过
- [ ] 空文档跳过测试 `parse_resources_from_yaml_skips_null_documents` 通过

---

## Scenario 2: 多错误聚合报告 (Code Review + Unit Test)

### Goal

验证配置校验能识别多个资源级错误并逐个报告。

### Steps

1. Review `core/src/config_load/validate/tests.rs` — 校验函数对重复 step ID、无效 capture 配置等多种错误类型返回结构化错误
2. Review `core/src/config_load/build.rs` — `build_active_config()` 聚合多文档校验结果
3. Run unit tests:
   ```bash
   cargo test -p agent-orchestrator -- validate_workflow_config
   ```

### Expected

- [ ] `validate_workflow_config_rejects_duplicate_step_ids` 通过 — 验证重复 ID 检测
- [ ] `validate_workflow_config_rejects_json_path_on_exit_code_capture` 通过 — 验证无效 capture 检测
- [ ] `build_active_config_with_self_heal_*` 测试通过 — 验证多资源聚合构建

---

## Scenario 3: 路径不存在错误 (Code Review + Unit Test)

### Goal

验证不存在路径能被识别并返回错误。

### Steps

1. Review `core/src/config_load/validate/root_path.rs` — `ensure_within_root()` 使用 `std::fs::canonicalize()` 检测路径存在性
2. Run unit tests:
   ```bash
   cargo test -p agent-orchestrator -- ensure_within_root
   ```

### Expected

- [ ] `ensure_within_root_rejects_nonexistent_path` 通过 — 不存在路径返回错误
- [ ] 错误信息包含路径相关描述

---

## Scenario 4: 路径逃逸检测 (Code Review + Unit Test)

### Goal

验证路径逃逸尝试被阻止。

### Steps

1. Review `core/src/config_load/validate/root_path.rs` — `ensure_within_root()` 通过 `canonicalize()` 后比较路径前缀，防止 `../` 逃逸
2. Run unit tests:
   ```bash
   cargo test -p agent-orchestrator -- ensure_within_root
   ```

### Expected

- [ ] `ensure_within_root_rejects_path_outside_root` 通过 — 目录同级路径被拒绝
- [ ] `ensure_within_root_rejects_symlink_escaping_root` 通过 — symlink 逃逸被检测
- [ ] `ensure_within_root_accepts_child_path` 通过 — 合法子路径被接受
- [ ] `ensure_within_root_accepts_deeply_nested_child` 通过 — 深层嵌套路径被接受

---

## Scenario 5: 有效配置规范化输出 (Code Review + Unit Test)

### Goal

验证有效配置被接受并可输出规范化结果。

### Steps

1. Review `core/src/config_load/normalize/tests.rs` — 规范化逻辑对有效配置生成默认值、填充 builtin CRD、保持幂等性
2. Run unit tests:
   ```bash
   cargo test -p agent-orchestrator -- normalize_config
   ```

### Expected

- [ ] `normalize_config_populates_builtin_crds` 通过 — CRD 填充正确
- [ ] `normalize_config_rebuilds_resource_store_from_config_snapshot` 通过 — 资源 store 重建正确
- [ ] `normalize_config_idempotent_double_call` 通过 — 规范化幂等
- [ ] `normalize_config_clears_stale_store` 通过 — 过期状态清理正确

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | YAML 语法错误预检 | ✅ PASS | 2026-03-21 | Claude | 3/3 tests passed: single-doc, multi-doc, null-skip |
| 2 | 多错误聚合报告 | ✅ PASS | 2026-03-21 | Claude | 10/10 tests passed: duplicate IDs, invalid capture, probe rules |
| 3 | 路径不存在错误 | ✅ PASS | 2026-03-21 | Claude | 6/6 ensure_within_root tests passed (covers S3+S4) |
| 4 | 路径逃逸检测 | ✅ PASS | 2026-03-21 | Claude | symlink escape, ../ escape, child/deeply-nested all rejected correctly |
| 5 | 有效配置规范化输出 | ✅ PASS | 2026-03-21 | Claude | 6/6 normalize tests passed: CRD fill, store rebuild, idempotency |
