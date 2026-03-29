---
self_referential_safe: true
---

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

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | 2026-03-29: S1-S5 all passed |

See also: `docs/qa/orchestrator/90b-unquoted-json-extraction-advanced.md` for file path repair, e2e extraction, and regression tests.
