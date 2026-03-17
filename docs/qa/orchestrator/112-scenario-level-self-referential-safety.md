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
- 文档级安全 ≥ 24
- 场景级标注 ≥ 32
- 总参与文档 ≥ 56（覆盖率 ≥ 40%）

---

## Checklist

| # | Scenario | Status | Test Date | Tester | Notes |
|---|----------|--------|-----------|--------|-------|
| 1 | Frontmatter 解析 — 有场景白名单 | ☐ | | | |
| 2 | Frontmatter 解析 — 无场景白名单 | ☐ | | | |
| 3 | CEL 变量 — 非空场景列表允许通过 | ☐ | | | |
| 4 | CEL 变量 — 空场景列表被跳过 | ☐ | | | |
| 5 | 覆盖率统计验证 | ☐ | | | |
