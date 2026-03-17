---
self_referential_safe: false
---

# QA: Plan Output — Spill Regression & Stream-JSON Extraction (FR-036)

**Split from**: `docs/qa/orchestrator/89-plan-output-context-overflow-mitigation.md`

## 前置条件

- orchestrator 已编译并可运行
- 项目根目录为 `{source_tree}`

## 场景 1: 现有 Spill 机制回归测试

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test spill`

**预期结果:**
- [ ] 所有 20+ 个 spill 相关测试全部通过
- [ ] UTF-8 多字节字符边界截断测试通过
- [ ] spill_to_file 小值/大值分支均正确

## 场景 2: extract_stream_json_result 提取逻辑测试

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test extract_stream_json`

**预期结果:**
- [ ] 所有 extract_stream_json 测试通过
- [ ] 多 result 行时取最后一条
- [ ] 含 [REDACTED] 的行仍能提取
- [ ] 无 result 行时返回 None

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☐ | |
