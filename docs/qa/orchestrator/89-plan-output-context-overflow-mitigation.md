---
self_referential_safe: true
---

# QA: Plan Output Context Overflow Mitigation (FR-036)

验证 plan output 自动摘要提取机制是否正确工作。

## 前置条件

- orchestrator 已编译并可运行
- 项目根目录为 `{source_tree}`

## 场景 1: Stream-JSON Plan Output 自动提取 Result（单元测试）

**步骤:**
1. 运行 `cargo test -p orchestrator-scheduler -- auto_capture_extracts_stream_json_result_for_spill`

**预期结果:**
- [x] 测试通过
- [x] 从包含 tool_use、tool_result、thinking 的 stream-json 输出中，仅提取 `{"type":"result","result":"..."}` 行的 result 字段
- [x] spill 文件只包含 plan 正文，不包含 session transcript

## 场景 2: 非 Stream-JSON 输出回退（单元测试）

**步骤:**
1. 运行 `cargo test -p orchestrator-scheduler -- auto_capture_falls_back_to_raw_stdout_for_non_stream_json`

**预期结果:**
- [x] 测试通过
- [x] 纯文本 stdout 原样写入 spill 文件（向后兼容）

## 场景 3: 大型 Transcript 小型 Result 不触发截断（单元测试）

**步骤:**
1. 运行 `cargo test -p orchestrator-scheduler -- auto_capture_stream_json_large_result_spills_only_extracted_text`

**预期结果:**
- [x] 测试通过
- [x] 100K+ 的原始 transcript 经提取后，spill 文件仅包含小型 plan 文本
- [x] inline pipeline 变量不包含 "truncated" 标记

## 场景 4: extract_stream_json_result 核心提取逻辑（单元测试）

**步骤:**
1. 运行 `cargo test -p agent-orchestrator -- extract_stream_json`

**预期结果:**
- [x] 测试通过（2 tests）
- [x] 多 result 行时取最后一条
- [x] 含 [REDACTED] 的行仍能提取

## 场景 5: Spill 截断边界处理（单元测试）

**步骤:**
1. 运行 `cargo test -p orchestrator-scheduler -- spill_to_file`

**预期结果:**
- [x] 测试通过（5+ tests）
- [x] UTF-8 多字节字符边界截断测试通过
- [x] 小值/大值/正好在 limit 处的分支均正确

---

## Checklist

| # | Check | Status | Notes |
|---|-------|--------|-------|
| 1 | All scenarios verified | ☑ | S1-S3 in orchestrator-scheduler (auto_capture tests), S4 in agent-orchestrator (extract_stream_json), S5 in orchestrator-scheduler (spill_to_file) |

See also: `docs/qa/orchestrator/89b-plan-output-spill-regression.md` for spill regression and stream-JSON extraction tests.
