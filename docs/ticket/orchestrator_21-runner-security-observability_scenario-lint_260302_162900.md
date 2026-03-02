# Ticket: QA Doc Scenario Limit Violation In Runner Security Observability

**Created**: 2026-03-02 16:29:00
**QA Document**: `docs/qa/orchestrator/21-runner-security-observability.md`
**Scenario**: #lint
**Status**: FAILED

---

## Test Content

Validate repository QA documentation guardrails after adding the new structured logging QA document.

---

## Expected Result

`./scripts/qa-doc-lint.sh` exits with code `0`, and every orchestrator QA document stays within the repository rule of at most 5 numbered scenarios.

---

## Actual Result

`./scripts/qa-doc-lint.sh` exits non-zero and reports:

- `Too many scenarios (8): docs/qa/orchestrator/21-runner-security-observability.md`

---

## Repro Steps

1. From repository root, run:
   ```bash
   ./scripts/qa-doc-lint.sh
   ```
2. Observe the scenario-count validation output.

---

## Evidence

**UI/CLI Output**:
```text
[qa-doc-lint] Checking banned patterns...
[qa-doc-lint] Checking legacy sqlite/global-fixture reset patterns...
[qa-doc-lint] Checking task create commands require --project...
[qa-doc-lint] Checking scenario count (<=5) for orchestrator docs...
[qa-doc-lint] Too many scenarios (8): docs/qa/orchestrator/21-runner-security-observability.md
[qa-doc-lint] FAILED
```

**Service Logs**:
```text
N/A
```

**DB Checks (if applicable)**:
```sql
-- N/A
```

---

## Analysis

**Root Cause**: Existing QA documentation drift. The runner security/observability document was expanded beyond the repository max-scenario rule and was not split into multiple files.
**Severity**: Medium
**Related Components**: Documentation / QA
