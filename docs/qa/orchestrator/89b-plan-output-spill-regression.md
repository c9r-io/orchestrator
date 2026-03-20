---
self_referential_safe: true
---

# QA: Plan Output — Spill Regression & Stream-JSON Extraction (FR-036)

**Split from**: `docs/qa/orchestrator/89-plan-output-context-overflow-mitigation.md`

## 前置条件

- orchestrator 已编译并可运行
- 项目根目录为 `{source_tree}`

## 场景 1: 现有 Spill 机制回归测试

**步骤:**
1. 运行 `cargo test -p orchestrator-scheduler -- spill`

**预期结果:**
- [x] 所有 14 个 spill 相关测试全部通过
- [x] UTF-8 多字节字符边界截断测试通过
- [x] spill_to_file 小值/大值分支均正确
- [x] spill_large_var inline/file 分支均正确

## 场景 2: extract_stream_json_result 提取逻辑测试

**步骤:**
1. 运行 `cargo test -p agent-orchestrator -- extract_stream_json`

**预期结果:**
- [x] 所有 extract_stream_json 测试通过（2 tests）
- [x] 多 result 行时取最后一条
- [x] 含 [REDACTED] 的行仍能提取
- [x] 无 result 行时返回 None

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | Scenario 1 (spill tests) | ✅ | 14 tests pass in orchestrator-scheduler crate |
| 2 | Scenario 2 (extract_stream_json) | ✅ | 2 tests pass in agent-orchestrator crate |
