---
self_referential_safe: true
---

# QA: Unquoted JSON Extraction — Advanced (FR-031)

**Split from**: `docs/qa/orchestrator/90-unquoted-json-extraction.md`

## 前置条件

- orchestrator 已编译并可运行
- 项目根目录为 `{source_tree}`

## 场景 1: 文件路径 Value 修复（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test repair_unquoted_json_file_paths`

**预期结果:**
- [x] 测试通过
- [x] 包含 `/` 和 `.` 的文件路径 value 被正确引号包裹

## 场景 2: 端到端 Unquoted Regression Targets 提取（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test extract_array_unquoted_regression_targets`

**预期结果:**
- [x] 测试通过
- [x] 从 5 个 unquoted object 的 array 中正确提取 5 个 item
- [x] JSONPath `$.regression_targets` 解析成功

## 场景 3: 混合文本 + Unquoted JSON 提取（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test extract_array_mixed_text_unquoted`

**预期结果:**
- [x] 测试通过
- [x] 前后包含自然语言文本的 unquoted JSON 被正确提取

## 场景 4: extract_dynamic_items 全流程集成（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test test_extract_dynamic_items_unquoted_json`

**预期结果:**
- [x] 测试通过
- [x] 从 unquoted pipeline variable 中提取 2 个 dynamic item
- [x] item_id 和 vars 均正确映射

**Note:** Test is in `orchestrator-scheduler`, not `core`. Correct command:
```bash
cd {source_tree} && cargo test -p orchestrator-scheduler test_extract_dynamic_items_unquoted_json
```

## 场景 5: 标准 JSON 回归保护（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test extract_array_pure_json_still_works`

**预期结果:**
- [x] 测试通过
- [x] 标准 JSON 输入不经过 repair 路径，直接解析成功

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1 ✅ S2 ✅ S3 ✅ S4 ✅ (doc fixed: test is in scheduler crate) S5 ✅ |
