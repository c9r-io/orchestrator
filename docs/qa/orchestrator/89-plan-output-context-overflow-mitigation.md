# QA: Plan Output Context Overflow Mitigation (FR-036)

验证 plan output 自动摘要提取机制是否正确工作。

## 前置条件

- orchestrator 已编译并可运行
- 项目根目录为 `{source_tree}`

## 场景 1: Stream-JSON Plan Output 自动提取 Result（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test auto_capture_extracts_stream_json_result_for_spill`

**预期结果:**
- [ ] 测试通过
- [ ] 从包含 tool_use、tool_result、thinking 的 stream-json 输出中，仅提取 `{"type":"result","result":"..."}` 行的 result 字段
- [ ] spill 文件只包含 plan 正文，不包含 session transcript

## 场景 2: 非 Stream-JSON 输出回退（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test auto_capture_falls_back_to_raw_stdout`

**预期结果:**
- [ ] 测试通过
- [ ] 纯文本 stdout 原样写入 spill 文件（向后兼容）

## 场景 3: 大型 Transcript 小型 Result 不触发截断（单元测试）

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test auto_capture_stream_json_large_result`

**预期结果:**
- [ ] 测试通过
- [ ] 100K+ 的原始 transcript 经提取后，spill 文件仅包含小型 plan 文本
- [ ] inline pipeline 变量不包含 "truncated" 标记

## 场景 4: Mock Agent 端到端 — stream-json plan output 提取并传递给下游 step（集成测试）

使用 echo 命令模拟 stream-json agent，验证完整的 plan → qa_doc_gen pipeline 变量传递。

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test stream_json_plan_output_extracts_result_text`

**预期结果:**
- [ ] 测试通过
- [ ] mock plan agent 输出包含 `{"type":"tool_use",...}` 和 `{"type":"result",...}` 的 JSONL
- [ ] `plan_output.txt` spill 文件仅包含提取后的 plan 正文
- [ ] spill 文件不包含 `tool_use`、`tool_result` 等 session transcript 行
- [ ] 下游 qa_doc_gen step 的 command 中 `{plan_output}` 已展开为 plan 正文（非截断形式）
- [ ] 下游 qa_doc_gen step 的 command 中 `{plan_output_path}` 指向正确的 spill 文件

## 场景 5: Mock Agent 端到端 — 非 stream-json 大文件 spill 仍正常截断（集成测试）

验证对于非 stream-json 输出（如 printf 裸文本），原有 spill 截断逻辑不受影响。

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test large_plan_output_spills_to_file`

**预期结果:**
- [ ] 测试通过
- [ ] 超过 PIPELINE_VAR_INLINE_LIMIT 的 printf 裸文本输出正常 spill 到文件
- [ ] 下游 step command 中的 `{plan_output}` 包含 "truncated" 截断标记
- [ ] `{plan_output_path}` 指向包含完整内容的 spill 文件

## 场景 6: 现有 Spill 机制回归测试

**步骤:**
1. 运行 `cd {source_tree}/core && cargo test spill`

**预期结果:**
- [ ] 所有 20+ 个 spill 相关测试全部通过
- [ ] UTF-8 多字节字符边界截断测试通过
- [ ] spill_to_file 小值/大值分支均正确

## 场景 7: extract_stream_json_result 提取逻辑测试

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
