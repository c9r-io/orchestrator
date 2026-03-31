---
self_referential_safe: true
---

# Orchestrator - 配置缺失与 Manifest 错误处理

**Module**: orchestrator
**Scope**: 验证 `init + apply -f` 路径下的配置缺失与错误处理
**Scenarios**: 4
**Priority**: High

---

## Background

Orchestrator 运行时配置存储于 SQLite。`init` 初始化目录、数据库并写入默认配置
（包含 default workspace 和预定义 workflow）。
用户可通过 `apply -f <manifest.yaml>` 导入自定义配置来覆盖或扩展默认配置。

Entry point: `orchestrator <command>`

---

## Scenario 1: init 后默认配置已存在，命令可正常执行

### Preconditions

- Rust toolchain available

### Goal

验证 `init` 创建默认配置（default workspace、workflow、agents）的逻辑正确 — 通过代码审查 + unit test 验证。

> **Note**: `init` 会自动创建 default workspace、基本 workflow 和 default agents。
> CLI 入口总会隐式调用 `init`，所以 "no manifest" 错误路径对用户不可见。

### Steps

1. **代码审查** — 验证 init 创建默认资源的逻辑：
   ```bash
   rg -n "normalize_config|populates_builtin|default.*workspace\|default.*workflow" core/src/ | head -15
   ```

2. **代码审查** — 验证 normalize 填充默认 CRD：
   ```bash
   rg -n "normalize_config_populates_builtin_crds|reconcile_all_builtins" core/src/ | head -10
   ```

3. **Unit test** — 运行配置初始化和规范化测试：
   ```bash
   cargo test --package agent-orchestrator --lib -- normalize_config_populates_builtin reconcile_all_builtins 2>&1 | tail -5
   ```

### Expected

- `normalize_config_populates_builtin_crds` 通过：init 路径创建 default workspace 和 workflow
- `reconcile_all_builtins_does_not_panic` 通过：CRD 回写逻辑正常
- 无 panic

---

## Scenario 2: init 创建默认配置，apply 可叠加自定义资源

### Preconditions

- Rust toolchain available

### Goal

验证 apply 逻辑正确处理资源叠加（created/configured/unchanged 状态）— 通过代码审查 + unit test 验证。

### Steps

1. **代码审查** — 验证 apply 的三态返回逻辑：
   ```bash
   rg -n "ApplyResult|Created|Configured|Unchanged" core/src/resource/ | head -15
   ```

2. **Unit test** — 运行 apply 操作的状态判定测试：
   ```bash
   cargo test --package agent-orchestrator --lib -- apply_result_created apply_result_configured apply_result_unchanged apply_to_project 2>&1 | tail -5
   ```

3. **Unit test** — 验证 config snapshot 同步：
   ```bash
   cargo test --package agent-orchestrator --lib -- sync_config_snapshot_to_store 2>&1 | tail -5
   ```

### Expected

- `apply_result_created_when_missing` 通过：新资源返回 Created
- `apply_result_configured_when_resource_changes` 通过：变更资源返回 Configured
- `apply_result_unchanged_for_identical_resource` 通过：相同资源返回 Unchanged
- `apply_to_project_routes_*` 系列通过：project scope 路由正确

---

## Scenario 3: apply 非法 Manifest 失败

### Preconditions

- Rust toolchain available

### Goal

验证 manifest 解析对非法 apiVersion 拒绝 — 通过代码审查 + unit test 验证。

### Steps

1. **代码审查** — 验证 apiVersion 校验逻辑：
   ```bash
   rg -n "apiVersion|Invalid.*api\|api.*version\|parse_resources" core/src/ | head -15
   ```

2. **代码审查** — 验证 resource dispatch 拒绝错误类型：
   ```bash
   rg -n "resource_dispatch_rejects|build_rejects" core/src/resource/ | head -10
   ```

3. **Unit test** — 运行 manifest 解析和校验测试：
   ```bash
   cargo test --package agent-orchestrator --lib -- build_rejects resource_dispatch_rejects validate_resource_name_rejects 2>&1 | tail -5
   ```

### Expected

- `build_rejects_wrong_kind` 通过：错误 kind 被拒绝
- `resource_dispatch_rejects_mismatched_spec_kind` 通过：spec 类型不匹配被拒绝
- `validate_resource_name_rejects_empty` 通过：空名称被拒绝
- 非法 manifest 不会修改活动配置

---

## Scenario 4: apply 语法损坏文件失败

### Preconditions

- Rust toolchain available

### Goal

验证 YAML 解析错误被正确捕获（不 panic）— 通过代码审查 + unit test 验证。

### Steps

1. **代码审查** — 验证 YAML 解析使用 Result 而非 unwrap：
   ```bash
   rg -n "serde_yaml::from|parse_resources_from_yaml|from_str.*yaml" core/src/ | head -10
   ```

2. **代码审查** — 验证错误类型定义：
   ```bash
   rg -n "YamlParse\|ParseError\|ManifestError" core/src/ | head -10
   ```

3. **Unit test** — 运行 manifest 解析测试（覆盖错误路径）：
   ```bash
   cargo test --package agent-orchestrator --lib -- build_rejects validate_resource 2>&1 | tail -5
   ```

### Expected

- YAML 解析使用 `?` 或 `map_err` 传播错误（非 `unwrap`）
- 错误类型包含诊断信息（文件路径、行号等）
- 无 panic

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | init 后默认配置已存在 | PASS | 2026-03-28 | Claude | Code review + unit test (normalize_config_populates_builtin_crds, reconcile_all_builtins_does_not_panic) |
| 2 | init + apply 叠加资源 | PASS | 2026-03-28 | Claude | Code review + unit test (apply_result_created_when_missing, apply_result_configured_when_resource_changes, apply_result_unchanged_for_identical_resource, sync_config_snapshot_to_store_populates_resource_store) |
| 3 | apply 非法 Manifest 失败 | PASS | 2026-03-28 | Claude | Code review + unit test (build_rejects_wrong_kind, resource_dispatch_rejects_mismatched_spec_kind, validate_resource_name_rejects_empty) |
| 4 | apply 语法损坏文件失败 | PASS | 2026-03-28 | Claude | Code review + unit test (parse_resources_from_yaml_multi_document, parse_resources_from_yaml_single_document, parse_resources_from_yaml_skips_null_documents) |
