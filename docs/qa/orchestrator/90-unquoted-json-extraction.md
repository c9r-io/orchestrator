# QA: generate_items 对 LLM 非标准 JSON 输出的容错解析 (FR-031)

验证 `extract_json_array` 对 LLM 常见非标准 JSON 输出的容错解析能力。

## 前置条件

- orchestrator 已编译并可运行
- 项目根目录为 `{source_tree}`

## 场景 1: Unquoted Key + Unquoted Value 修复（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test repair_unquoted_json_keys_and_values`

**预期结果:**
- [ ] 测试通过
- [ ] `{id: docs/qa/foo.md, name: test}` 被修复为合法 JSON
- [ ] key 和 value 均被加上双引号

## 场景 2: 嵌套 Array 中的 Unquoted JSON（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test repair_unquoted_json_nested_array`

**预期结果:**
- [ ] 测试通过
- [ ] `{items: [{id: a}, {id: b}]}` 正确修复并解析出 2 个元素

## 场景 3: 合法 JSON 幂等性（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test repair_unquoted_json_preserves_valid`

**预期结果:**
- [ ] 测试通过
- [ ] 已引号的 JSON 输入输出完全相同

## 场景 4: 混合引号处理（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test repair_unquoted_json_mixed_quoted`

**预期结果:**
- [ ] 测试通过
- [ ] `{"id": "a", name: b}` — 已引号的 key 保留，未引号的 key 被修复

## 场景 5: 原始类型保留（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test repair_unquoted_json_numbers_bools_null`

**预期结果:**
- [ ] 测试通过
- [ ] 数字 42、布尔 true、null 保持原始类型，不被引号包裹

## 场景 6: 文件路径 Value 修复（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test repair_unquoted_json_file_paths`

**预期结果:**
- [ ] 测试通过
- [ ] 包含 `/` 和 `.` 的文件路径 value 被正确引号包裹

## 场景 7: 端到端 Unquoted Regression Targets 提取（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test extract_array_unquoted_regression_targets`

**预期结果:**
- [ ] 测试通过
- [ ] 从 5 个 unquoted object 的 array 中正确提取 5 个 item
- [ ] JSONPath `$.regression_targets` 解析成功

## 场景 8: 混合文本 + Unquoted JSON 提取（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test extract_array_mixed_text_unquoted`

**预期结果:**
- [ ] 测试通过
- [ ] 前后包含自然语言文本的 unquoted JSON 被正确提取

## 场景 9: extract_dynamic_items 全流程集成（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test test_extract_dynamic_items_unquoted_json`

**预期结果:**
- [ ] 测试通过
- [ ] 从 unquoted pipeline variable 中提取 2 个 dynamic item
- [ ] item_id 和 vars 均正确映射

## 场景 10: 标准 JSON 回归保护（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test extract_array_pure_json_still_works`

**预期结果:**
- [ ] 测试通过
- [ ] 标准 JSON 输入不经过 repair 路径，直接解析成功

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
