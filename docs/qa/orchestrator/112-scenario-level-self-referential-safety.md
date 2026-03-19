# QA 112: Scenario-Level Self-Referential Safety

**Scope**: 验证 FR-058 场景级安全分级机制的正确性，确认 prehook 过滤、CEL 变量、frontmatter 解析均按预期工作。

## Scenario 1: Frontmatter 解析 — 有场景白名单

### Preconditions
- QA 文档含 `self_referential_safe: false` 和 `self_referential_safe_scenarios: [S2, S3]`

### Steps
1. 检查解析函数：
   ```bash
   rg -n "parse_qa_doc_safe_scenarios" core/src/ticket.rs
   ```
2. 运行单元测试：
   ```bash
   cargo test --lib -p agent-orchestrator -- ticket::tests::test_parse_qa_doc_safe_scenarios
   ```

### Expected
- 函数存在且测试全部通过
- 返回 `["S2", "S3"]`

---

## Scenario 2: Frontmatter 解析 — 无场景白名单

### Steps
1. 验证缺少 `self_referential_safe_scenarios` 时返回空列表：
   ```bash
   cargo test --lib -p agent-orchestrator -- ticket::tests::test_parse_qa_doc_safe_scenarios_absent
   ```

### Expected
- 返回空 Vec

---

## Scenario 3: CEL 变量 — 非空场景列表允许通过

### Steps
1. 验证 CEL 表达式正确评估：
   ```bash
   cargo test --lib -p agent-orchestrator -- prehook::tests::test_self_referential_safe_scenarios_non_empty
   ```

### Expected
- `self_referential_safe || size(self_referential_safe_scenarios) > 0` 为 true（即使 `self_referential_safe` 为 false）

---

## Scenario 4: CEL 变量 — 空场景列表被跳过

### Steps
1. 验证空列表不通过过滤：
   ```bash
   cargo test --lib -p agent-orchestrator -- prehook::tests::test_self_referential_safe_scenarios_empty
   ```

### Expected
- `self_referential_safe || size(self_referential_safe_scenarios) > 0` 为 false

---

## Scenario 5: 覆盖率统计验证

### Steps
1. 统计文档级安全文档数：
   ```bash
   grep -rL "self_referential_safe: false" docs/qa/orchestrator/*.md docs/qa/self-bootstrap/*.md | wc -l
   ```
2. 统计场景级标注文档数：
   ```bash
   grep -rl "self_referential_safe_scenarios:" docs/qa/orchestrator/*.md docs/qa/self-bootstrap/*.md | wc -l
   ```

### Expected
- 文档级安全 ≥ 25
- 场景级标注 ≥ 15
- 总参与文档 ≥ 55（覆盖率 ≥ 40%）

> **Note**: FR-060 迭代 1-7 将多个文档从 `self_referential_safe: false` 转为 `true`（移除 `self_referential_safe_scenarios`），导致文档级安全数增加、场景级标注数减少，但总参与文档数保持增长。阈值已相应调整（场景级从 ≥30 降至 ≥15，因为更多文档升级为全文档级安全）。截至迭代 7：文档级安全 62，场景级标注 18，总参与 80。

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Frontmatter 解析 — 有场景白名单 | PASS | 2026-03-18 | | Function `parse_qa_doc_safe_scenarios` found at line 56; 5 tests passed |
| 2 | Frontmatter 解析 — 无场景白名单 | PASS | 2026-03-18 | | test_parse_qa_doc_safe_scenarios_absent passed |
| 3 | CEL 变量 — 非空场景列表允许通过 | PASS | 2026-03-18 | | test_self_referential_safe_scenarios_non_empty passed |
| 4 | CEL 变量 — 空场景列表被跳过 | PASS | 2026-03-18 | | test_self_referential_safe_scenarios_empty passed |
| 5 | 覆盖率统计验证 | PASS | 2026-03-19 | | Doc-level: 62 ≥ 25; Scenario-level: 18 ≥ 15; Total: 80 ≥ 55; Coverage: 58% ≥ 40% |
